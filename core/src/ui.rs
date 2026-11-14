//! Interface: servidor HTTP minimo, so em 127.0.0.1, que entrega o painel e uma API.
use crate::matrix::Matrix;
use crate::state::{take_peaks, Shared, NET_RX, RX_SLOTS};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Duration;

const PAGE: &str = include_str!("ui.html");

/// Nome do sistema como aparece no painel.
fn os_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    }
}

/// O painel com a versao e o sistema preenchidos, para aparecerem em qualquer captura de tela.
pub fn page() -> String {
    PAGE.replace("{{VERSAO}}", crate::VERSION).replace("{{SISTEMA}}", os_name())
}

pub fn serve(shared: Arc<Shared>, listener: TcpListener) {
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let shared = shared.clone();
            std::thread::spawn(move || {
                let _ = handle(conn, &shared);
            });
        }
    });
}

fn handle(mut conn: TcpStream, shared: &Shared) -> std::io::Result<()> {
    conn.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16_384 {
        let n = conn.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buf);
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let (method, target) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
    let header = |name: &str| {
        head.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    };
    // So atende o proprio computador: bloqueia paginas externas que tentem
    // comandar o programa pelo navegador.
    let host = header("host").unwrap_or_default();
    let local = host.starts_with("127.0.0.1") || host.starts_with("localhost");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));

    let (code, ctype, body) = if !local {
        (403, "text/plain", "acesso negado".to_string())
    } else if method == "GET" && path == "/" {
        (200, "text/html; charset=utf-8", page())
    } else if method == "GET" && path == "/api/state" {
        (200, "application/json", state_json(shared))
    } else if method == "POST" && header("x-aoip").is_some() {
        match action(shared, path, query) {
            Ok(()) => (200, "application/json", "{\"ok\":true}".to_string()),
            Err(e) => (400, "application/json", format!("{{\"ok\":false,\"error\":{}}}", json_str(&e))),
        }
    } else {
        (404, "text/plain", "nao encontrado".to_string())
    };
    write!(
        conn,
        "HTTP/1.1 {code} X\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    conn.write_all(body.as_bytes())
}

fn action(shared: &Shared, path: &str, query: &str) -> Result<(), String> {
    let q = |key: &str| {
        query.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            (k == key).then(|| url_decode(v))
        })
    };
    let num = |key: &str| q(key).and_then(|v| v.parse::<usize>().ok()).ok_or(format!("falta {key}"));
    let matrix = |f: &mut dyn FnMut(&mut Matrix) -> Result<(), String>| -> Result<(), String> {
        let which = q("m").unwrap_or_default();
        let cell = match which.as_str() {
            "tx" => &shared.tx,
            "rx" => &shared.rx,
            _ => return Err("matriz invalida".into()),
        };
        let spec = cell.edit(|m| f(m).map(|_| m.to_spec()))?;
        let mut c = shared.config.lock().unwrap();
        if which == "tx" { c.tx_map = Some(spec) } else { c.rx_map = Some(spec) }
        Ok(())
    };
    match path {
        "/api/point" => {
            let (i, o) = (num("i")?, num("o")?);
            let gain = match q("db").as_deref() {
                Some("off") | None => 0.0,
                Some(db) => 10f32.powf(db.parse::<f32>().map_err(|_| "ganho invalido")?.clamp(-60.0, 12.0) / 20.0),
            };
            matrix(&mut |m| {
                if i >= m.ins() || o >= m.outs() {
                    return Err("ponto fora da matriz".into());
                }
                m.set(i, o, gain);
                Ok(())
            })?;
        }
        "/api/preset" => {
            let kind = q("kind").unwrap_or_default();
            matrix(&mut |m| {
                *m = if kind == "identity" { Matrix::identity(m.ins(), m.outs()) } else { Matrix::new(m.ins(), m.outs()) };
                Ok(())
            })?;
        }
        "/api/rx" => {
            let slot = num("slot")?;
            if slot >= RX_SLOTS {
                return Err("posicao invalida".into());
            }
            shared.config.lock().unwrap().rx[slot] = q("name").unwrap_or_default();
        }
        "/api/setup" => {
            // Placa de rede: vazio (automatica), o nome de uma placa, ou um IP fixo.
            let iface = q("iface").unwrap_or_default().trim().to_string();
            let looks_ip = !iface.is_empty() && iface.chars().all(|c| c.is_ascii_digit() || c == '.');
            if (looks_ip && iface.parse::<std::net::Ipv4Addr>().is_err()) || iface.len() > 80 || iface.chars().any(char::is_control) {
                return Err("Placa de rede ou IP invalido".into());
            }
            let name = q("name").unwrap_or_default().trim().to_string();
            if name.chars().count() > crate::state::NAME_MAX || name.chars().any(char::is_control) {
                return Err(format!("O nome pode ter ate {} letras.", crate::state::NAME_MAX));
            }
            let mut c = shared.config.lock().unwrap();
            c.device = q("device").unwrap_or_default();
            c.iface = iface;
            c.name = name; // vazio = automatico (nome do computador)
            c.tx_channels = q("tx").and_then(|v| v.parse().ok()).unwrap_or(0);
            c.latency_ms = q("latency").and_then(|v| v.parse().ok()).unwrap_or(4).clamp(1, 100);
            c.rate = q("rate").and_then(|v| v.parse().ok()).unwrap_or(0);
            // Placa virtual: so muda o que veio no pedido.
            if let Some(n) = q("card").and_then(|v| v.parse().ok()).filter(|n| crate::state::CARD_CHANNELS.contains(n)) {
                c.card_channels = n;
            }
            if let Some(n) = q("buffer").and_then(|v| v.parse().ok()).filter(|n| crate::state::BUFFERS.contains(n)) {
                c.buffer = n;
            }
            if let Some(v) = q("sys") {
                c.sys_audio = v == "1";
            }
            drop(c);
            shared.save();
            shared.restart.store(true, Relaxed);
            return Ok(());
        }
        "/api/quit" => {
            shared.quit.store(true, Relaxed);
            return Ok(());
        }
        _ => return Err("comando desconhecido".into()),
    }
    shared.save();
    Ok(())
}

