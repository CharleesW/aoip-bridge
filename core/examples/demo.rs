//! Painel com dados falsos, para ver a interface sem placa nem rede:
//!   cargo run -p aoip-core --example demo   ->  http://127.0.0.1:8787
use aoip_core::matrix::Matrix;
use aoip_core::state::{note_peaks, Config, Flow, Shared};
use std::sync::Arc;

fn main() {
    let empty = std::env::args().any(|a| a == "--empty");
    let mut cfg = Config::default();
    let shared = if empty {
        Arc::new(Shared::new(cfg, std::env::temp_dir().join("aoip-demo.conf")))
    } else {
        cfg.device = "Soundcraft Ui24R".into();
        cfg.iface = "192.168.1.50".into();
        cfg.name = "Ui24R".into();
        cfg.rx[0] = "MESA-FOH : 1".into();
        cfg.rx[1] = "PALCO : 3".into();
        let s = Arc::new(Shared::new(cfg, std::env::temp_dir().join("aoip-demo.conf")));
        s.tx.edit(|m| { *m = Matrix::identity(32, 32); m.set(0, 9, 0.5); m.set(3, 3, 1.6); });
        s.rx.edit(|m| { *m = Matrix::identity(32, 32); m.set(9, 1, 0.25); });
        {
            let mut st = s.status.lock().unwrap();
            st.running = true; st.device = "Soundcraft Ui24R".into(); st.iface = "192.168.1.50".into();
            st.ins = 32; st.outs = 32; st.net_tx = 32; st.ptp = true; st.grandmaster = "001DC1FFFE123456".into();
            st.gm_ip = "192.168.5.30".into(); st.gm_name = "MESA-FOH".into(); st.iface_label = "Ethernet".into();
            st.name = "Ui24R".into(); st.auto_name = "PC-IGREJA".into();
        }
        *s.flows.lock().unwrap() = vec![
            Flow { name: "MESA-FOH : 1".into(), addr: "239.69.83.67:5004".into(), channels: 8, origin: "192.168.5.30".into(), compatible: true },
            Flow { name: "PALCO : 3".into(), addr: "239.69.12.9:5004".into(), channels: 2, origin: "192.168.5.31".into(), compatible: true },
            Flow { name: "RADIO 96k".into(), addr: "239.69.1.1:5004".into(), channels: 2, origin: "192.168.5.32".into(), compatible: false },
        ];
        *s.slot_active.lock().unwrap() = vec![true, false, false, false];
        s
    };
    *shared.ifaces.lock().unwrap() = vec![
        aoip_core::state::NetIface { id: "Ethernet".into(), label: "Ethernet (Realtek PCIe GbE)".into(), ip: "192.168.1.50".parse().unwrap() },
        aoip_core::state::NetIface { id: "Wi-Fi".into(), label: "Wi-Fi (Intel AX201)".into(), ip: "192.168.10.20".parse().unwrap() },
    ];
    *shared.devices.lock().unwrap() = vec![("Soundcraft Ui24R".into(), 32, 32), ("MacBook Pro Microphone".into(), 1, 0)];
    let l = std::net::TcpListener::bind("127.0.0.1:8787").unwrap();
    aoip_core::ui::serve(shared.clone(), l);
    println!("http://127.0.0.1:8787");
    let mut t = 0f32;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        t += 0.3;
        let lv: Vec<f32> = (0..32).map(|c| if c < 10 { (0.5 + 0.5 * (t + c as f32).sin()) * 0.8 / (1 + c) as f32 * 3.0 } else { 0.0 }).collect();
        if !empty { note_peaks(&shared.in_peak, &lv, 32); note_peaks(&shared.rx_peak, &lv[..32], 32); }
    }
}
