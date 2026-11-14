//! Atualizacao pelo GitHub: procura versoes novas (ao abrir e a cada 6 horas), e, quando o
//! usuario clica em Atualizar, baixa o pacote deste sistema, confere a impressao digital
//! publicada pelo GitHub e abre o instalador de sempre:
//!   - Windows: Instalar.bat, como administrador (o Windows pergunta);
//!   - Mac: Instalar.command, no Terminal (onde a pessoa digita a senha).
//! Para baixar usa o `curl` do proprio sistema (Windows 10/11 e Mac ja tem): nada de
//! bibliotecas a mais. O pedido ao GitHub leva so o nome e a versao do programa.
use aoip_core::state::Shared;
use aoip_core::update::{pick_release, sha256_hex, Available, UpdateInfo, RELEASES_URL};
use aoip_core::VERSION;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Qual programa esta se atualizando (para reabrir o certo depois de instalar).
#[derive(Clone, Copy, PartialEq)]
pub enum Program {
    /// aoip-bridge (placa de verdade).
    Bridge,
    /// Placa virtual (AoIP Virtual no Windows; o auxiliar no Mac).
    Virtual,
}

const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);

/// Versao atual (para teste, AOIP_UPDATE_AS finge outra: assim da para ver o aviso).
fn current() -> String {
    std::env::var("AOIP_UPDATE_AS").unwrap_or_else(|_| VERSION.to_string())
}

fn asset_prefix() -> &'static str {
    if cfg!(windows) { "AoIP-Bridge-Windows-" } else { "AoIP-Bridge-Mac-" }
}

fn curl() -> Command {
    let mut c = Command::new(if cfg!(windows) { "curl.exe" } else { "/usr/bin/curl" });
    c.args(["-fsSL", "--retry", "2", "-A", &format!("AoIP-Bridge/{VERSION}")]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: sem janela preta piscando
    }
    c
}

fn set(shared: &Shared, f: impl FnOnce(&mut UpdateInfo)) {
    f(&mut shared.update.lock().unwrap());
}

/// Procura uma versao nova. Devolve a versao encontrada (ou None, ja na mais nova).
pub fn check(shared: &Shared) -> Result<Option<Available>, String> {
    set(shared, |u| u.state = "checking".into());
    let out = curl().args(["--max-time", "20", "-H", "Accept: application/vnd.github+json", RELEASES_URL]).output()
        .map_err(|e| format!("nao consegui procurar atualizacoes ({e})"))?;
    if !out.status.success() {
        return Err("sem resposta do GitHub (sem internet?)".into());
    }
    let found = pick_release(&String::from_utf8_lossy(&out.stdout), &current(), asset_prefix())?;
    set(shared, |u| {
        *u = match &found {
            Some(a) => UpdateInfo { state: "available".into(), version: a.version.clone(), page: a.page.clone(), message: String::new() },
            None => UpdateInfo { state: "uptodate".into(), ..Default::default() },
        }
    });
    Ok(found)
}

fn run(cmd: &mut Command, what: &str) -> Result<(), String> {
    let s = cmd.status().map_err(|e| format!("{what}: {e}"))?;
    if s.success() { Ok(()) } else { Err(format!("{what} falhou")) }
}

/// Baixa o pacote numa pasta temporaria, confere a impressao digital e abre o zip.
/// Devolve a pasta.
pub fn download(a: &Available) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("aoip-atualizacao-{}", a.version));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("pasta temporaria: {e}"))?;
    let zip = dir.join(&a.asset_name);
    run(curl().args(["--max-time", "600", "-o"]).arg(&zip).arg(&a.url), "o download")?;
    let data = std::fs::read(&zip).map_err(|e| format!("arquivo baixado: {e}"))?;
    if sha256_hex(&data) != a.sha256 {
        let _ = std::fs::remove_dir_all(&dir);
        return Err("o arquivo baixado nao confere com o publicado no GitHub (nada foi instalado)".into());
    }
    extract(&zip, &dir)?;
    Ok(dir)
}

