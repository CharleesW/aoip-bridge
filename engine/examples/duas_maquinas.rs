//! Duas "maquinas" no mesmo computador (so para testar o Controller): "DESK" com 32 canais
//! e "MIX" com 16, cada uma com painel proprio. Rodar com AOIP_SELFTEST=2 (nada sai para a
//! rede): DESK em http://127.0.0.1:8801 e MIX em http://127.0.0.1:8802.
use aoip_core::state::{Config, Shared};
use std::sync::Arc;
use std::time::Duration;

fn machine(name: &str, ch: usize, panel: u16, base: u32) -> Arc<Shared> {
    let path = std::env::temp_dir().join(format!("aoip-teste-{name}.conf"));
    let _ = std::fs::remove_file(&path);
    let mut cfg = Config::default();
    cfg.name = name.into();
    cfg.rx_map = Some(String::new()); // comeca sem nada ligado
    let shared = Arc::new(Shared::new(cfg.clone(), path));
    let iface = aoip_engine::resolve_iface(&shared, &cfg).unwrap();
    let (net, mut cap, mut play) = aoip_engine::start_network(&shared, &cfg, iface, ch, ch, 48_000, base).unwrap();
    {
        let mut st = shared.status.lock().unwrap();
        (st.running, st.ins, st.outs, st.net_tx, st.rate, st.device) = (true, ch, ch, net.net_tx, 48_000, format!("placa de teste {name}"));
    }
    aoip_core::ui::serve(shared.clone(), std::net::TcpListener::bind(("127.0.0.1", panel)).unwrap());
    let sh = shared.clone();
    std::thread::spawn(move || {
        // "placa": canal k toca um tom de (k+1)*100 Hz; mede o que chega nas saidas
        let mut buf = vec![0f32; 48 * ch];
        let mut out = vec![0f32; 48 * ch];
        let mut t = 0u64;
        loop {
            for f in 0..48 {
                for k in 0..ch {
                    buf[f * ch + k] = 0.1 * ((t + f as u64) as f32 * std::f32::consts::TAU * (k as f32 + 1.0) * 100.0 / 48_000.0).sin();
                }
            }
            t += 48;
            cap(&buf);
            play(&mut out);
            aoip_core::state::note_peaks(&sh.in_peak, &out, ch); // usa os medidores de entrada para mostrar o que chegou
            aoip_engine::refresh_status(&sh, &net);
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    shared
}

fn main() {
    let _desk = machine("DESK", 32, 8801, 0);
    let _mix = machine("MIX", 16, 8802, 64);
    println!("DESK: http://127.0.0.1:8801   MIX: http://127.0.0.1:8802");
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
