#[path = "../src/icon.rs"]
mod icon;
fn main() {
    let n: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(32);
    let light = std::env::args().nth(2).is_some();
    let p = icon::bgra(n, light);
    std::io::Write::write_all(&mut std::io::stdout(), &p).unwrap();
}
