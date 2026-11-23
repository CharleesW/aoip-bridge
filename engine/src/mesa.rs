//! Um programa so para o usuario: o AoIP Virtual (sempre ligado) tambem cuida do programa da
//! mesa (placa de verdade). Ele liga, religa se cair e desliga quando o usuario pede. A mesa
//! roda em outro processo de proposito: se o driver da placa travar, a placa virtual e o que
//! depende dela continuam no ar.
use aoip_core::state::Shared;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const MESA_PORT: u16 = 8787;
/// Painel da placa virtual: e ele que serve a tela unica (`/app`).
pub const APP_PORT: u16 = 8788;

/// O painel da mesa responde nesta porta?
pub fn alive(port: u16) -> bool {
    http(port, "GET", "/api/ping").is_some()
}

fn http(port: u16, method: &str, path: &str) -> Option<String> {
    let mut s = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300)).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(800))).ok()?;
    s.write_all(format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-AoIP: 1\r\nContent-Length: 0\r\n\r\n").as_bytes()).ok()?;
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out.starts_with("HTTP/1.1 200").then_some(out)
}

/// O programa da mesa: no Windows, na mesma pasta deste; no Mac, um aplicativo dentro do
/// "AoIP Bridge.app" (assim a permissao de microfone do macOS fica com ele).
pub fn find_exe() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    if let Some(p) = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.parent()?.join("Helpers/AoIP Mesa.app"))) {
        if p.exists() {
            return Some(p);
        }
    }
    let name = if cfg!(windows) { "aoip-bridge.exe" } else { "aoip-bridge" };
    let p = std::env::current_exe().ok()?.parent()?.join(name);
    p.exists().then_some(p)
}

fn start(exe: &PathBuf, port: u16) -> Option<Child> {
    let mut c = Command::new(exe);
    #[cfg(target_os = "macos")]
    if exe.extension().is_some_and(|e| e == "app") {
        c = Command::new("/usr/bin/open");
        c.args(["-g", "-n"]).arg(exe).arg("--args");
    }
    c.args(["--no-browser", "--port", &port.to_string()]);
    #[cfg(unix)]
    {
        // Grupo proprio: o launchd encerra o grupo do auxiliar quando ele reinicia, e a mesa
        // nao pode cair junto.
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // sem janela preta
    }
    c.spawn().ok()
}