/// Baixa, confere e abre o instalador.
pub fn install(shared: &Shared, a: &Available, program: Program) -> Result<(), String> {
    set(shared, |u| (u.state, u.message) = ("downloading".into(), format!("Baixando {}...", a.asset_name)));
    let dir = download(a)?;
    set(shared, |u| u.message = "Abrindo o instalador...".into());
    open_installer(&dir, program)?;
    set(shared, |u| {
        u.state = "installing".into();
        u.message = if cfg!(windows) {
            "Instalando: aceite o pedido de administrador do Windows. O AoIP fecha e abre de novo sozinho.".into()
        } else {
            "Instalando: siga no Terminal (o Mac pede a sua senha). A placa virtual volta sozinha; o AoIP Bridge, abra de novo depois.".into()
        };
    });
    Ok(())
}

fn extract(zip: &Path, dir: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // tar.exe do Windows 10/11 entende .zip
        return run(Command::new("tar.exe").arg("-xf").arg(zip).arg("-C").arg(dir).creation_flags(0x0800_0000), "abrir o pacote");
    }
    #[cfg(not(windows))]
    run(Command::new("/usr/bin/ditto").args(["-x", "-k"]).arg(zip).arg(dir), "abrir o pacote")
}

fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find(&p, name) {
                return Some(f);
            }
        } else if p.file_name().is_some_and(|n| n == name) {
            return Some(p);
        }
    }
    None
}

fn open_installer(dir: &Path, program: Program) -> Result<(), String> {
    #[cfg(windows)]
    {
        let bat = find(dir, "Instalar.bat").ok_or("o pacote nao tem o Instalar.bat")?;
        let args = if program == Program::Bridge { "'--auto','--abrir-bridge'" } else { "'--auto'" };
        let ps = format!("Start-Process -FilePath '{}' -ArgumentList {args} -Verb RunAs", bat.display().to_string().replace('\'', "''"));
        return run(Command::new("powershell.exe").args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps]), "pedir permissao de administrador")
            .map_err(|_| "o Windows nao deu permissao de administrador (atualizacao cancelada)".to_string());
    }
    #[cfg(not(windows))]
    {
        let _ = program;
        let script = find(dir, "Instalar.command").ok_or("o pacote nao tem o Instalar.command")?;
        let _ = Command::new("/bin/chmod").arg("+x").arg(&script).status();
        run(Command::new("/usr/bin/open").args(["-a", "Terminal"]).arg(&script), "abrir o Terminal")
    }
}

/// Procura atualizacoes em segundo plano (20 s depois de abrir e a cada 6 h, ou quando o
/// painel pede) e instala quando o painel ou o icone pedem.
pub fn spawn(shared: Arc<Shared>, program: Program) {
    std::thread::spawn(move || {
        let start = Instant::now();
        let (mut last, mut found): (Option<Instant>, Option<Available>) = (None, None);
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let enabled = shared.config.lock().unwrap().check_updates;
            let asked = shared.update_check.swap(false, Relaxed);
            let due = enabled && start.elapsed() > Duration::from_secs(20) && last.map_or(true, |t| t.elapsed() > CHECK_EVERY);
            if asked || due {
                last = Some(Instant::now());
                match check(&shared) {
                    Ok(f) => found = f,
                    Err(e) => set(&shared, |u| *u = UpdateInfo { state: "error".into(), message: e, ..Default::default() }),
                }
            }
            if shared.update_install.swap(false, Relaxed) {
                let target = match &found {
                    Some(a) => Some(a.clone()),
                    None => check(&shared).ok().flatten(),
                };
                match target {
                    Some(a) => {
                        if let Err(e) = install(&shared, &a, program) {
                            set(&shared, |u| (u.state, u.message) = ("error".into(), e));
                        }
                    }
                    None => set(&shared, |u| (u.state, u.message) = ("uptodate".into(), String::new())),
                }
            }
        }
    });
}
