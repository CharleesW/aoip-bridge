//! AoIP Virtual (Windows): a placa virtual sempre ligada. Inicia com o Windows, fica com o
//! icone perto do relogio, cuida da rede (fluxos, relogio, painel em http://127.0.0.1:8788)
//! e atende o driver ASIO "AoIP Bridge ASIO", que so leva o audio entre o programa de audio
//! e este programa. Pelo icone: canais (2x2 a 64x64), latencia, taxa, capturar o som do
//! Windows (fluxo a parte "NOME - Saida de Audio") e iniciar com o Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod icon;

use aoip_core::state::Shared;
use aoip_core::ui;
use aoip_engine::virtual_card::VirtualCard;
use aoip_engine::{local, open_browser, refresh_ifaces};
use std::net::TcpListener;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Duration;

/// Porta do painel (a variavel AOIP_PANEL_PORT muda, para testes).
fn panel_port() -> u16 {
    std::env::var("AOIP_PANEL_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8788)
}

/// Liga a placa virtual (rede, painel, atendimento ao driver). None = ja havia outra copia
/// aberta (o painel dela e mostrado).
fn start_core() -> Option<Arc<VirtualCard>> {
    // A porta do painel e a trava de "uma copia so". Logo apos sair, pode levar um instante.
    let panel = (0..15).find_map(|_| {
        TcpListener::bind(("127.0.0.1", panel_port())).ok().or_else(|| {
            std::thread::sleep(Duration::from_millis(200));
            None
        })
    });
    let Some(panel) = panel else {
        eprintln!("painel: a porta {} ja esta em uso (outra copia aberta?)", panel_port());
        if !std::env::args().any(|a| a == "--tray") {
            open_browser(&format!("http://127.0.0.1:{}", panel_port()));
        }
        return None;
    };
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
    let path = std::path::Path::new(&home).join(".aoip-asio.conf");
    let first_run = !path.exists();
    let shared = Arc::new(Shared::load(path));
    if first_run {
        shared.config.lock().unwrap().tx_channels = 0; // todos os canais da placa
    }
    shared.status.lock().unwrap().auto_name = aoip_core::state::auto_name(&aoip_engine::net::computer_name(), " Virtual");
    refresh_ifaces(&shared);
    let card = VirtualCard::new(shared.clone());
    aoip_engine::update::spawn(shared.clone(), aoip_engine::update::Program::Virtual);
    ui::serve(shared, panel);
    match TcpListener::bind(("127.0.0.1", local::PORT)) {
        Ok(l) => local::serve(card.clone(), l),
        Err(e) => card.shared.status.lock().unwrap().error = format!("A porta local {} esta ocupada ({e}): o driver ASIO nao vai conseguir falar com este programa.", local::PORT),
    }
    let c = card.clone();
    std::thread::spawn(move || {
        c.start();
        loop {
            std::thread::sleep(Duration::from_millis(250));
            c.tick();
        }
    });
    Some(card)
}

#[cfg(windows)]
fn main() {
    let Some(card) = start_core() else { return };
    win::system_audio(card.clone());
    win::tray(card);
}

#[cfg(not(windows))]
fn main() {
    // Fora do Windows (so para testar): placa virtual sem icone, ate o painel mandar encerrar.
    let Some(card) = start_core() else { return };
    println!("AoIP Virtual sem icone (teste). Painel: http://127.0.0.1:{}", panel_port());
    while !card.shared.quit.load(Relaxed) {
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use aoip_core::state::{BUFFERS, CARD_CHANNELS};
    use std::sync::atomic::AtomicIsize;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Registry::*;
    use windows_sys::Win32::UI::Shell::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const WM_TRAY: u32 = WM_APP + 1;
    const ID_PANEL: usize = 1;
    const ID_CH: usize = 100; // + indice em CARD_CHANNELS
    const ID_BUF: usize = 200; // + indice em BUFFERS
    const ID_44K: usize = 300;
    const ID_48K: usize = 301;
    const ID_SYS: usize = 400;
    const ID_BOOT: usize = 401;
    const ID_UPDATE: usize = 500;
    const ID_QUIT: usize = 900;
    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const RUN_VALUE: &str = "AoIP Virtual";

    static CARD: std::sync::OnceLock<Arc<VirtualCard>> = std::sync::OnceLock::new();
    static ICON: AtomicIsize = AtomicIsize::new(0);
    static TASKBAR_CREATED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn card() -> &'static Arc<VirtualCard> {
        CARD.get().expect("placa iniciada")
    }

    // ------------------------------------------------------------ iniciar com o Windows

    fn exe_path() -> String {
        std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()
    }

    fn starts_with_windows() -> bool {
        unsafe {
            let mut key = 0;
            if RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, KEY_READ, &mut key) != 0 {
                return false;
            }
            let found = RegQueryValueExW(key, wide(RUN_VALUE).as_ptr(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) == 0;
            RegCloseKey(key);
            found
        }
    }

    fn set_start_with_windows(on: bool) {
        unsafe {
            let mut key = 0;
            if RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, KEY_SET_VALUE, &mut key) != 0 {
                return;
            }
            if on {
                let value = wide(&format!("\"{}\" --tray", exe_path()));
                RegSetValueExW(key, wide(RUN_VALUE).as_ptr(), 0, REG_SZ, value.as_ptr() as *const u8, (value.len() * 2) as u32);
            } else {
                RegDeleteValueW(key, wide(RUN_VALUE).as_ptr());
            }
            RegCloseKey(key);
        }
    }

    // ------------------------------------------------------------ som do Windows

    /// Captura o que o Windows esta tocando na saida de som atual ("loopback") e manda para
    /// o fluxo a parte, enquanto "Capturar som do Windows" estiver ligado.
    pub fn system_audio(card: Arc<VirtualCard>) {
        std::thread::spawn(move || {
            use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
            let mut current: Option<(cpal::Stream, String)> = None;
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let want = card.shared.config.lock().unwrap().sys_audio;
                let host = cpal::host_from_id(cpal::HostId::Wasapi).unwrap_or_else(|_| cpal::default_host());
                let device = host.default_output_device();
                let name = device.as_ref().and_then(|d| d.name().ok()).unwrap_or_default();
                let stale = current.as_ref().is_some_and(|(_, n)| *n != name); // trocou a saida de som
                if (!want || stale) && current.is_some() {
                    current = None;
                    card.set_aux_format(None);
                }
                if !want || current.is_some() {
                    continue;
                }
                let Some(device) = device else { continue };
                let Ok(cfg) = device.default_output_config() else { continue };
                let (ch, rate) = (cfg.channels() as usize, cfg.sample_rate().0);
                let c = card.clone();
                let err = |e| eprintln!("captura do som do Windows: {e}");
                let stream = match cfg.sample_format() {
                    cpal::SampleFormat::F32 => device.build_input_stream(&cfg.config(), move |d: &[f32], _: &_| c.feed_aux(d), err, None),
                    cpal::SampleFormat::I16 => {
                        let mut buf = Vec::new();
                        device.build_input_stream(&cfg.config(), move |d: &[i16], _: &_| {
                            buf.clear();
                            buf.extend(d.iter().map(|s| *s as f32 / 32768.0));
                            c.feed_aux(&buf);
                        }, err, None)
                    }
                    _ => continue,
                };
                if let Ok(s) = stream {
                    if s.play().is_ok() {
                        card.set_aux_format(Some((ch, rate)));
                        current = Some((s, name));
                    }
                }
            }
        });
    }

    // ------------------------------------------------------------ icone e menu

    fn tooltip() -> String {
        let c = card();
        let cfg = c.shared.config.lock().unwrap().clone();
        let st = c.shared.status.lock().unwrap().clone();
        let clock = if st.gm_self { "relogio: este PC".to_string() } else if !st.gm_ip.is_empty() { format!("relogio: {}", if st.gm_name.is_empty() { &st.gm_ip } else { &st.gm_name }) } else { "sem relogio".into() };
        let t = format!("AoIP Virtual {}: {}x{}, {} kHz, {}", aoip_core::VERSION, cfg.card_channels, cfg.card_channels, if cfg.rate == 44_100 { "44,1" } else { "48" }, clock);
        t.chars().take(127).collect()
    }

    unsafe fn notify(hwnd: HWND, msg: u32) {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = ICON.load(Relaxed);
        for (d, s) in nid.szTip.iter_mut().zip(wide(&tooltip())) {
            *d = s;
        }
        Shell_NotifyIconW(msg, &nid);
    }

    unsafe fn add_item(menu: HMENU, id: usize, text: &str, checked: bool, enabled: bool) {
        let mut flags = MF_STRING;
        if checked {
            flags |= MF_CHECKED;
        }
        if !enabled {
            flags |= MF_GRAYED;
        }
        AppendMenuW(menu, flags, id, wide(text).as_ptr());
    }

    unsafe fn show_menu(hwnd: HWND) {
        let c = card();
        let cfg = c.shared.config.lock().unwrap().clone();
        let st = c.shared.status.lock().unwrap().clone();
        let menu = CreatePopupMenu();
        add_item(menu, 0, &format!("AoIP Virtual {}  ({})", aoip_core::VERSION, st.name), false, false);
        let clock = if st.gm_self { "Relógio: este computador é o mestre".to_string() }
            else if !st.gm_ip.is_empty() { format!("Relógio: {} ({})", if st.gm_name.is_empty() { "aparelho" } else { &st.gm_name }, st.gm_ip) }
            else if !st.error.is_empty() { "Rede: com problema (abra o painel)".into() }
            else { "Relógio: procurando…".into() };
        add_item(menu, 0, &clock, false, false);
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add_item(menu, ID_PANEL, "Abrir painel", false, true);
        let upd = c.shared.update.lock().unwrap().clone();
        if upd.state == "available" {
            add_item(menu, ID_UPDATE, &format!("Atualizar para a versão {} (feche o REAPER antes)", upd.version), false, true);
        }

        let sub = CreatePopupMenu();
        for (i, n) in CARD_CHANNELS.iter().enumerate() {
            add_item(sub, ID_CH + i, &format!("{n} x {n}"), cfg.card_channels == *n, true);
        }
        AppendMenuW(menu, MF_POPUP, sub as usize, wide(&format!("Canais: {0} x {0}", cfg.card_channels)).as_ptr());

        let sub = CreatePopupMenu();
        let rate = if cfg.rate == 44_100 { 44_100.0 } else { 48_000.0 };
        for (i, b) in BUFFERS.iter().enumerate() {
            add_item(sub, ID_BUF + i, &format!("{b} amostras ({:.1} ms)", *b as f64 * 1000.0 / rate).replace('.', ","), cfg.buffer == *b, true);
        }
        AppendMenuW(menu, MF_POPUP, sub as usize, wide(&format!("Latência: {} amostras", cfg.buffer)).as_ptr());

        let sub = CreatePopupMenu();
        add_item(sub, ID_44K, "44,1 kHz (convertido para os 48 kHz da rede)", cfg.rate == 44_100, true);
        add_item(sub, ID_48K, "48 kHz (igual à rede)", cfg.rate != 44_100, true);
        AppendMenuW(menu, MF_POPUP, sub as usize, wide(&format!("Taxa: {} kHz", if cfg.rate == 44_100 { "44,1" } else { "48" })).as_ptr());

        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add_item(menu, ID_SYS, "Capturar som do Windows (fluxo \"Saída de Áudio\")", cfg.sys_audio, true);
        add_item(menu, ID_BOOT, "Iniciar com o Windows", starts_with_windows(), true);
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add_item(menu, ID_QUIT, "Sair (a placa AoIP para de funcionar)", false, true);

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd); // sem isto o menu nao fecha ao clicar fora
        let id = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, std::ptr::null()) as usize;
        DestroyMenu(menu);
        PostMessageW(hwnd, WM_NULL, 0, 0);
        command(hwnd, id);
    }

    unsafe fn command(hwnd: HWND, id: usize) {
        let c = card();
        match id {
            ID_PANEL => open_browser(&format!("http://127.0.0.1:{}", panel_port())),
            _ if (ID_CH..ID_CH + CARD_CHANNELS.len()).contains(&id) => c.apply(|cfg| cfg.card_channels = CARD_CHANNELS[id - ID_CH]),
            _ if (ID_BUF..ID_BUF + BUFFERS.len()).contains(&id) => c.apply(|cfg| cfg.buffer = BUFFERS[id - ID_BUF]),
            ID_44K => c.apply(|cfg| cfg.rate = 44_100),
            ID_48K => c.apply(|cfg| cfg.rate = 48_000),
            ID_SYS => c.apply(|cfg| cfg.sys_audio = !cfg.sys_audio),
            ID_BOOT => set_start_with_windows(!starts_with_windows()),
            ID_UPDATE => c.shared.update_install.store(true, Relaxed),
            ID_QUIT => quit(hwnd),
            _ => {}
        }
    }

    unsafe fn quit(hwnd: HWND) {
        notify(hwnd, NIM_DELETE);
        std::process::exit(0);
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_TRAY => {
                match lp as u32 {
                    WM_LBUTTONDBLCLK => open_browser(&format!("http://127.0.0.1:{}", panel_port())),
                    WM_RBUTTONUP | WM_LBUTTONUP => show_menu(hwnd),
                    _ => {}
                }
                0
            }
            WM_SETTINGCHANGE => {
                // trocou o tema claro/escuro: redesenha o icone
                ICON.store(make_icon(GetSystemMetrics(SM_CXSMICON).max(16)), Relaxed);
                notify(hwnd, NIM_MODIFY);
                0
            }
            WM_TIMER => {
                notify(hwnd, NIM_MODIFY); // dica com relogio e formato atualizados
                if card().shared.quit.load(Relaxed) {
                    quit(hwnd); // "Encerrar" no painel
                }
                0
            }
            _ if msg == TASKBAR_CREATED.load(Relaxed) && msg != 0 => {
                notify(hwnd, NIM_ADD); // o Explorer reiniciou: poe o icone de volta
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    /// Barra de tarefas no tema claro do Windows.
    fn light_taskbar() -> bool {
        unsafe {
            let mut key = 0;
            let path = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
            if RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_READ, &mut key) != 0 {
                return false;
            }
            let (mut value, mut size) = (0u32, 4u32);
            let ok = RegQueryValueExW(key, wide("SystemUsesLightTheme").as_ptr(), std::ptr::null(), std::ptr::null_mut(), &mut value as *mut u32 as *mut u8, &mut size) == 0;
            RegCloseKey(key);
            ok && value == 1
        }
    }

    unsafe fn make_icon(size: i32) -> HICON {
        let pixels = super::icon::bgra(size as usize, light_taskbar());
        let color = CreateBitmap(size, size, 1, 32, pixels.as_ptr() as *const _);
        let mask_bits = vec![0u8; (size * size / 8) as usize];
        let mask = CreateBitmap(size, size, 1, 1, mask_bits.as_ptr() as *const _);
        let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info);
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }

    pub fn tray(card: Arc<VirtualCard>) {
        let _ = CARD.set(card);
        // Na primeira vez, ja deixa marcado para iniciar com o Windows (desliga pelo menu).
        let marker = std::env::temp_dir().join("aoip-virtual-iniciar.ok");
        if !marker.exists() {
            set_start_with_windows(true);
            let _ = std::fs::write(&marker, b"1");
        }
        unsafe {
            let hinst = GetModuleHandleW(std::ptr::null());
            let class = wide("AoIPVirtualTray");
            let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst, lpszClassName: class.as_ptr(), ..std::mem::zeroed() };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(0, class.as_ptr(), wide("AoIP Virtual").as_ptr(), 0, 0, 0, 0, 0, 0, 0, hinst, std::ptr::null());
            TASKBAR_CREATED.store(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()), Relaxed);
            ICON.store(make_icon(GetSystemMetrics(SM_CXSMICON).max(16)), Relaxed);
            notify(hwnd, NIM_ADD);
            SetTimer(hwnd, 1, 1000, None);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