/// Liga o acompanhamento. `exe`: o programa da mesa (None = nao instalado).
pub fn supervise(shared: Arc<Shared>, exe: Option<PathBuf>, port: u16) {
    {
        let mut m = shared.mesa.lock().unwrap();
        m.available = exe.is_some();
        m.wanted = shared.config.lock().unwrap().mesa_on;
    }
    std::thread::spawn(move || {
        let mut child: Option<Child> = None;
        let mut not_before = Instant::now();
        let mut delay = 2u64;
        let mut up_since: Option<Instant> = None;
        let mut close_until: Option<Instant> = None;
        let mut was_wanted = shared.mesa.lock().unwrap().wanted;
        loop {
            let wanted = shared.mesa.lock().unwrap().wanted;
            // So fecha quando o usuario DESLIGA por aqui. Uma mesa aberta a mao, ou por outra
            // versao, nunca e fechada sozinha.
            if was_wanted && !wanted {
                close_until = Some(Instant::now() + Duration::from_secs(15));
            }
            if wanted {
                close_until = None;
            }
            was_wanted = wanted;
            let up = alive(port);
            up_since = if up { up_since.or(Some(Instant::now())) } else { None };
            if up_since.is_some_and(|t| t.elapsed() > Duration::from_secs(60)) {
                delay = 2; // firme ha um minuto: a proxima queda religa depressa
            }
            // Recolhe o processo que terminou (a mesa se reinicia sozinha ao salvar, e outro
            // processo assume a porta).
            if child.as_mut().is_some_and(|c| !matches!(c.try_wait(), Ok(None))) {
                child = None;
                not_before = Instant::now() + Duration::from_secs(delay);
                delay = (delay * 2).min(60); // caiu de novo: espera cada vez mais
            }
            if wanted && !up && child.is_none() && Instant::now() >= not_before {
                if let Some(exe) = &exe {
                    child = start(exe, port);
                    not_before = Instant::now() + Duration::from_secs(8); // tempo de abrir
                }
            }
            if up && close_until.is_some_and(|t| Instant::now() < t) {
                stop(port);
            }
            shared.mesa.lock().unwrap().running = up;
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

/// Pede para a mesa fechar direito (solta a placa e avisa a rede).
pub fn stop(port: u16) {
    http(port, "POST", "/api/quit");
}

/// Aberto a mao (atalho antigo, por exemplo): se a placa virtual esta ligada, entrega a ela o
/// cuidado da mesa e mostra a janela unica. `false` = nao ha placa virtual para isso.
pub fn adopt(port: u16) -> bool {
    if http(port, "GET", "/api/app").is_none_or(|r| !r.contains("\"mesa\"")) {
        return false; // desligada, ou de uma versao antiga
    }
    http(port, "POST", "/api/app/mesa?on=1").is_some()
}

/// A tela unica do programa.
pub fn app_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/app?virtual={port}")
}

/// Abre o programa em janela propria. No Mac, o aplicativo AoIP Bridge. No Windows usa o Edge em modo aplicativo (sem barra de
/// endereco nem abas); sem o Edge, abre no navegador.
pub fn open_window(url: &str) {
    #[cfg(target_os = "macos")]
    if Command::new("/usr/bin/open").args(["-b", "org.aoipbridge.app"]).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success()) {
        return;
    }
    #[cfg(windows)]
    if show_existing() {
        return; // ja esta aberta: so traz para a frente
    }
    #[cfg(windows)]
    for base in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        let Some(dir) = std::env::var_os(base) else { continue };
        let edge = std::path::Path::new(&dir).join("Microsoft\\Edge\\Application\\msedge.exe");
        if edge.exists() {
            let profile = std::env::var_os("LOCALAPPDATA").map(|d| std::path::Path::new(&d).join("AOIP-Bridge-Janela"));
            let mut c = Command::new(edge);
            c.arg(format!("--app={url}")).args(["--window-size=1240,840", "--no-first-run", "--no-default-browser-check"]);
            if let Some(p) = profile {
                c.arg(format!("--user-data-dir={}", p.display()));
            }
            if c.spawn().is_ok() {
                return;
            }
        }
    }
    crate::open_browser(url);
}

/// E a nossa janela? (o titulo e so "AOIP-Bridge 0.7.0"; uma aba de navegador com o painel
/// tem tambem o nome do navegador no titulo)
#[cfg(any(windows, test))]
fn is_our_title(title: &str) -> bool {
    title.strip_prefix("AOIP-Bridge ").is_some_and(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

/// Windows: traz para a frente a janela ja aberta.
#[cfg(windows)]
fn show_existing() -> bool {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe extern "system" fn each(h: HWND, found: LPARAM) -> i32 {
        let (mut title, mut class) = ([0u16; 128], [0u16; 64]);
        // SAFETY: buffers locais, com o tamanho informado; `found` aponta para a variavel abaixo.
        unsafe {
            let n = GetWindowTextW(h, title.as_mut_ptr(), 128).max(0) as usize;
            let c = GetClassNameW(h, class.as_mut_ptr(), 64).max(0) as usize;
            if IsWindowVisible(h) != 0 && String::from_utf16_lossy(&class[..c]).starts_with("Chrome_WidgetWin") && is_our_title(&String::from_utf16_lossy(&title[..n])) {
                *(found as *mut HWND) = h;
                return 0;
            }
        }
        1
    }
    // SAFETY: EnumWindows chama `each` so durante esta chamada.
    unsafe {
        let mut h: HWND = std::mem::zeroed();
        EnumWindows(Some(each), &mut h as *mut HWND as LPARAM);
        if h as isize == 0 {
            return false;
        }
        if IsIconic(h) != 0 {
            ShowWindow(h, SW_RESTORE);
        }
        SetForegroundWindow(h);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aoip_core::state::Config;

    /// Imita a mesa: um servidor que responde a /api/ping e some no /api/quit.
    fn fake(port: u16) -> Arc<Shared> {
        let shared = Arc::new(Shared::new(Config::default(), std::env::temp_dir().join(format!("aoip-mesa-{port}.conf"))));
        let l = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
        aoip_core::ui::serve(shared.clone(), l);
        shared
    }

    #[test]
    fn recognizes_only_our_window() {
        assert!(is_our_title("AOIP-Bridge 0.7.0"));
        assert!(!is_our_title("AOIP-Bridge 0.7.0 - Google Chrome"));
        assert!(!is_our_title("AOIP-Bridge ") && !is_our_title("AOIP-Bridge"));
    }

    #[test]
    fn sees_the_mesa_and_asks_it_to_close() {
        let mesa = fake(18787);
        assert!(alive(18787));
        assert!(!alive(18786), "ninguem nesta porta");
        assert!(!adopt(18786), "sem placa virtual nao ha quem cuide da mesa");
        let card = fake(18785);
        assert!(adopt(18785) && card.mesa.lock().unwrap().wanted, "a placa virtual passa a cuidar da mesa");
        // mesa aberta a mao e nada pedido por aqui: nao se mexe nela
        let off = Arc::new(Shared::new(Config::default(), std::env::temp_dir().join("aoip-mesa-off.conf")));
        supervise(off.clone(), None, 18787);
        std::thread::sleep(Duration::from_millis(2600));
        assert!(off.mesa.lock().unwrap().running && !mesa.quit.load(std::sync::atomic::Ordering::Relaxed), "nao fecha a mesa aberta a mao");
        let me = Arc::new(Shared::new(Config::default(), std::env::temp_dir().join("aoip-mesa-eu.conf")));
        me.mesa.lock().unwrap().wanted = true;
        me.config.lock().unwrap().mesa_on = true;
        supervise(me.clone(), None, 18787);
        std::thread::sleep(Duration::from_millis(500));
        assert!(me.mesa.lock().unwrap().running && !me.mesa.lock().unwrap().available);
        assert!(!mesa.quit.load(std::sync::atomic::Ordering::Relaxed), "ligada e desejada: nao fecha");
        me.mesa.lock().unwrap().wanted = false;
        std::thread::sleep(Duration::from_millis(2600));
        assert!(mesa.quit.load(std::sync::atomic::Ordering::Relaxed), "desligou: pediu para fechar");
    }
}
