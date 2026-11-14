//! Motor de rede do AoIP Bridge: relogio PTP, envio e recepcao de fluxos AES67 e as duas
//! pontas de audio (`Capture` e `Playback`) que a "placa" alimenta. A placa pode ser uma
//! placa de verdade (programa `aoip-bridge`, via cpal) ou o driver ASIO virtual.
pub mod local;
pub mod net;
pub mod virtual_card;

use aoip_core::state::{auto_name, choose_iface, device_of_flow, note_peaks, Config, Flow, Shared, NET_RX, RX_SLOTS};
use aoip_core::{matrix::Matrix, ptp, resample::DriftFifo, rtp, sap, sdp::Session};
use aoip_core::{FRAMES_PER_PACKET, MAX_FLOW_CHANNELS, RATE};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub type Res<T> = Result<T, Box<dyn std::error::Error>>;
/// Nomes dos fluxos que este programa envia (para nao aparecerem na lista de recepcao).
type OwnFlows = Arc<Mutex<Vec<String>>>;
pub type Fifo = Arc<Mutex<DriftFifo>>;

/// Autoteste (variavel de ambiente AOIP_SELFTEST=1): sem placa com sinal, sem relogio
/// PTP e sem outro aparelho. Um tom de teste entra nos canais 1 e 2, o relogio local
/// faz o papel do PTP e os pacotes ficam so neste computador (TTL 0), de modo que o
/// programa consegue assinar os proprios fluxos. Serve para conferir o caminho
/// placa -> rede -> placa; nao valida sincronismo com aparelhos de verdade.
pub fn selftest() -> bool {
    std::env::var_os("AOIP_SELFTEST").is_some_and(|v| v == "1" || v == "2")
}

/// No autoteste "1" um tom de teste substitui o audio da placa. No "2" o audio da placa e
/// mantido (usado pelo teste do driver ASIO, em que o programa de audio gera o proprio tom).
fn selftest_tone() -> bool {
    std::env::var_os("AOIP_SELFTEST").is_some_and(|v| v == "1")
}

