//! Programa auxiliar da placa virtual "AoIP Bridge" do macOS.
//!
//! O macOS roda os drivers de audio num processo restrito e de prioridade baixa, onde rede e
//! painel ficam lentos. Por isso o driver (`driver.c`) so troca audio com este programa, por
//! UDP dentro do proprio computador (127.0.0.1), e aqui ficam a rede AES67 e o painel
//! (http://127.0.0.1:8788). Tudo e indexado pelo relogio da placa (quadros desde que ela ligou):
//!   driver -> aqui: o que os programas tocaram, com a hora em que deve soar
//!   aqui -> driver: o que chegou da rede, com a hora a que pertence
//! Sem nenhum programa usando a placa, fica parado (nada vai para a rede). Quando a placa para,
//! ou a configuracao muda, este programa termina e o macOS (launchd) o abre de novo.
use aoip_core::state::Shared;
use aoip_core::{ui, RATE};
use aoip_engine::{realtime_thread_for, refresh_ifaces, refresh_status, report_stats, resolve_iface, selftest, start_network, Capture, Network, Playback};
use std::net::{TcpListener, UdpSocket};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::{Duration, Instant};

const CHANNELS: usize = 32;
const DRIVER_NAME: &str = "AoIP Bridge";
const HELPER_PORT: u16 = 48788; // aqui
const DRIVER_PORT: u16 = 48789; // o driver
const MAGIC: u32 = 0x414F_4950; // "AOIP"
const HEADER: usize = 40;
const BLOCK: usize = 48; // quadros por passo (1 ms)
const MARGIN: i64 = 192; // folga de 4 ms em cada sentido entre o driver e este programa
const RING: usize = 8192; // quadros guardados do que os programas tocaram
const KIND_OUT: u32 = 1; // audio dos programas
const KIND_IN: u32 = 3; // audio da rede

extern "C" {
    fn mach_absolute_time() -> u64;
}

struct Packet {
    epoch: u32,
    anchor: u64,
    ticks_per_frame: f64,
    sample_time: i64,
    frames: usize,
    kind: u32,
}

fn parse(b: &[u8]) -> Option<Packet> {
    if b.len() < HEADER || u32::from_ne_bytes(b[0..4].try_into().ok()?) != MAGIC {
        return None;
    }
    let p = Packet {
        epoch: u32::from_ne_bytes(b[4..8].try_into().ok()?),
        anchor: u64::from_ne_bytes(b[8..16].try_into().ok()?),
        ticks_per_frame: f64::from_ne_bytes(b[16..24].try_into().ok()?),
        sample_time: i64::from_ne_bytes(b[24..32].try_into().ok()?),
        frames: u32::from_ne_bytes(b[32..36].try_into().ok()?) as usize,
        kind: u32::from_ne_bytes(b[36..40].try_into().ok()?),
    };
    (p.ticks_per_frame > 0.0 && b.len() >= HEADER + p.frames * CHANNELS * 4).then_some(p)
}

fn slot(t: i64) -> usize {
    t.rem_euclid(RING as i64) as usize
}

