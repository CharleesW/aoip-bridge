//! Testa a atualizacao sem instalar nada: finge estar numa versao antiga, procura no GitHub,
//! baixa o pacote deste sistema, confere a impressao digital e abre o zip numa pasta
//! temporaria. O instalador NAO e aberto.
//!   cargo run -p aoip-engine --example atualizacao [versao-fingida]
fn main() {
    let fake = std::env::args().nth(1).unwrap_or("0.4.0".into());
    let out = std::process::Command::new("/usr/bin/curl")
        .args(["-fsSL", "--max-time", "20", "-A", "AoIP-Bridge/teste", aoip_core::update::RELEASES_URL]).output().unwrap();
    let json = String::from_utf8_lossy(&out.stdout);
    let prefix = if cfg!(windows) { "AoIP-Bridge-Windows-" } else { "AoIP-Bridge-Mac-" };
    match aoip_core::update::pick_release(&json, &fake, prefix).unwrap() {
        None => println!("fingindo {fake}: nenhuma versao mais nova"),
        Some(a) => {
            println!("fingindo {fake}: disponivel {} ({}), sha256 {}", a.version, a.asset_name, &a.sha256[..16]);
            let dir = aoip_engine::update::download(&a).unwrap();
            let found: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            println!("baixado e conferido; pasta {}: {:?}", dir.display(), found);
        }
    }
    let real = aoip_core::update::pick_release(&json, aoip_core::VERSION, prefix).unwrap();
    println!("na versao real ({}): {}", aoip_core::VERSION, real.map(|a| format!("disponivel {}", a.version)).unwrap_or("ja e a mais nova".into()));
}
