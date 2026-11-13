//! Liga a rede, desliga e liga de novo varias vezes (como o driver faz ao salvar a
//! configuracao) e mostra quantas threads o programa tem: nao pode ir crescendo.
//! Rodar com AOIP_SELFTEST=1 (nada sai para a rede).
use aoip_core::state::{Config, Shared};
use std::sync::Arc;
use std::time::Duration;

fn threads() -> usize {
    let pid = std::process::id().to_string();
    let out = std::process::Command::new("ps").args(["-M", "-p", &pid]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).lines().count().saturating_sub(1)
}

fn main() {
    let shared = Arc::new(Shared::new(Config::default(), std::env::temp_dir().join("aoip-religar.conf")));
    shared.config.lock().unwrap().rx[0] = "x".into();
    for round in 1..=2 {
        let cfg = shared.config.lock().unwrap().clone();
        let iface = aoip_engine::resolve_iface(&shared, &cfg).unwrap();
        let (net, mut cap, mut play) = aoip_engine::start_network(&shared, &cfg, iface, 32, 32, 48_000, 64).unwrap();
        let mut buf = vec![0f32; 48 * 32];
        for _ in 0..500 {
            cap(&buf);
            play(&mut buf);
            std::thread::sleep(Duration::from_millis(1));
        }
        let up = threads();
        net.shutdown();
        std::thread::sleep(Duration::from_millis(800));
        println!("rodada {round}: {up} threads com a rede ligada, {} depois de desligar", threads());
    }
}
