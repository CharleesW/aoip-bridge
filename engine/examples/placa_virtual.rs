//! Teste de ponta a ponta da placa virtual sempre ligada, sem Windows: um "driver" de
//! mentira toca um tom no canal 1, o tom vai para a rede (so neste computador, com
//! AOIP_SELFTEST=2), volta pela recepcao e deve aparecer nas entradas do "programa de audio".
//!   AOIP_SELFTEST=2 cargo run -p aoip-engine --example placa_virtual
use aoip_core::state::{Config, Shared};
use aoip_engine::local::{self, CardClient};
use aoip_engine::virtual_card::VirtualCard;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let path = std::env::temp_dir().join("aoip-placa-virtual-teste.conf");
    let _ = std::fs::remove_file(&path);
    let mut cfg = Config::default();
    cfg.card_channels = 16;
    cfg.sys_audio = true;
    cfg.name = "Teste".into();
    cfg.rx[0] = "Teste : 1".into(); // recebe o proprio fluxo de volta
    cfg.rx[1] = "Teste - Saida de Audio".into(); // e o som do computador, nas entradas 9-10
    let shared = Arc::new(Shared::new(cfg, path));
    let card = VirtualCard::new(shared.clone());
    card.set_aux_format(Some((2, 44_100))); // "som do computador" em 44,1 kHz
    card.start();
    {
        let c = card.clone();
        std::thread::spawn(move || {
            let mut ph = 0f32;
            let mut buf = vec![0f32; 441 * 2];
            loop {
                for f in 0..441 { let v = 0.3 * ph.sin(); buf[f * 2] = v; buf[f * 2 + 1] = -v; ph = (ph + std::f32::consts::TAU * 440.0 / 44_100.0) % std::f32::consts::TAU; }
                c.feed_aux(&buf);
                std::thread::sleep(Duration::from_millis(10));
            }
        });
    }
    let listener = std::net::TcpListener::bind(("127.0.0.1", local::PORT)).expect("porta 48790 livre");
    local::serve(card.clone(), listener);
    {
        let c = card.clone();
        std::thread::spawn(move || loop { c.tick(); std::thread::sleep(Duration::from_millis(250)); });
    }

    let info = local::query().unwrap();
    println!("formato: {} canais, {} Hz, bloco {}", info.channels, info.rate, info.buffer);
    let mut drv = CardClient::connect().unwrap();
    let (ch, frames) = (info.channels, 128usize);
    let period = Duration::from_nanos(frames as u64 * 1_000_000_000 / info.rate as u64);
    let (mut to_net, mut from_net) = (vec![0f32; frames * ch], vec![0f32; frames * ch]);
    let (mut phase, mut next, start) = (0f32, Instant::now(), Instant::now());
    let (mut peak, mut aux_l, mut aux_r) = (0f32, 0f32, 0f32);
    while start.elapsed() < Duration::from_secs(6) {
        for f in 0..frames {
            to_net[f * ch] = 0.5 * phase.sin();
            phase = (phase + std::f32::consts::TAU * 1000.0 / info.rate as f32) % std::f32::consts::TAU;
        }
        drv.exchange(&to_net, &mut from_net, ch).unwrap();
        if start.elapsed() > Duration::from_secs(4) {
            for f in 0..frames {
                peak = peak.max(from_net[f * ch].abs());
                aux_l = aux_l.max(from_net[f * ch + 8].abs());
                aux_r = aux_r.max(from_net[f * ch + 9].abs());
            }
        }
        next += period;
        if let Some(w) = next.checked_duration_since(Instant::now()) {
            std::thread::sleep(w);
        }
    }
    let st = shared.status.lock().unwrap().clone();
    println!("nome na rede: {} | fluxos vistos: {:?}", st.name, shared.flows.lock().unwrap().iter().map(|f| f.name.clone()).collect::<Vec<_>>());
    println!("pico que voltou no canal 1: {peak:.3} (o tom saiu com 0,5)");
    println!("som do computador nas entradas 9 e 10: {aux_l:.3} / {aux_r:.3} (saiu com 0,3)");
    println!("{}", if aux_l > 0.25 && aux_r > 0.25 { "OK: o fluxo Saida de Audio chegou" } else { "FALHOU: o fluxo Saida de Audio nao chegou" });
    println!("{}", if peak > 0.4 { "OK: o audio fez a volta inteira" } else { "FALHOU: o audio nao voltou" });
}
