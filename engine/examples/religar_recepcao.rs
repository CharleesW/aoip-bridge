//! Mede quanto tempo a recepcao fica muda quando a configuracao e salva (a rede religa).
//! Uma "MESA" fica enviando o tempo todo (como outra maquina) e se anuncia a cada 10 s; a
//! placa virtual recebe dela. Depois de estabilizar, so a placa virtual religa, e contamos
//! quantos milissegundos passam ate ela voltar a receber. AOIP_SELFTEST=2: nada sai da maquina.
//!   AOIP_SELFTEST=2 cargo run -p aoip-engine --example religar_recepcao
use aoip_core::state::{Config, Shared};
use aoip_engine::virtual_card::VirtualCard;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn active(s: &Shared) -> bool {
    s.slot_active.lock().unwrap().first().copied().unwrap_or(false)
}

fn main() {
    // a "mesa": outro motor, que nao religa
    let mut mesa_cfg = Config::default();
    mesa_cfg.name = "MESA".into();
    let mesa = Arc::new(Shared::new(mesa_cfg.clone(), std::env::temp_dir().join("aoip-religar-mesa.conf")));
    let iface = aoip_engine::resolve_iface(&mesa, &mesa_cfg).unwrap();
    let (_net, mut cap, mut play) = aoip_engine::start_network(&mesa, &mesa_cfg, iface, 8, 8, 48_000, 0).unwrap();
    std::thread::spawn(move || {
        let mut b = vec![0.1f32; 48 * 8];
        loop {
            cap(&b);
            play(&mut b);
            b.fill(0.1);
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    let path = std::env::temp_dir().join("aoip-religar-recepcao.conf");
    let _ = std::fs::remove_file(&path);
    let mut cfg = Config::default();
    cfg.card_channels = 8;
    cfg.name = "Medida".into();
    cfg.rx[0] = "MESA : 1".into();
    let shared = Arc::new(Shared::new(cfg, path));
    let card = VirtualCard::new(shared.clone());
    card.start();
    let t = Instant::now();
    while !active(&shared) && t.elapsed() < Duration::from_secs(40) {
        std::thread::sleep(Duration::from_millis(20));
    }
    println!("primeira vez: recebeu apos {} ms", t.elapsed().as_millis());
    for round in 1..=3 {
        std::thread::sleep(Duration::from_secs(2));
        let t = Instant::now();
        card.start();
        let religou = t.elapsed().as_millis();
        while !active(&shared) && t.elapsed() < Duration::from_secs(40) {
            std::thread::sleep(Duration::from_millis(20));
        }
        println!("religada {round}: rede no ar em {religou} ms, recepcao de volta em {} ms", t.elapsed().as_millis());
    }
}