fn state_json(shared: &Shared) -> String {
    let st = shared.status.lock().unwrap().clone();
    let cfg = shared.config.lock().unwrap().clone();
    let points = |m: &Matrix| {
        let p: Vec<String> = m.points().iter().map(|(i, o, db)| format!("[{i},{o},{db}]")).collect();
        format!("[{}]", p.join(","))
    };
    let levels = |v: Vec<f32>| {
        let p: Vec<String> = v.iter().map(|x| format!("{:.4}", x.min(4.0))).collect();
        format!("[{}]", p.join(","))
    };
    let devices: Vec<String> = shared.devices.lock().unwrap().iter()
        .map(|(n, i, o)| format!("{{\"name\":{},\"ins\":{i},\"outs\":{o}}}", json_str(n))).collect();
    let flows: Vec<String> = shared.flows.lock().unwrap().iter()
        .map(|f| format!("{{\"name\":{},\"addr\":{},\"ch\":{},\"ok\":{},\"origin\":{}}}", json_str(&f.name), json_str(&f.addr), f.channels, f.compatible, json_str(&f.origin))).collect();
    let ifaces: Vec<String> = shared.ifaces.lock().unwrap().iter()
        .map(|i| format!("{{\"id\":{},\"label\":{},\"ip\":\"{}\"}}", json_str(&i.id), json_str(&i.label), i.ip)).collect();
    let slots: Vec<String> = cfg.rx.iter().map(|s| json_str(s)).collect();
    let active: Vec<String> = shared.slot_active.lock().unwrap().iter().map(|b| b.to_string()).collect();
    format!(
        "{{\"running\":{},\"error\":{},\"device\":{},\"iface\":{},\"ins\":{},\"outs\":{},\"ntx\":{},\"rate\":{},\"nrx\":{NET_RX},\
         \"ptp\":{},\"gm\":{},\"gmIp\":{},\"gmName\":{},\"gmSelf\":{},\"name\":{},\"autoName\":{},\"ifaceLabel\":{},\"notice\":{},\"ifaces\":[{}],\"virtual\":{},\"auxName\":{},\
         \"txFill\":{},\"under\":{},\"over\":{},\
         \"cfg\":{{\"device\":{},\"iface\":{},\"name\":{},\"tx\":{},\"latency\":{},\"rate\":{},\"cardCh\":{},\"buffer\":{},\"sysAudio\":{}}},\
         \"devices\":[{}],\"flows\":[{}],\"slots\":[{}],\"active\":[{}],\"tx\":{},\"rx\":{},\"lvIn\":{},\"lvRx\":{}}}",
        st.running, json_str(&st.error), json_str(&st.device), json_str(&st.iface), st.ins, st.outs, st.net_tx, st.rate,
        st.ptp, json_str(&st.grandmaster), json_str(&st.gm_ip), json_str(&st.gm_name), st.gm_self,
        json_str(&st.name), json_str(&st.auto_name), json_str(&st.iface_label), json_str(&st.notice), ifaces.join(","), st.virtual_card, json_str(&st.aux_name), st.tx_fill, st.underruns, st.overruns,
        json_str(&cfg.device), json_str(&cfg.iface), json_str(&cfg.name), cfg.tx_channels, cfg.latency_ms, cfg.rate, cfg.card_channels, cfg.buffer, cfg.sys_audio,
        devices.join(","), flows.join(","), slots.join(","), active.join(","),
        points(&shared.tx.snapshot()), points(&shared.rx.snapshot()),
        levels(take_peaks(&shared.in_peak, st.ins)), levels(take_peaks(&shared.rx_peak, NET_RX)),
    )
}

pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

pub fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' => {
                match b.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Config;

    fn request(port: u16, req: &str) -> String {
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.write_all(req.as_bytes()).unwrap();
        let mut s = String::new();
        c.read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn page_shows_version_and_system() {
        let p = page();
        assert!(!p.contains("{{"), "sobrou marcador no painel");
        assert!(p.contains(&format!("v{} · {}", crate::VERSION, os_name())));
        assert!(p.contains(&format!("<title>AOIP-Bridge {}</title>", crate::VERSION)));
    }

    #[test]
    fn decode_and_escape() {
        assert_eq!(url_decode("MESA%20%3A+1%"), "MESA : 1%");
        assert_eq!(url_decode("a%zz%4"), "a%zz%4");
        assert_eq!(url_decode("caf%C3%A9"), "café");
        assert_eq!(json_str("a\"b\\c\n"), "\"a\\\"b\\\\c\\u000a\"");
    }

    #[test]
    fn api_end_to_end() {
        let dir = std::env::temp_dir().join(format!("aoip-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let shared = Arc::new(Shared::new(Config::default(), dir.join("c.conf")));
        shared.tx.edit(|m| *m = Matrix::new(4, 8));
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        serve(shared.clone(), l);
        let post = |path: &str| request(port, &format!("POST {path} HTTP/1.1\r\nHost: localhost:{port}\r\nX-AoIP: 1\r\n\r\n"));

        assert!(request(port, "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").contains("<html"));
        assert!(post("/api/point?m=tx&i=2&o=5&db=-6").contains("\"ok\":true"));
        assert_eq!(shared.tx.snapshot().to_spec(), "3>6@-6");
        assert!(post("/api/point?m=tx&i=9&o=0&db=0").starts_with("HTTP/1.1 400"));
        assert!(post("/api/rx?slot=1&name=MESA%20%3A%202").contains("true"));
        let state = request(port, "GET /api/state HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(state.contains("\"tx\":[[2,5,-6]]"), "{state}");
        assert!(state.contains("\"slots\":[\"\",\"MESA : 2\",\"\""), "{state}");
        // config foi gravada em disco
        let saved = Config::parse(&std::fs::read_to_string(dir.join("c.conf")).unwrap());
        assert_eq!((saved.tx_map.as_deref(), saved.rx[1].as_str()), (Some("3>6@-6"), "MESA : 2"));
        assert!(post("/api/preset?m=tx&kind=identity").contains("true"));
        assert_eq!(shared.tx.snapshot().points().len(), 4);
        assert!(post("/api/setup?device=Ui24R&iface=999.1.1.1").starts_with("HTTP/1.1 400"));
        assert!(post("/api/setup?device=Ui24R&name=UM-NOME-COMPRIDO-DEMAIS-PARA-REDE").starts_with("HTTP/1.1 400"));
        assert!(post("/api/setup?device=Ui24R&iface=Ethernet%202&name=Mesa").contains("true"));
        assert_eq!(shared.config.lock().unwrap().iface, "Ethernet 2");
        assert!(post("/api/setup?device=Ui24R&iface=10.0.0.5&name=&tx=16&latency=8").contains("true"));
        assert!(shared.restart.load(Relaxed));
        assert_eq!(shared.config.lock().unwrap().name, "", "vazio = nome do computador");

        // protecoes: POST sem o cabecalho proprio e Host externo sao recusados
        assert!(request(port, "POST /api/quit HTTP/1.1\r\nHost: localhost\r\n\r\n").starts_with("HTTP/1.1 404"));
        assert!(request(port, "GET /api/state HTTP/1.1\r\nHost: evil.example\r\n\r\n").starts_with("HTTP/1.1 403"));
        assert!(!shared.quit.load(Relaxed));
        std::fs::remove_dir_all(dir).ok();
    }
}
