//! A placa virtual sem programa de audio para de mandar pacotes e volta quando o programa abre?
//! Uma placa envia; outra (em outro programa, "recebe") mede quanto o som leva para voltar.
//!   AOIP_SELFTEST=2 cargo run -p aoip-engine --example pausa
use aoip_core::state::{Config, Shared};
use aoip_engine::local::CardHost;
use aoip_engine::virtual_card::VirtualCard;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::{Duration, Instant};

const CH: usize = 8;

fn card(name: &str, rx: &str) -> Arc<VirtualCard> {
    let path = std::env::temp_dir().join(format!("aoip-pausa-{name}.conf"));
    let _ = std::fs::remove_file(&path);
    let mut cfg = Config::default();
    (cfg.card_channels, cfg.name) = (CH, name.into());
    cfg.rx[0] = rx.into();
    let c = VirtualCard::new(Arc::new(Shared::new(cfg, path)));
    c.start();
    c
}

/// Toca (ou fica calada) e devolve o nivel que chegou na entrada 1, por `secs`.
fn run(c: &VirtualCard, secs: f32, phase: &mut f32) -> Option<f32> {
    let start = Instant::now();
    let (mut first, mut n) = (None, 0u64);
    let mut out = vec![0f32; 48 * CH];
    let mut inp = vec![0f32; 48 * CH];
    while start.elapsed().as_secs_f32() < secs {
        for f in out.chunks_exact_mut(CH) {
            f[0] = 0.3 * phase.sin();
            *phase = (*phase + std::f32::consts::TAU * 440.0 / 48_000.0) % std::f32::consts::TAU;
        }
        c.process(&out, &mut inp, CH);
        if first.is_none() && inp.chunks_exact(CH).any(|f| f[0].abs() > 0.1) {
            first = Some(start.elapsed().as_secs_f32() * 1000.0);
        }
        n += 1;
        let due = start + Duration::from_millis(n);
        std::thread::sleep(due.saturating_duration_since(Instant::now()));
    }
    first
}

fn main() {
    let c = card("Pausa", "Pausa : 1");
    let pk = || c.shared.tx_diag.packets.load(Relaxed);
    let mut ph = 0f32;
    let measure = |what: &str| {
        let a = pk();
        std::thread::sleep(Duration::from_secs(2));
        println!("{what}: {} pacotes em 2 s", pk() - a);
        pk() - a
    };
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(measure("sem programa"), 0);
    run(&c, 14.0, &mut ph); // inclui achar o anuncio
    let d = &c.shared.rx_diag[0];
    println!("tocando: recebidos {}", d.packets.load(Relaxed));
    assert!(d.packets.load(Relaxed) > 2000);
    let (lost, jumps, txj) = (d.lost.load(Relaxed), d.jumps.load(Relaxed), c.shared.tx_diag.jumps.load(Relaxed));
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(measure("programa fechado"), 0);
    std::thread::sleep(Duration::from_secs(4));
    let back = run(&c, 4.0, &mut ph).expect("o som voltou");
    println!("programa reaberto: som de volta em {back:.0} ms (o primeiro bloco pode ser resto da fila)");
    println!("perdidos {} saltos na chegada {} no envio {}", d.lost.load(Relaxed) - lost, d.jumps.load(Relaxed) - jumps, c.shared.tx_diag.jumps.load(Relaxed) - txj);
    assert_eq!((d.lost.load(Relaxed) - lost, d.jumps.load(Relaxed) - jumps, c.shared.tx_diag.jumps.load(Relaxed) - txj), (0, 0, 0));
    println!("OK");
}