fn main() {
    let home = std::env::var_os("HOME").unwrap_or_default();
    let path = std::path::Path::new(&home).join(".aoip-coreaudio.conf");
    let first_run = !path.exists();
    let shared = Arc::new(Shared::load(path));
    {
        let mut c = shared.config.lock().unwrap();
        c.device = DRIVER_NAME.into();
        c.rate = RATE;
        if first_run {
            c.tx_channels = 0; // todos os 32 canais, como uma placa 32x32
        }
    }
    *shared.devices.lock().unwrap() = vec![(DRIVER_NAME.to_string(), CHANNELS, CHANNELS)];
    refresh_ifaces(&shared);
    shared.status.lock().unwrap().auto_name = aoip_core::state::auto_name(&aoip_engine::net::computer_name(), " Virtual");
    let panel: u16 = std::env::var("AOIP_PANEL_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8788);

    // Uma copia so: quem chegar depois encontra a porta ocupada e sai.
    let Ok(sock) = UdpSocket::bind(("127.0.0.1", HELPER_PORT)) else { return };
    match TcpListener::bind(("127.0.0.1", panel)) {
        Ok(listener) => ui::serve(shared.clone(), listener),
        Err(e) => eprintln!("painel indisponivel na porta {panel}: {e}"),
    }
    shared.status.lock().unwrap().error =
        "A placa AoIP Bridge esta parada. Abra um programa de audio e escolha a placa \"AoIP Bridge\" para ligar a rede.".into();

    // Parado ate o driver dar sinal de vida (algum programa abriu a placa).
    let mut buf = vec![0u8; HEADER + 64 * CHANNELS * 4];
    sock.set_read_timeout(Some(Duration::from_millis(250))).ok();
    let first = loop {
        if shared.restart.load(Relaxed) {
            std::thread::sleep(Duration::from_millis(400));
            return; // configuracao salva com a placa parada: recomeca com ela
        }
        shared.quit.store(false, Relaxed);
        if let Ok((n, _)) = sock.recv_from(&mut buf) {
            if let Some(p) = parse(&buf[..n]) {
                break p;
            }
        }
    };

    shared.status.lock().unwrap().error.clear();
    let cfg = shared.config.lock().unwrap().clone();
    let started = resolve_iface(&shared, &cfg).and_then(|iface| start_network(&shared, &cfg, iface, CHANNELS, CHANNELS, RATE, 64));
    let mut engine: Option<(Network, Capture, Playback)> = match started {
        Ok(x) => {
            let mut st = shared.status.lock().unwrap();
            st.running = true;
            st.device = DRIVER_NAME.into();
            (st.ins, st.outs, st.net_tx, st.rate) = (CHANNELS, CHANNELS, x.0.net_tx, RATE);
            Some(x)
        }
        Err(e) => {
            shared.status.lock().unwrap().error = e.to_string();
            None
        }
    };

    sock.set_nonblocking(true).ok();
    realtime_thread_for(Duration::from_millis(1), Duration::from_micros(300));
    let driver = ("127.0.0.1", DRIVER_PORT);
    let mut ring = vec![0f32; RING * CHANNELS];
    let (mut epoch, mut anchor, mut tpf) = (first.epoch.wrapping_add(1), first.anchor, first.ticks_per_frame); // forca "placa ligou"
    let mut next = 0i64; // proximo quadro (no relogio da placa) a mandar para a rede
    let mut block = vec![0f32; BLOCK * CHANNELS];
    let mut from_net = vec![0f32; BLOCK * CHANNELS];
    let mut out = vec![0u8; HEADER + BLOCK * CHANNELS * 4];
    let (mut last_packet, mut last_status, mut ticks) = (Instant::now(), Instant::now(), 0u32);
    let mut pending = Some(first);
    let mut pending_len = 0usize;
    loop {
        // 1. o que o driver mandou
        loop {
            let (p, n) = match pending.take() {
                Some(p) => (p, pending_len),
                None => match sock.recv_from(&mut buf) {
                    Ok((n, _)) => match parse(&buf[..n]) {
                        Some(p) => (p, n),
                        None => continue,
                    },
                    Err(_) => break,
                },
            };
            let _ = n;
            last_packet = Instant::now();
            if p.epoch != epoch {
                // a placa ligou (de novo): relogio novo, comeca do "agora"
                (epoch, anchor, tpf) = (p.epoch, p.anchor, p.ticks_per_frame);
                ring.fill(0.0);
                let now = unsafe { mach_absolute_time() }.saturating_sub(anchor) as f64 / tpf;
                next = now as i64 - MARGIN;
            }
            if p.kind == KIND_OUT && p.frames > 0 {
                let data = &buf[HEADER..HEADER + p.frames * CHANNELS * 4];
                for f in 0..p.frames {
                    let t = p.sample_time + f as i64;
                    if t < next || t >= next + RING as i64 {
                        continue; // ja passou, ou longe demais no futuro
                    }
                    let (dst, src) = (slot(t) * CHANNELS, f * CHANNELS * 4);
                    for c in 0..CHANNELS {
                        let b = &data[src + c * 4..src + c * 4 + 4];
                        ring[dst + c] = f32::from_ne_bytes([b[0], b[1], b[2], b[3]]);
                    }
                }
            }
        }
        pending_len = 0;

        // 2. anda ate o "agora" da placa, um milissegundo de audio por vez
        let now = (unsafe { mach_absolute_time() }.saturating_sub(anchor) as f64 / tpf) as i64;
        if now - MARGIN - next > 4800 {
            next = now - MARGIN; // ficou mais de 100 ms para tras: pula
        }
        while next + BLOCK as i64 <= now - MARGIN {
            for f in 0..BLOCK {
                let s = slot(next + f as i64) * CHANNELS;
                block[f * CHANNELS..(f + 1) * CHANNELS].copy_from_slice(&ring[s..s + CHANNELS]);
                ring[s..s + CHANNELS].fill(0.0);
            }
            from_net.fill(0.0);
            if let Some((_, capture, playback)) = engine.as_mut() {
                capture(&block);
                playback(&mut from_net);
            }
            out[0..4].copy_from_slice(&MAGIC.to_ne_bytes());
            out[4..8].copy_from_slice(&epoch.to_ne_bytes());
            out[8..16].copy_from_slice(&anchor.to_ne_bytes());
            out[16..24].copy_from_slice(&tpf.to_ne_bytes());
            out[24..32].copy_from_slice(&(next + MARGIN).to_ne_bytes()); // pertence ao "agora"
            out[32..36].copy_from_slice(&(BLOCK as u32).to_ne_bytes());
            out[36..40].copy_from_slice(&KIND_IN.to_ne_bytes());
            for (i, s) in from_net.iter().enumerate() {
                out[HEADER + i * 4..HEADER + i * 4 + 4].copy_from_slice(&s.to_ne_bytes());
            }
            let _ = sock.send_to(&out, driver);
            next += BLOCK as i64;
        }

        // 3. painel, e a hora de parar
        if last_status.elapsed() > Duration::from_millis(250) {
            last_status = Instant::now();
            ticks += 1;
            if let Some((n, _, _)) = &engine {
                refresh_status(&shared, n);
                if selftest() && ticks % 20 == 0 {
                    report_stats(&shared, n);
                }
            }
            shared.quit.store(false, Relaxed);
            if shared.restart.load(Relaxed) || last_packet.elapsed() > Duration::from_secs(3) {
                std::thread::sleep(Duration::from_millis(400)); // deixa a resposta HTTP sair
                return; // a placa parou ou a configuracao mudou: o launchd abre de novo, parado
            }
        }
        std::thread::sleep(Duration::from_micros(700));
    }
}