/// Mostra uma linha do autoteste e tambem a grava em `aoip-bridge-autoteste.log` na pasta
/// do usuario, para poder ser enviada depois.
pub fn report(line: &str) {
    use std::io::Write;
    println!("{line}");
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default();
    let path = std::path::Path::new(&home).join("aoip-bridge-autoteste.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

/// Porta RTP dos fluxos enviados. O autoteste usa outra porta porque a 5004 pode estar
/// presa por outro programa de audio em rede neste computador.
fn tx_port() -> u16 {
    if selftest() { 5006 } else { 5004 }
}

/// Contadores do autoteste: blocos e quadros vindos da placa, pacotes enviados e recebidos.
static CAP_CALLS: AtomicU64 = AtomicU64::new(0);
static CAP_FRAMES: AtomicU64 = AtomicU64::new(0);
static TX_PKTS: AtomicU64 = AtomicU64::new(0);
static RX_PKTS: AtomicU64 = AtomicU64::new(0);
/// Maiores pausas (em microssegundos) vistas no periodo: laco de envio e blocos da placa.
static TX_GAP_US: AtomicU64 = AtomicU64::new(0);
static CAP_GAP_US: AtomicU64 = AtomicU64::new(0);
static TX_BURST: AtomicU64 = AtomicU64::new(0);
static RX_GAP_US: AtomicU64 = AtomicU64::new(0);
static PLAY_GAP_US: AtomicU64 = AtomicU64::new(0);

/// Relogio PTP compartilhado: hora PTP = relogio monotonico local + deslocamento.
pub struct Clock {
    pub base: Instant,
    pub offset: AtomicI64,
    pub locked: AtomicBool,
    pub gmid: Mutex<Option<[u8; 8]>>,
    /// Este computador esta gerando o relogio (nao ha outro na rede).
    pub master: AtomicBool,
    /// De que IP veio o anuncio de cada relogio (para mostrar quem e o mestre).
    pub announcers: Mutex<HashMap<[u8; 8], Ipv4Addr>>,
    /// IP deste programa na rede de audio.
    pub iface: Ipv4Addr,
    /// Pedido para desligar a rede (todas as threads deste motor terminam em ate ~0,5 s).
    pub stop: AtomicBool,
}

impl Clock {
    fn local_ns(&self) -> i128 {
        self.base.elapsed().as_nanos() as i128
    }
    fn now(&self) -> Option<u128> {
        self.locked.load(Relaxed).then(|| (self.local_ns() + self.offset.load(Relaxed) as i128) as u128)
    }
}

/// Pede ao macOS prioridade de tempo real para a thread atual (a mesma classe das threads
/// de audio do sistema). Sem isso o laco de envio, que precisa acordar a cada 1 ms, sofre
/// pausas de varios milissegundos e, de vez em quando, de mais de 100 ms.
pub fn realtime_thread() -> bool {
    realtime_thread_for(Duration::from_millis(1), Duration::from_micros(200))
}

/// Prioridade de tempo real para uma thread que acorda a cada `period` e trabalha ate
/// `busy` por vez. (No macOS quem passa do combinado perde a prioridade; no Windows os
/// valores nao sao usados.)
#[cfg(target_os = "macos")]
pub fn realtime_thread_for(period: Duration, busy: Duration) -> bool {
    #[repr(C)]
    struct Timebase { numer: u32, denom: u32 }
    #[repr(C)]
    struct TimeConstraint { period: u32, computation: u32, constraint: u32, preemptible: u32 }
    extern "C" {
        fn mach_timebase_info(info: *mut Timebase) -> i32;
        fn mach_thread_self() -> u32;
        fn thread_policy_set(thread: u32, flavor: u32, policy: *const TimeConstraint, count: u32) -> i32;
    }
    const THREAD_TIME_CONSTRAINT_POLICY: u32 = 2;
    let mut tb = Timebase { numer: 0, denom: 0 };
    // SAFETY: chamadas do kernel Mach com estruturas no formato documentado.
    unsafe {
        if mach_timebase_info(&mut tb) != 0 || tb.numer == 0 {
            return false;
        }
        let ticks = |ns: u64| (ns * tb.denom as u64 / tb.numer as u64) as u32;
        let (period, busy) = (period.as_nanos() as u64, busy.as_nanos() as u64);
        let policy = TimeConstraint { period: ticks(period), computation: ticks(busy), constraint: ticks((busy * 2).min(period)), preemptible: 1 };
        thread_policy_set(mach_thread_self(), THREAD_TIME_CONSTRAINT_POLICY, &policy, 4) == 0
    }
}

/// Windows: registra a thread na classe "Pro Audio" do agendador multimidia (MMCSS) e
/// pede temporizador de 1 ms, o mesmo que os programas de audio fazem.
#[cfg(windows)]
pub fn realtime_thread_for(_period: Duration, _busy: Duration) -> bool {
    #[link(name = "avrt")]
    extern "system" {
        fn AvSetMmThreadCharacteristicsW(task: *const u16, index: *mut u32) -> *mut std::ffi::c_void;
    }
    #[link(name = "winmm")]
    extern "system" {
        fn timeBeginPeriod(ms: u32) -> u32;
    }
    let task: Vec<u16> = "Pro Audio".encode_utf16().chain(std::iter::once(0)).collect();
    let mut index = 0u32;
    // SAFETY: chamadas da API do Windows com texto UTF-16 terminado em zero.
    unsafe {
        timeBeginPeriod(1);
        !AvSetMmThreadCharacteristicsW(task.as_ptr(), &mut index).is_null()
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn realtime_thread_for(_period: Duration, _busy: Duration) -> bool {
    false
}

/// Escuta Sync/Follow_Up/Announce e mantem o relogio compartilhado. Se nao houver nenhum
/// relogio PTPv2 na rede, este computador passa a gerar um (veja `ptp::Election`).
fn spawn_ptp(clock: Arc<Clock>, iface: Ipv4Addr, instance: u8) -> Res<()> {
    let own = ptp::clock_id(iface, instance);
    let follower = Arc::new(Mutex::new(ptp::Follower::default()));
    // Quem esta com placa de verdade (a mesa) tem preferencia para ser o relogio.
    let priority1 = if instance == 0 { ptp::CARD_PRIORITY1 } else { ptp::OWN_PRIORITY1 };
    let election = Arc::new(Mutex::new(ptp::Election::new(own, priority1, clock.local_ns())));
    for port in [ptp::EVENT_PORT, ptp::GENERAL_PORT] {
        let sock = net::listener(ptp::GROUP, port, iface)
            .map_err(|e| format!("Nao consegui abrir a porta {port} do relogio PTP ({e}). Outro programa de audio em rede pode estar usando."))?;
        sock.set_read_timeout(Some(Duration::from_millis(500)))?; // para notar o pedido de desligar
        let (clock, follower, election) = (clock.clone(), follower.clone(), election.clone());
        std::thread::spawn(move || {
            realtime_thread(); // o carimbo de chegada e tirado aqui: atraso vira erro de relogio
            let mut buf = [0u8; 512];
            while !clock.stop.load(Relaxed) {
                let (n, from) = match sock.recv_from(&mut buf) {
                    Ok(x) => x,
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
                    Err(_) => break,
                };
                let local = clock.local_ns();
                let pkt = &buf[..n];
                let Some((domain, msg)) = ptp::parse(pkt) else { continue };
                if domain != 0 || ptp::source_id(pkt) == Some(own) {
                    continue; // outro dominio, ou eco das nossas proprias mensagens
                }
                if let Some((p1, gm)) = ptp::announce_rank(pkt) {
                    election.lock().unwrap().on_announce(p1, gm, local);
                    if let IpAddr::V4(ip) = from.ip() {
                        clock.announcers.lock().unwrap().insert(gm, ip);
                    }
                }
                if clock.master.load(Relaxed) {
                    continue; // enquanto geramos o relogio, nao seguimos ninguem
                }
                let mut f = follower.lock().unwrap();
                if let Some(off) = f.on_msg(msg, local) {
                    clock.offset.store(off as i64, Relaxed);
                    clock.locked.store(true, Relaxed);
                }
                if f.gmid.is_some() {
                    *clock.gmid.lock().unwrap() = f.gmid;
                }
            }
        });
    }

    // Relogio proprio: 8 Sync/Follow_Up e 1 Announce por segundo enquanto formos o mestre.
    let sock = net::sender(iface, false)?;
    std::thread::spawn(move || {
        realtime_thread();
        let (mut seq, mut tick) = (0u16, 0u32);
        while !clock.stop.load(Relaxed) {
            std::thread::sleep(Duration::from_millis(125));
            let is_master = election.lock().unwrap().is_master(clock.local_ns());
            if is_master != clock.master.swap(is_master, Relaxed) {
                // Trocou de papel: o seguidor recomeca do zero na proxima vez.
                *follower.lock().unwrap() = ptp::Follower::default();
                if is_master {
                    if !clock.locked.load(Relaxed) {
                        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
                        clock.offset.store(unix as i64 - clock.local_ns() as i64, Relaxed);
                    }
                    *clock.gmid.lock().unwrap() = Some(own);
                    clock.locked.store(true, Relaxed);
                } else {
                    clock.locked.store(false, Relaxed); // ate travar no relogio novo
                }
            }
            if !is_master {
                continue;
            }
            let Some(now) = clock.now() else { continue };
            seq = seq.wrapping_add(1);
            let _ = sock.send_to(&ptp::build_sync(own, seq), (ptp::GROUP, ptp::EVENT_PORT));
            let _ = sock.send_to(&ptp::build_follow_up(own, seq, now), (ptp::GROUP, ptp::GENERAL_PORT));
            if tick % 8 == 0 {
                let _ = sock.send_to(&ptp::build_announce(own, (tick / 8) as u16, now, priority1), (ptp::GROUP, ptp::GENERAL_PORT));
            }
            tick = tick.wrapping_add(1);
        }
    });
    Ok(())
}

/// Controller: anuncia esta maquina a rede uma vez por segundo e guarda as outras que se
/// anunciam (para o painel mostrar todas e rotear entre elas).
fn spawn_nodes(shared: Arc<Shared>, clock: Arc<Clock>, iface: Ipv4Addr, id: String, control: u16) -> Res<()> {
    use aoip_core::routing::{note_node, NodeInfo, ANNOUNCE_GROUP, ANNOUNCE_PORT};
    let sender = net::sender(iface, selftest())?;
    let sock = net::listener(ANNOUNCE_GROUP, ANNOUNCE_PORT, iface)?;
    sock.set_read_timeout(Some(Duration::from_millis(250)))?;
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 16 * 1024];
        let mut last = None::<Instant>;
        while !clock.stop.load(Relaxed) {
            if last.map_or(true, |t| t.elapsed() >= Duration::from_secs(1)) {
                last = Some(Instant::now());
                let own = NodeInfo::from_shared(&shared, id.clone(), iface, control);
                let _ = sender.send_to(own.encode().as_bytes(), (ANNOUNCE_GROUP, ANNOUNCE_PORT));
                note_node(&mut shared.nodes.lock().unwrap(), own, true, Instant::now());
            }
            if let Ok((n, _)) = sock.recv_from(&mut buf) {
                if let Some(info) = std::str::from_utf8(&buf[..n]).ok().and_then(NodeInfo::parse) {
                    if info.id != id {
                        note_node(&mut shared.nodes.lock().unwrap(), info, false, Instant::now());
                    }
                }
            }
        }
    });
    Ok(())
}

/// Controller: atende os comandos de roteamento que outros paineis mandam para esta maquina
/// ("ligue sua entrada 3 no canal 2 do fluxo X", "renomeie seu canal 5"). So da rede local.
fn spawn_control(shared: Arc<Shared>, clock: Arc<Clock>, port: u16) -> Res<()> {
    let listener = std::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .map_err(|e| format!("Nao consegui abrir a porta {port} do Controller ({e})."))?;
    listener.set_nonblocking(true)?;
    std::thread::spawn(move || {
        while !clock.stop.load(Relaxed) {
            let Ok((conn, peer)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            };
            let local = match peer.ip() {
                IpAddr::V4(ip) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
                IpAddr::V6(ip) => ip.is_loopback(),
            };
            let shared = shared.clone();
            std::thread::spawn(move || {
                use std::io::{BufRead, BufReader, Write};
                let _ = conn.set_nonblocking(false);
                let _ = conn.set_read_timeout(Some(Duration::from_secs(3)));
                let mut line = String::new();
                if BufReader::new(&conn).read_line(&mut line).is_err() {
                    return;
                }
                let reply = if local { aoip_core::routing::handle_command(&shared, &line) } else { "ERR so aceito comandos da rede local".into() };
                let _ = (&conn).write_all(format!("{reply}\n").as_bytes());
            });
        }
    });
    Ok(())
}

struct TxFlow {
    session: Session,
    dest: SocketAddrV4,
    first_ch: usize,
}

/// Empacota 1 ms de audio por vez, no ritmo do relogio PTP, e anuncia via SAP.
fn spawn_tx(clock: Arc<Clock>, fifo: Fifo, mut flows: Vec<TxFlow>, iface: Ipv4Addr, ssrc_salt: u32) -> Res<()> {
    let sock = net::sender(iface, selftest())?;
    let ch = fifo.lock().unwrap().channels();
    std::thread::spawn(move || {
        let ns_per_pkt = FRAMES_PER_PACKET as u128 * 1_000_000_000 / RATE as u128;
        let mut block = vec![0f32; FRAMES_PER_PACKET * ch];
        let mut part = Vec::new();
        let mut pkt = Vec::new();
        let (mut seq, mut next, mut last_sap) = (0u16, 0u128, None::<Instant>);
        let ssrc = u32::from(iface) ^ 0x4145_5336 ^ ssrc_salt;
        if !realtime_thread() {
            eprintln!("aviso: sem prioridade de tempo real no envio; pode haver falhas no audio.");
        }
        let mut woke = Instant::now();
        while !clock.stop.load(Relaxed) {
            std::thread::sleep(Duration::from_micros(250));
            TX_GAP_US.fetch_max(woke.elapsed().as_micros() as u64, Relaxed);
            woke = Instant::now();
            let gmid = *clock.gmid.lock().unwrap();
            let (Some(now), Some(gmid)) = (clock.now(), gmid) else { continue };

            if last_sap.map_or(true, |t| t.elapsed() > Duration::from_secs(10)) {
                last_sap = Some(Instant::now());
                for f in &mut flows {
                    f.session.refclk = Some((gmid, 0));
                    let msg = sap::build(false, f.session.session_id as u16, iface, &f.session.to_sdp());
                    let _ = sock.send_to(&msg, (sap::GROUP, sap::PORT));
                }
            }

            // Fora de sincronia (inicio, ou o relogio deu um salto): realinha.
            if next == 0 || now + 100_000_000 < next || now > next + 100_000_000 {
                next = now / ns_per_pkt * ns_per_pkt + ns_per_pkt;
            }
            TX_BURST.fetch_max(((now.saturating_sub(next)) / ns_per_pkt) as u64 + 1, Relaxed);
            while next <= now {
                fifo.lock().unwrap().pull(&mut block);
                let ts = rtp::timestamp_from_ptp(next, RATE);
                for f in &flows {
                    let n = f.session.channels as usize;
                    part.clear();
                    for frame in block.chunks_exact(ch) {
                        part.extend_from_slice(&frame[f.first_ch..f.first_ch + n]);
                    }
                    rtp::build(&mut pkt, f.session.pt, seq, ts, ssrc, &part);
                    let _ = sock.send_to(&pkt, f.dest);
                }
                TX_PKTS.fetch_add(1, Relaxed);
                seq = seq.wrapping_add(1);
                next += ns_per_pkt;
            }
        }
        // Desligando: avisa a rede que os fluxos acabaram (somem das listas dos outros na hora).
        for f in &flows {
            let msg = sap::build(true, f.session.session_id as u16, iface, &f.session.to_sdp());
            let _ = sock.send_to(&msg, (sap::GROUP, sap::PORT));
        }
    });
    Ok(())
}

/// Descobre fluxos via SAP e mantem as assinaturas escolhidas no painel.
fn spawn_rx(shared: Arc<Shared>, fifos: Vec<Fifo>, iface: Ipv4Addr, clock: Arc<Clock>, own: OwnFlows) -> Res<()> {
    let sock = net::listener(sap::GROUP, sap::PORT, iface)?;
    sock.set_read_timeout(Some(Duration::from_millis(500)))?;
    std::thread::spawn(move || {
        let mut buf = [0u8; 2048];
        let mut known: Vec<Session> = Vec::new();
        let mut running: Vec<Option<(String, Arc<AtomicBool>)>> = (0..RX_SLOTS).map(|_| None).collect();
        let usable = |s: &Session| s.rate == RATE && (s.bits == 24 || s.bits == 16);
        loop {
            if clock.stop.load(Relaxed) {
                for (_, stop) in running.iter_mut().filter_map(Option::take) {
                    stop.store(true, Relaxed);
                }
                *shared.slot_active.lock().unwrap() = vec![false; RX_SLOTS];
                return;
            }
            if let Ok((n, _)) = sock.recv_from(&mut buf) {
                if let Some((ann, s)) = sap::parse(&buf[..n]).and_then(|a| Session::parse(a.sdp).map(|s| (a, s))) {
                    // Os fluxos deste programa nao aparecem na lista; os de outro programa no
                    // mesmo computador aparecem (dá para receber a mesa na placa virtual).
                    if !own.lock().unwrap().contains(&s.name) || selftest() {
                        known.retain(|k| k.name != s.name);
                        if !ann.delete {
                            known.push(s);
                        }
                        known.sort_by(|a, b| a.name.cmp(&b.name));
                        *shared.flows.lock().unwrap() = known.iter()
                            .map(|k| Flow { name: k.name.clone(), addr: format!("{}:{}", k.group, k.port), channels: k.channels, origin: k.origin.to_string(), compatible: usable(k) })
                            .collect();
                    }
                }
            }
            let want = shared.config.lock().unwrap().rx.clone();
            for slot in 0..RX_SLOTS {
                let current = running[slot].as_ref().map(|r| r.0.clone()).unwrap_or_default();
                if current != want[slot] {
                    if let Some((_, stop)) = running[slot].take() {
                        stop.store(true, Relaxed);
                    }
                }
                if running[slot].is_none() && !want[slot].is_empty() {
                    if let Some(s) = known.iter().find(|k| k.name == want[slot] && usable(k)) {
                        match net::listener(s.group, s.port, iface) {
                            Ok(sock) => {
                                let stop = Arc::new(AtomicBool::new(false));
                                running[slot] = Some((s.name.clone(), stop.clone()));
                                let (s, fifo) = (s.clone(), fifos[slot].clone());
                                std::thread::spawn(move || receive_flow(s, sock, fifo, stop));
                            }
                            Err(e) => {
                                // Tenta de novo na proxima volta; o painel mostra o motivo.
                                shared.status.lock().unwrap().error = format!(
                                    "Nao consegui receber \"{}\": a porta {} esta em uso por outro programa de audio em rede neste computador ({e}).",
                                    s.name, s.port
                                );
                            }
                        }
                    }
                }
            }
            *shared.slot_active.lock().unwrap() = running.iter().map(|r| r.is_some()).collect();
        }
    });
    Ok(())
}

fn receive_flow(s: Session, sock: std::net::UdpSocket, fifo: Fifo, stop: Arc<AtomicBool>) {
    let _ = sock.set_read_timeout(Some(Duration::from_millis(300)));
    let (src_ch, dst_ch) = (s.channels as usize, MAX_FLOW_CHANNELS);
    let mut buf = [0u8; 2048];
    let (mut samples, mut wide) = (Vec::new(), Vec::new());
    // Mesma razao do envio: sem prioridade a thread acorda atrasada e os pacotes chegam em rajadas.
    realtime_thread();
    let mut last = Instant::now();
    while !stop.load(Relaxed) {
        let Ok((n, from)) = sock.recv_from(&mut buf) else { last = Instant::now(); continue };
        RX_GAP_US.fetch_max(last.elapsed().as_micros() as u64, Relaxed);
        last = Instant::now();
        // A porta pode ser compartilhada por varios grupos: confere a origem.
        if from.ip() != std::net::IpAddr::V4(s.origin) {
            continue;
        }
        let Some(p) = rtp::parse(&buf[..n]) else { continue };
        if p.pt != s.pt {
            continue;
        }
        samples.clear();
        rtp::decode(p.payload, s.bits, &mut samples);
        // Cada fluxo ocupa sempre 8 canais na fila (os que sobram ficam mudos).
        wide.clear();
        for frame in samples.chunks_exact(src_ch) {
            wide.extend((0..dst_ch).map(|c| frame.get(c).copied().unwrap_or(0.0)));
        }
        fifo.lock().unwrap().push(&wide);
        RX_PKTS.fetch_add(1, Relaxed);
    }
}

/// O que a placa entrega: quadros intercalados com `dev_ins` canais, na taxa da placa.
pub type Capture = Box<dyn FnMut(&[f32]) + Send>;
/// O que a placa pede: preenche quadros intercalados com `dev_outs` canais.
pub type Playback = Box<dyn FnMut(&mut [f32]) + Send>;

/// Parte de rede em funcionamento (as threads seguem rodando ate o programa terminar).
pub struct Network {
    pub clock: Arc<Clock>,
    pub tx_fifo: Fifo,
    pub rx_fifos: Vec<Fifo>,
    pub net_tx: usize,
    /// Nome deste programa na rede.
    pub name: String,
    /// Base do nome do fluxo do som do computador: o nome escolhido, ou o do computador.
    aux_base: String,
    group_base: u32,
    flow_base: u32,
    own: OwnFlows,
}

/// Nome do fluxo a parte com o som do computador.
pub fn aux_flow_name(name: &str) -> String {
    format!("{name} - Saida de Audio")
}

impl Network {
    /// Desliga a rede deste motor: param o relogio, o envio, a recepcao e os anuncios, e as
    /// portas sao soltas. Usado para aplicar uma configuracao nova sem fechar o programa.
    pub fn shutdown(&self) {
        self.clock.stop.store(true, Relaxed);
    }

    /// Liga um fluxo a parte, fora da matriz, com o som do computador ("NOME - Saida de
    /// Audio", 2 canais). `ch` e `rate`: formato de quem entrega o som (mais de 2 canais sao
    /// somados em 2). O fluxo para junto com a rede.
    pub fn start_aux(&self, ch: usize, rate: u32) -> Res<Capture> {
        let ch = ch.max(1);
        let iface = self.clock.iface;
        let name = aux_flow_name(&self.aux_base);
        let group = Ipv4Addr::from(self.group_base + MAX_FLOW_CHANNELS as u32); // depois dos 8 fluxos possiveis
        let flow = TxFlow {
            dest: SocketAddrV4::new(group, tx_port()),
            first_ch: 0,
            session: Session {
                name: name.clone(), origin: iface, session_id: (u32::from(iface) as u64) << 8 | (self.flow_base as u64 + 8),
                group, port: tx_port(), pt: 97, channels: 2, rate: RATE, bits: 24, refclk: None,
            },
        };
        let target = 8 * rate as usize / 1000; // 8 ms de folga: o som do computador nao e critico
        let fifo: Fifo = Arc::new(Mutex::new(DriftFifo::with_ratio(2, target, rate as f64 / RATE as f64)));
        self.own.lock().unwrap().push(name);
        spawn_tx(self.clock.clone(), fifo.clone(), vec![flow], iface, 0x5A5A_0000)?;
        let mut stereo: Vec<f32> = Vec::new();
        Ok(Box::new(move |input: &[f32]| {
            let frames = input.len() / ch;
            stereo.clear();
            for frame in input.chunks_exact(ch) {
                let (l, r) = match ch {
                    1 => (frame[0], frame[0]),
                    _ => (frame[0], frame[1]),
                };
                stereo.push(l);
                stereo.push(r);
            }
            let mut f = fifo.lock().unwrap();
            f.note_block(frames);
            f.push(&stereo);
        }))
    }
}

/// Atualiza a lista de placas de rede que o painel oferece.
pub fn refresh_ifaces(shared: &Shared) {
    *shared.ifaces.lock().unwrap() = net::interfaces();
}

/// IP da rede de audio, pela placa de rede escolhida em Configuracao. O IP e lido na hora,
/// entao muda sozinho de uma rede para outra. Se o que foi salvo nao existe mais (o IP de
/// outra rede, ou a placa desligada), usa a rede automatica e deixa um aviso no painel.
pub fn resolve_iface(shared: &Shared, cfg: &Config) -> Res<Ipv4Addr> {
    let list = net::interfaces();
    let (chosen, notice) = choose_iface(&cfg.iface, &list);
    *shared.ifaces.lock().unwrap() = list.clone();
    let ip = match chosen.or_else(net::guess_iface).or_else(|| list.first().map(|i| i.ip)) {
        Some(ip) => ip,
        None => return Err("Nenhuma rede ligada neste computador. Conecte o cabo de rede (ou o Wi-Fi) e tente de novo.".into()),
    };
    let label = list.iter().find(|i| i.ip == ip).map(|i| i.label.clone()).unwrap_or_default();
    let mut st = shared.status.lock().unwrap();
    st.notice = notice.unwrap_or_default();
    st.iface_label = label;
    Ok(ip)
}

/// Liga a rede para uma placa com `dev_ins` entradas e `dev_outs` saidas em `dev_rate` Hz.
/// `flow_base` desloca os enderecos multicast dos fluxos enviados (para dois programas no
/// mesmo computador nao usarem o mesmo endereco).
pub fn start_network(
    shared: &Arc<Shared>, cfg: &Config, iface: Ipv4Addr, dev_ins: usize, dev_outs: usize, dev_rate: u32, flow_base: u32,
) -> Res<(Network, Capture, Playback)> {
    let net_tx = if cfg.tx_channels == 0 { dev_ins } else { cfg.tx_channels }.clamp(1, 64);
    // Nome na rede: o escolhido, ou o nome do computador ("PC-IGREJA"); a placa virtual
    // ganha " Virtual" para nao se confundir com o AoIP Bridge do mesmo computador.
    let auto = auto_name(&net::computer_name(), if flow_base == 0 { "" } else { " Virtual" });
    let name = cfg.flow_name(&auto);
    {
        let mut st = shared.status.lock().unwrap();
        (st.name, st.auto_name) = (name.clone(), auto);
    }
    let target = cfg.latency_ms.max(1) * RATE as usize / 1000;
    let tx_target = cfg.latency_ms.max(1) * dev_rate as usize / 1000; // em quadros da placa
    shared.status.lock().unwrap().iface = iface.to_string();

    shared.tx.edit(|m| *m = Matrix::from_saved(cfg.tx_map.as_deref(), dev_ins, net_tx));
    shared.rx.edit(|m| *m = Matrix::from_saved(cfg.rx_map.as_deref(), NET_RX, dev_outs));

    let clock = Arc::new(Clock {
        base: Instant::now(), offset: AtomicI64::new(0), locked: AtomicBool::new(false), gmid: Mutex::new(None), master: AtomicBool::new(false),
        announcers: Mutex::new(HashMap::new()), iface, stop: AtomicBool::new(false),
    });
    if selftest() {
        // Relogio local no lugar do PTP, com um identificador de grandmaster ficticio.
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        clock.offset.store(unix as i64, Relaxed);
        *clock.gmid.lock().unwrap() = Some([0x02, 0, 0, 0xFF, 0xFE, 0, 0, 0x01]);
        clock.locked.store(true, Relaxed);
    } else {
        spawn_ptp(clock.clone(), iface, if flow_base == 0 { 0 } else { 1 })?;
    }

    // Fluxos TX: ate 8 canais cada, em enderecos multicast consecutivos (239.69.x.y).
    let ip = iface.octets();
    let base = u32::from(Ipv4Addr::new(239, 69, ip[2], ip[3])) + flow_base;
    let flows: Vec<TxFlow> = (0..net_tx.div_ceil(MAX_FLOW_CHANNELS))
        .map(|k| {
            let first_ch = k * MAX_FLOW_CHANNELS;
            let group = Ipv4Addr::from(base + k as u32);
            TxFlow {
                dest: SocketAddrV4::new(group, tx_port()),
                first_ch,
                session: Session {
                    name: format!("{} : {}", name, k + 1), origin: iface,
                    session_id: (u32::from(iface) as u64) << 8 | (flow_base as u64 + k as u64), group, port: tx_port(), pt: 97,
                    channels: (net_tx - first_ch).min(MAX_FLOW_CHANNELS) as u8, rate: RATE, bits: 24, refclk: None,
                },
            }
        })
        .collect();

    let tx_fifo: Fifo = Arc::new(Mutex::new(DriftFifo::with_ratio(net_tx, tx_target, dev_rate as f64 / RATE as f64)));
    let rx_fifos: Vec<Fifo> = (0..RX_SLOTS)
        .map(|_| Arc::new(Mutex::new(DriftFifo::with_ratio(MAX_FLOW_CHANNELS, target, RATE as f64 / dev_rate as f64))))
        .collect();
    let own: OwnFlows = Arc::new(Mutex::new(flows.iter().map(|f| f.session.name.clone()).collect()));
    spawn_tx(clock.clone(), tx_fifo.clone(), flows, iface, 0)?;
    spawn_rx(shared.clone(), rx_fifos.clone(), iface, clock.clone(), own.clone())?;
    // Controller: o AoIP Bridge usa a porta 8791 e a placa virtual a 8792 (podem estar no
    // mesmo computador). Sem a porta de comandos o resto funciona; o painel avisa.
    let instance = if flow_base == 0 { 0u16 } else { 1 };
    let control = aoip_core::routing::CONTROL_PORT + instance;
    if let Err(e) = spawn_control(shared.clone(), clock.clone(), control) {
        shared.status.lock().unwrap().notice = e.to_string();
    }
    spawn_nodes(shared.clone(), clock.clone(), iface, format!("{iface}/{instance}"), control)?;

    // Entrada da placa -> matriz TX -> fila de rede.
    let (sh, fifo) = (shared.clone(), tx_fifo.clone());
    let (mut matrix, mut version) = (sh.tx.snapshot(), sh.tx.version());
    let mut mixed: Vec<f32> = Vec::new();
    let (test, mut tone, mut phase) = (selftest_tone(), Vec::<f32>::new(), 0f32);
    let mut last_cap = Instant::now();
    let capture = move |input: &[f32]| {
        CAP_GAP_US.fetch_max(last_cap.elapsed().as_micros() as u64, Relaxed);
        last_cap = Instant::now();
        CAP_CALLS.fetch_add(1, Relaxed);
        CAP_FRAMES.fetch_add((input.len() / dev_ins) as u64, Relaxed);
        // Autoteste: 440 Hz no canal 1 (-20 dB) e no canal 2 (-26 dB), resto mudo.
        let input = if test {
            tone.clear();
            tone.resize(input.len(), 0.0);
            for frame in tone.chunks_exact_mut(dev_ins) {
                let v = phase.sin();
                phase = (phase + std::f32::consts::TAU * 440.0 / dev_rate as f32) % std::f32::consts::TAU;
                frame[0] = 0.1 * v;
                if dev_ins > 1 {
                    frame[1] = 0.05 * v;
                }
            }
            &tone[..]
        } else {
            input
        };
        if sh.tx.version() != version {
            if let Some(m) = sh.tx.try_snapshot() {
                version = sh.tx.version();
                matrix = m;
            }
        }
        note_peaks(&sh.in_peak, input, dev_ins);
        mixed.resize(input.len() / dev_ins * net_tx, 0.0);
        matrix.process(input, &mut mixed);
        let mut f = fifo.lock().unwrap();
        f.note_block(input.len() / dev_ins);
        f.push(&mixed);
    };
    // Filas de rede -> matriz RX -> saida da placa.
    let (sh, fifos) = (shared.clone(), rx_fifos.clone());
    let (mut matrix, mut version) = (sh.rx.snapshot(), sh.rx.version());
    let (mut net, mut one) = (Vec::<f32>::new(), Vec::<f32>::new());
    let mut last_play = Instant::now();
    let playback = move |output: &mut [f32]| {
        PLAY_GAP_US.fetch_max(last_play.elapsed().as_micros() as u64, Relaxed);
        last_play = Instant::now();
        if sh.rx.version() != version {
            if let Some(m) = sh.rx.try_snapshot() {
                version = sh.rx.version();
                matrix = m;
            }
        }
        let frames = output.len() / dev_outs;
        net.clear();
        net.resize(frames * NET_RX, 0.0);
        one.resize(frames * MAX_FLOW_CHANNELS, 0.0);
        for (i, f) in fifos.iter().enumerate() {
            let mut f = f.lock().unwrap();
            // a fila de recepcao conta em quadros da rede (48000 Hz)
            f.note_block((frames * RATE as usize).div_ceil(dev_rate as usize));
            f.pull(&mut one);
            drop(f);
            for (dst, src) in net.chunks_exact_mut(NET_RX).zip(one.chunks_exact(MAX_FLOW_CHANNELS)) {
                dst[i * MAX_FLOW_CHANNELS..(i + 1) * MAX_FLOW_CHANNELS].copy_from_slice(src);
            }
        }
        note_peaks(&sh.rx_peak, &net, NET_RX);
        matrix.process(&net, output);
    };

    let aux_base = if cfg.name.trim().is_empty() { auto_name(&net::computer_name(), "") } else { name.clone() };
    shared.status.lock().unwrap().aux_name = aux_flow_name(&aux_base);
    let network = Network { clock, tx_fifo, rx_fifos, net_tx, name, aux_base, group_base: base, flow_base, own };
    Ok((network, Box::new(capture), Box::new(playback)))
}

/// Atualiza no estado compartilhado o que o painel mostra sobre relogio e fila.
pub fn refresh_status(shared: &Shared, n: &Network) {
    static CALLS: AtomicU64 = AtomicU64::new(0);
    if CALLS.fetch_add(1, Relaxed) % 20 == 0 {
        refresh_ifaces(shared); // a cada ~5 s: cabo ligado ou desligado aparece no painel
    }
    // Quem e o relogio mestre: IP de quem anunciou; nome pelo nome dos fluxos dele na rede
    // ou, sem fluxos, pelo nome do computador (DNS reverso / NetBIOS / mDNS).
    let gmid = *n.clock.gmid.lock().unwrap();
    let is_self = n.clock.master.load(Relaxed);
    let gm_ip = match gmid {
        _ if is_self => Some(n.clock.iface),
        Some(g) => n.clock.announcers.lock().unwrap().get(&g).copied(),
        None => None,
    };
    let gm_name = match gm_ip {
        _ if is_self => shared.status.lock().unwrap().name.clone(),
        Some(ip) => {
            let from_flows = shared.flows.lock().unwrap().iter()
                .find(|f| f.origin == ip.to_string())
                .map(|f| device_of_flow(&f.name).to_string());
            from_flows.or_else(|| net::name_of(ip)).unwrap_or_default()
        }
        None => String::new(),
    };
    let mut st = shared.status.lock().unwrap();
    st.ptp = n.clock.locked.load(Relaxed);
    st.grandmaster = gmid.map(|g| g.iter().map(|b| format!("{b:02X}")).collect()).unwrap_or_default();
    st.gm_ip = gm_ip.map(|ip| ip.to_string()).unwrap_or_default();
    st.gm_name = gm_name;
    st.gm_self = is_self;
    let f = n.tx_fifo.lock().unwrap();
    (st.tx_fill, st.underruns, st.overruns) = (f.fill(), f.underruns, f.overruns);
}

/// Linhas periodicas do autoteste (a cada 5 s).
pub fn report_stats(shared: &Shared, n: &Network) {
    let (under, over, fill, target) = { let f = n.tx_fifo.lock().unwrap(); (f.underruns, f.overruns, f.fill(), f.target()) };
    let (calls, frames) = (CAP_CALLS.swap(0, Relaxed), CAP_FRAMES.swap(0, Relaxed));
    let st = shared.status.lock().unwrap().clone();
    report(&format!(
        "autoteste 5s [{} {}x{} {} Hz]: {} blocos / {} quadros da placa (bloco {}), {} pacotes enviados, {} recebidos | maior pausa: envio {:.1} ms, placa {:.1} ms, rajada {} pacotes | fila de envio {} (alvo {} = {:.1} ms) vazios {} estouros {}",
        st.device, st.ins, st.outs, st.rate, calls, frames, if calls > 0 { frames / calls } else { 0 },
        TX_PKTS.swap(0, Relaxed), RX_PKTS.swap(0, Relaxed),
        TX_GAP_US.swap(0, Relaxed) as f64 / 1000.0, CAP_GAP_US.swap(0, Relaxed) as f64 / 1000.0, TX_BURST.swap(0, Relaxed),
        fill, target, target as f64 * 1000.0 / st.rate.max(1) as f64, under, over
    ));
    let rx: Vec<String> = n.rx_fifos.iter().map(|f| { let f = f.lock().unwrap(); format!("{}/{}/{}/{}", f.fill(), f.target(), f.underruns, f.overruns) }).collect();
    report(&format!(
        "  filas de recepcao (enchimento/alvo/vazios/estouros): {} | maior pausa: recepcao {:.1} ms, saida da placa {:.1} ms",
        rx.join("  "), RX_GAP_US.swap(0, Relaxed) as f64 / 1000.0, PLAY_GAP_US.swap(0, Relaxed) as f64 / 1000.0
    ));
}

pub fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

