//! Ponte entre uma placa de audio (ASIO no Windows, CoreAudio no macOS) e a rede AES67,
//! controlada por um painel no navegador (http://127.0.0.1:8787).
//!
//!   entradas da placa -> matriz TX -> fluxos AES67 (visiveis no Dante Controller)
//!   fluxos AES67 da rede -> matriz RX -> saidas da placa
//!
//! A parte de rede fica no crate `aoip-engine`; aqui fica so a placa (cpal).
use aoip_core::state::Shared;
use aoip_core::{ui, RATE};
use aoip_engine::{open_browser, refresh_status, report, report_stats, resolve_iface, selftest, start_network, Network, Res};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::net::TcpListener;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Duration;

/// Os drivers virtuais deste mesmo projeto: eles ja sao a ponte com a rede, entao nao entra
/// na lista de placas (abri-lo aqui so colocaria um segundo remetente no fluxo dele).
const OWN_DRIVERS: [&str; 2] = ["AoIP Bridge ASIO", "AoIP Bridge"]; // Windows, macOS

fn audio_host() -> cpal::Host {
    #[cfg(windows)]
    if let Ok(h) = cpal::host_from_id(cpal::HostId::Asio) {
        return h;
    }
    cpal::default_host()
}

fn list_devices(host: &cpal::Host) -> Vec<(String, usize, usize)> {
    let Ok(devices) = host.devices() else { return vec![] };
    let mut out: Vec<(String, usize, usize)> = Vec::new();
    for d in devices {
        let Ok(name) = d.name() else { continue };
        if OWN_DRIVERS.contains(&name.as_str()) {
            continue;
        }
        let ins = d.default_input_config().map(|c| c.channels() as usize).unwrap_or(0);
        let outs = d.default_output_config().map(|c| c.channels() as usize).unwrap_or(0);
        match out.iter_mut().find(|x| x.0 == name) {
            Some(x) => {
                x.1 = x.1.max(ins);
                x.2 = x.2.max(outs);
            }
            None => out.push((name, ins, outs)),
        }
    }
    out
}

fn find_device(host: &cpal::Host, name: &str, input: bool) -> Option<cpal::Device> {
    let usable = |d: &cpal::Device| if input { d.default_input_config().is_ok() } else { d.default_output_config().is_ok() };
    let all: Vec<cpal::Device> = host.devices().ok()?.filter(usable).collect();
    let lower = name.to_lowercase();
    let exact = all.iter().position(|d| d.name().map(|n| n == name).unwrap_or(false));
    let partial = || all.iter().position(|d| d.name().map(|n| n.to_lowercase().contains(&lower)).unwrap_or(false));
    let idx = exact.or_else(partial)?;
    all.into_iter().nth(idx)
}

/// Escolhe como abrir a placa. `want` = taxa pedida; a primeira que a placa aceitar vale.
/// Entre as opcoes da placa naquela taxa, fica a de mais canais (e, no empate, o mesmo
/// formato de amostra que a placa usa por padrao).
fn pick_config(dev: &cpal::Device, input: bool, want: &[u32]) -> Res<cpal::SupportedStreamConfig> {
    let default = if input { dev.default_input_config()? } else { dev.default_output_config()? };
    let ranges: Vec<cpal::SupportedStreamConfigRange> =
        if input { dev.supported_input_configs()?.collect() } else { dev.supported_output_configs()?.collect() };
    for &rate in want {
        if rate == default.sample_rate().0 {
            return Ok(default); // ja esta nessa taxa
        }
        let best = ranges.iter()
            .filter(|r| r.min_sample_rate().0 <= rate && rate <= r.max_sample_rate().0)
            .max_by_key(|r| (r.channels(), r.sample_format() == default.sample_format()));
        if let Some(r) = best {
            return Ok(r.clone().with_sample_rate(cpal::SampleRate(rate)));
        }
    }
    let mut rates: Vec<u32> = [44_100, 48_000, 88_200, 96_000].into_iter()
        .filter(|&x| ranges.iter().any(|r| r.min_sample_rate().0 <= x && x <= r.max_sample_rate().0))
        .collect();
    rates.dedup();
    let list = rates.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(", ");
    Err(format!(
        "A placa nao aceita {} Hz na {}. Taxas aceitas: {}. Escolha outra em Configuracao > Taxa de amostragem da placa.",
        want.first().copied().unwrap_or(0), if input { "entrada" } else { "saida" }, if list.is_empty() { "desconhecidas".into() } else { list }
    ).into())
}

fn input_stream<T>(dev: &cpal::Device, cfg: &cpal::StreamConfig, mut f: impl FnMut(&[f32]) + Send + 'static) -> Res<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let mut buf: Vec<f32> = Vec::new();
    Ok(dev.build_input_stream(
        cfg,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            buf.clear();
            buf.extend(data.iter().map(|s| f32::from_sample_(*s)));
            f(&buf);
        },
        |e| eprintln!("erro na entrada de audio: {e}"),
        None,
    )?)
}

fn output_stream<T>(dev: &cpal::Device, cfg: &cpal::StreamConfig, mut f: impl FnMut(&mut [f32]) + Send + 'static) -> Res<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let mut buf: Vec<f32> = Vec::new();
    Ok(dev.build_output_stream(
        cfg,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            buf.resize(data.len(), 0.0);
            f(&mut buf);
            for (d, s) in data.iter_mut().zip(&buf) {
                *d = T::from_sample_(*s);
            }
        },
        |e| eprintln!("erro na saida de audio: {e}"),
        None,
    )?)
}

struct Engine {
    _input: cpal::Stream,
    _output: cpal::Stream,
    network: Network,
}

