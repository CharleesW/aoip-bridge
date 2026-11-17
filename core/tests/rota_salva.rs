// Reproduz: notebook recebia da placa virtual do desktop; o Controller trocou as 32 entradas
// para a mesa ("DESKTOP : 1..4"); o programa salvou, fechou e abriu de novo.
use aoip_core::matrix::Matrix;
use aoip_core::routing::{route_of, set_route};
use aoip_core::state::{Config, NET_RX, RX_SLOTS};

#[test]
fn routes_survive_save_and_reload() {
    let mut cfg = Config::default();
    let mut m = Matrix::new(NET_RX, 32);
    for k in 0..32 {
        set_route(&mut cfg.rx, &mut m, k, Some((&format!("DESKV : {}", k / 8 + 1), k % 8))).unwrap();
    }
    for k in 0..32 {
        set_route(&mut cfg.rx, &mut m, k, Some((&format!("DESK : {}", k / 8 + 1), k % 8))).unwrap();
    }
    cfg.rx_map = Some(m.to_spec());
    let before: Vec<_> = (0..32).map(|o| route_of(&cfg.rx, &m, o)).collect();
    println!("slots antes: {:?}", cfg.rx);
    let back = Config::parse(&cfg.to_text());
    println!("slots depois: {:?}", back.rx);
    let m2 = Matrix::from_saved(back.rx_map.as_deref(), NET_RX, 32);
    let after: Vec<_> = (0..32).map(|o| route_of(&back.rx, &m2, o)).collect();
    assert_eq!(back.rx.len(), RX_SLOTS);
    assert_eq!(after, before);
    assert_eq!(after[0], Some(("DESK : 1".into(), 0)));
}
