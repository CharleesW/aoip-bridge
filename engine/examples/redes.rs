fn main() {
    for i in aoip_engine::net::interfaces() { println!("{} | {} | {}", i.id, i.label, i.ip); }
    println!("computador: {}", aoip_engine::net::computer_name());
    let ip: std::net::Ipv4Addr = std::env::args().nth(1).unwrap_or("192.168.5.49".into()).parse().unwrap();
    aoip_engine::net::name_of(ip);
    std::thread::sleep(std::time::Duration::from_secs(4));
    println!("nome de {ip}: {:?}", aoip_engine::net::name_of(ip));
}