fn start_engine(shared: &Arc<Shared>, host: &cpal::Host) -> Res<Engine> {
    let cfg = shared.config.lock().unwrap().clone();
    if cfg.device.is_empty() {
        return Err("".into()); // primeira execucao: o painel pede a configuracao
    }
    if let Some(own) = OWN_DRIVERS.iter().find(|d| d.eq_ignore_ascii_case(cfg.device.trim())) {
        return Err(format!("\"{own}\" e o driver virtual deste mesmo programa e nao pode ser usado como placa aqui. Escolha a placa de som de verdade em Configuracao, ou feche este programa e use o driver direto no REAPER.").into());
    }
    let iface = resolve_iface(&cfg)?;

    let missing = |what: &str| format!("A placa \"{}\" nao foi encontrada ({what}). Confira se esta ligada e escolha de novo em Configuracao.", cfg.device);
    let in_dev = find_device(host, &cfg.device, true).ok_or_else(|| missing("entrada"))?;
    let out_dev = find_device(host, &cfg.device, false).ok_or_else(|| missing("saida"))?;
    // Taxa da placa: a escolhida em Configuracao; no automatico, 48000 Hz se a placa aceitar
    // e, senao, a taxa em que ela ja esta. A rede e sempre 48000 Hz: quando a placa fica em
    // outra taxa, as filas convertem.
    let current = in_dev.default_input_config()?.sample_rate().0;
    let want: Vec<u32> = if cfg.rate != 0 { vec![cfg.rate] } else { vec![RATE, current] };
    let in_def = pick_config(&in_dev, true, &want)?;
    let dev_rate = in_def.sample_rate().0;
    let out_def = pick_config(&out_dev, false, &[dev_rate])?;
    let (dev_ins, dev_outs) = (in_def.channels() as usize, out_def.channels() as usize);
    let (network, capture, playback) = start_network(shared, &cfg, iface, dev_ins, dev_outs, dev_rate, 0)?;

    let (in_cfg, out_cfg) = (in_def.config(), out_def.config());
    use cpal::SampleFormat::{F32, I16, I32};
    let input = match in_def.sample_format() {
        F32 => input_stream::<f32>(&in_dev, &in_cfg, capture)?,
        I32 => input_stream::<i32>(&in_dev, &in_cfg, capture)?,
        I16 => input_stream::<i16>(&in_dev, &in_cfg, capture)?,
        other => return Err(format!("formato de entrada nao suportado: {other}").into()),
    };
    let output = match out_def.sample_format() {
        F32 => output_stream::<f32>(&out_dev, &out_cfg, playback)?,
        I32 => output_stream::<i32>(&out_dev, &out_cfg, playback)?,
        I16 => output_stream::<i16>(&out_dev, &out_cfg, playback)?,
        other => return Err(format!("formato de saida nao suportado: {other}").into()),
    };
    input.play()?;
    output.play()?;

    let mut st = shared.status.lock().unwrap();
    st.running = true;
    st.device = in_dev.name().unwrap_or_default();
    (st.ins, st.outs, st.net_tx, st.rate) = (dev_ins, dev_outs, network.net_tx, dev_rate);
    drop(st);
    Ok(Engine { _input: input, _output: output, network })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let no_browser = args.iter().any(|a| a == "--no-browser");
    let port: u16 = args.iter().position(|a| a == "--port").and_then(|i| args.get(i + 1)?.parse().ok()).unwrap_or(8787);
    let url = format!("http://127.0.0.1:{port}");

    // Depois de um reinicio a porta pode levar um instante para liberar.
    let listener = (0..25).find_map(|_| {
        TcpListener::bind(("127.0.0.1", port)).ok().or_else(|| {
            std::thread::sleep(Duration::from_millis(200));
            None
        })
    });
    let Some(listener) = listener else {
        // Ja esta aberto: so mostra o painel.
        open_browser(&url);
        return;
    };

    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default();
    let shared = Arc::new(Shared::load(std::path::Path::new(&home).join(".aoip-bridge.conf")));
    let host = audio_host();
    *shared.devices.lock().unwrap() = list_devices(&host);
    if let Some(ip) = aoip_engine::net::guess_iface() {
        shared.status.lock().unwrap().iface = ip.to_string();
    }
    ui::serve(shared.clone(), listener);
    println!("painel: {url}");
    if selftest() {
        report(&format!("AUTOTESTE ({}): tom de teste e relogio local; nada sai para a rede.", std::env::consts::OS));
    }
    if !no_browser {
        open_browser(&url);
    }

    let engine = match start_engine(&shared, &host) {
        Ok(e) => Some(e),
        Err(e) => {
            shared.status.lock().unwrap().error = e.to_string();
            None
        }
    };

    let mut ticks = 0u32;
    loop {
        std::thread::sleep(Duration::from_millis(250));
        ticks += 1;
        if selftest() && ticks % 20 == 0 {
            match &engine {
                Some(e) => report_stats(&shared, &e.network),
                None => {
                    let err = shared.status.lock().unwrap().error.clone();
                    report(&format!("autoteste: motor parado. {}", if err.is_empty() { "Escolha a placa no painel e clique em Salvar e iniciar." } else { err.as_str() }));
                }
            }
        }
        if shared.quit.load(Relaxed) {
            std::thread::sleep(Duration::from_millis(300)); // deixa a resposta HTTP sair
            return;
        }
        if shared.restart.load(Relaxed) {
            std::thread::sleep(Duration::from_millis(300));
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).args(["--no-browser", "--port", &port.to_string()]).spawn();
            }
            return;
        }
        if let Some(e) = &engine {
            refresh_status(&shared, &e.network);
        } else if shared.devices.lock().unwrap().is_empty() {
            *shared.devices.lock().unwrap() = list_devices(&host);
        }
    }
}

