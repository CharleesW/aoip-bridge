//! Driver ASIO virtual "AoIP Bridge ASIO" (Windows, 64 bits).
//!
//! O programa de audio (REAPER etc.) carrega esta DLL como se fosse uma placa de 32x32
//! canais a 48 kHz. O que o programa toca nas saidas vai para a rede (fluxos AES67); os
//! fluxos assinados da rede chegam nas entradas. A rede e o painel sao os mesmos do
//! `aoip-bridge` (crate `aoip-engine`); o painel deste driver fica em http://127.0.0.1:8788.
//!
//! O codigo compila em qualquer sistema para que `examples/host_test.rs` possa exercitar o
//! driver como um programa de audio faria; so o registro e o carregamento sao do Windows.
#![allow(non_snake_case, clippy::missing_safety_doc)]

use aoip_core::state::Shared;
use aoip_core::ui;
use aoip_engine::{open_browser, realtime_thread_for, refresh_ifaces, refresh_status, report_stats, resolve_iface, selftest, start_network, Capture, Network, Playback};
use std::ffi::c_void;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const DRIVER_NAME: &str = "AoIP Bridge ASIO";
pub const CHANNELS: usize = 32;
pub const RATE: u32 = 48_000;
pub const PANEL_PORT: u16 = 8788;
const BUF_MIN: i32 = 64;
const BUF_MAX: i32 = 2048;
const BUF_PREFERRED: i32 = 256;

// ---------------------------------------------------------------- tipos COM / ASIO

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Guid(pub u32, pub u16, pub u16, pub [u8; 8]);

/// {BA273706-2FFA-4FAD-A237-B2BE6CA92823}: identidade deste driver no registro do Windows.
pub const CLSID: Guid = Guid(0xBA27_3706, 0x2FFA, 0x4FAD, [0xA2, 0x37, 0xB2, 0xBE, 0x6C, 0xA9, 0x28, 0x23]);
const IID_IUNKNOWN: Guid = Guid(0, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
pub const IID_ICLASSFACTORY: Guid = Guid(1, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);

type Hresult = i32;
const S_OK: Hresult = 0;
const S_FALSE: Hresult = 1;
const E_NOINTERFACE: Hresult = 0x8000_4002u32 as i32;
const E_POINTER: Hresult = 0x8000_4003u32 as i32;
const CLASS_E_NOAGGREGATION: Hresult = 0x8004_0110u32 as i32;
const CLASS_E_CLASSNOTAVAILABLE: Hresult = 0x8004_0111u32 as i32;

type AsioError = i32;
pub const ASE_OK: AsioError = 0;
const ASE_NOT_PRESENT: AsioError = -1000;
const ASE_INVALID_PARAMETER: AsioError = -998;
const ASE_INVALID_MODE: AsioError = -997;
const ASE_NO_CLOCK: AsioError = -995;
const ASE_NO_MEMORY: AsioError = -994;
/// ASIOSTFloat32LSB: amostras float de 32 bits.
const SAMPLE_FLOAT32: i32 = 19;

#[repr(C)]
pub struct AsioBufferInfo {
    pub is_input: i32,
    pub channel_num: i32,
    pub buffers: [*mut c_void; 2],
}

#[repr(C)]
pub struct AsioChannelInfo {
    pub channel: i32,
    pub is_input: i32,
    pub is_active: i32,
    pub channel_group: i32,
    pub sample_type: i32,
    pub name: [u8; 32],
}

#[repr(C)]
pub struct AsioClockSource {
    index: i32,
    associated_channel: i32,
    associated_group: i32,
    is_current_source: i32,
    name: [u8; 32],
}

/// Inteiro de 64 bits no formato do ASIO (metade alta primeiro).
#[repr(C)]
pub struct Asio64 {
    hi: u32,
    lo: u32,
}

#[repr(C)]
pub struct AsioCallbacks {
    pub buffer_switch: unsafe extern "C" fn(index: i32, direct_process: i32),
    pub sample_rate_did_change: unsafe extern "C" fn(rate: f64),
    pub asio_message: unsafe extern "C" fn(selector: i32, value: i32, message: *mut c_void, opt: *mut f64) -> i32,
    pub buffer_switch_time_info: unsafe extern "C" fn(params: *mut c_void, index: i32, direct_process: i32) -> *mut c_void,
}

/// Tabela de metodos de IASIO (IUnknown + os 21 metodos, na ordem do ASIO SDK).
#[repr(C)]
pub struct AsioVtbl {
    pub query_interface: unsafe extern "system" fn(*mut Driver, *const Guid, *mut *mut c_void) -> Hresult,
    pub add_ref: unsafe extern "system" fn(*mut Driver) -> u32,
    pub release: unsafe extern "system" fn(*mut Driver) -> u32,
    pub init: unsafe extern "system" fn(*mut Driver, *mut c_void) -> i32,
    pub get_driver_name: unsafe extern "system" fn(*mut Driver, *mut u8),
    pub get_driver_version: unsafe extern "system" fn(*mut Driver) -> i32,
    pub get_error_message: unsafe extern "system" fn(*mut Driver, *mut u8),
    pub start: unsafe extern "system" fn(*mut Driver) -> AsioError,
    pub stop: unsafe extern "system" fn(*mut Driver) -> AsioError,
    pub get_channels: unsafe extern "system" fn(*mut Driver, *mut i32, *mut i32) -> AsioError,
    pub get_latencies: unsafe extern "system" fn(*mut Driver, *mut i32, *mut i32) -> AsioError,
    pub get_buffer_size: unsafe extern "system" fn(*mut Driver, *mut i32, *mut i32, *mut i32, *mut i32) -> AsioError,
    pub can_sample_rate: unsafe extern "system" fn(*mut Driver, f64) -> AsioError,
    pub get_sample_rate: unsafe extern "system" fn(*mut Driver, *mut f64) -> AsioError,
    pub set_sample_rate: unsafe extern "system" fn(*mut Driver, f64) -> AsioError,
    pub get_clock_sources: unsafe extern "system" fn(*mut Driver, *mut AsioClockSource, *mut i32) -> AsioError,
    pub set_clock_source: unsafe extern "system" fn(*mut Driver, i32) -> AsioError,
    pub get_sample_position: unsafe extern "system" fn(*mut Driver, *mut Asio64, *mut Asio64) -> AsioError,
    pub get_channel_info: unsafe extern "system" fn(*mut Driver, *mut AsioChannelInfo) -> AsioError,
    pub create_buffers: unsafe extern "system" fn(*mut Driver, *mut AsioBufferInfo, i32, i32, *const AsioCallbacks) -> AsioError,
    pub dispose_buffers: unsafe extern "system" fn(*mut Driver) -> AsioError,
    pub control_panel: unsafe extern "system" fn(*mut Driver) -> AsioError,
    pub future: unsafe extern "system" fn(*mut Driver, i32, *mut c_void) -> AsioError,
    pub output_ready: unsafe extern "system" fn(*mut Driver) -> AsioError,
}

#[repr(C)]
pub struct Driver {
    pub vtbl: *const AsioVtbl,
    refs: AtomicU32,
}

#[repr(C)]
pub struct FactoryVtbl {
    pub query_interface: unsafe extern "system" fn(*mut Factory, *const Guid, *mut *mut c_void) -> Hresult,
    pub add_ref: unsafe extern "system" fn(*mut Factory) -> u32,
    pub release: unsafe extern "system" fn(*mut Factory) -> u32,
    pub create_instance: unsafe extern "system" fn(*mut Factory, *mut c_void, *const Guid, *mut *mut c_void) -> Hresult,
    pub lock_server: unsafe extern "system" fn(*mut Factory, i32) -> Hresult,
}

#[repr(C)]
pub struct Factory {
    pub vtbl: *const FactoryVtbl,
}
unsafe impl Sync for Factory {}

// ---------------------------------------------------------------- motor de rede (um por processo)

struct Engine {
    shared: Arc<Shared>,
    capture: Mutex<Capture>,
    playback: Mutex<Playback>,
}

static ENGINE: OnceLock<Engine> = OnceLock::new();
static STARTING: Mutex<()> = Mutex::new(());

/// Liga a rede na primeira vez. Uma falha nao fica guardada: a proxima tentativa comeca do zero
/// (por exemplo, depois de fechar o outro programa que estava com o driver aberto).
fn engine() -> Result<&'static Engine, String> {
    let _guard = STARTING.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(e) = ENGINE.get() {
        return Ok(e);
    }
    let e = start_engine().map_err(|e| e.to_string())?;
    Ok(ENGINE.get_or_init(|| e))
}

fn start_engine() -> Result<Engine, Box<dyn std::error::Error>> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
    let path = std::path::Path::new(&home).join(".aoip-asio.conf");
    let first_run = !path.exists();
    let shared = Arc::new(Shared::load(path));
    {
        let mut c = shared.config.lock().unwrap();
        c.device = DRIVER_NAME.into();
        c.rate = RATE;
        if first_run {
            c.tx_channels = 8; // um fluxo de 8 canais para comecar; o painel permite mudar
        }
    }
    *shared.devices.lock().unwrap() = vec![(DRIVER_NAME.to_string(), CHANNELS, CHANNELS)];
    shared.status.lock().unwrap().auto_name = aoip_core::state::auto_name(&aoip_engine::net::computer_name(), " Virtual");
    // A porta do painel tambem serve de trava: so um programa por vez pode usar o driver.
    // Dois programas com o driver aberto seriam dois remetentes no mesmo fluxo de rede.
    let listener = TcpListener::bind(("127.0.0.1", PANEL_PORT)).map_err(|_| {
        "O AoIP Bridge ASIO ja esta aberto em outro programa. Feche o outro programa e tente de novo."
    })?;
    let cfg = shared.config.lock().unwrap().clone();
    let started = resolve_iface(&shared, &cfg).and_then(|iface| start_network(&shared, &cfg, iface, CHANNELS, CHANNELS, RATE, 64));
    // O painel abre mesmo se a rede falhar: e nele que se corrige a placa de rede.
    ui::serve(shared.clone(), listener);
    {
        let mut st = shared.status.lock().unwrap();
        st.running = true; // a placa virtual em si funciona; o que pode faltar e a rede
        st.device = DRIVER_NAME.into();
        (st.ins, st.outs, st.rate) = (CHANNELS, CHANNELS, RATE);
    }
    match started {
        Ok((network, capture, playback)) => {
            shared.status.lock().unwrap().net_tx = network.net_tx;
            housekeeping(shared.clone(), Some(network));
            Ok(Engine { shared, capture: Mutex::new(capture), playback: Mutex::new(playback) })
        }
        Err(e) => {
            // Sem rede, o programa de audio continua com a placa (em silencio) em vez de
            // dizer "indisponivel", e o painel abre sozinho mostrando o motivo.
            shared.status.lock().unwrap().error = format!(
                "A rede nao abriu: {e} Escolha a placa de rede em Configuracao, salve, e depois feche e abra de novo o programa de audio."
            );
            housekeeping(shared.clone(), None);
            open_browser(&format!("http://127.0.0.1:{PANEL_PORT}"));
            let capture: Capture = Box::new(|_: &[f32]| {});
            let playback: Playback = Box::new(|out: &mut [f32]| out.fill(0.0));
            Ok(Engine { shared, capture: Mutex::new(capture), playback: Mutex::new(playback) })
        }
    }
}

/// Mantem o painel atualizado. Mudancas de configuracao so valem na proxima vez que o
/// programa de audio abrir, porque a rede nao pode ser religada com o audio rodando.
fn housekeeping(shared: Arc<Shared>, network: Option<Network>) {
    std::thread::spawn(move || {
        let mut ticks = 0u32;
        loop {
            std::thread::sleep(Duration::from_millis(250));
            ticks += 1;
            match &network {
                Some(n) => {
                    refresh_status(&shared, n);
                    if selftest() && ticks % 20 == 0 {
                        report_stats(&shared, n);
                    }
                }
                None if ticks % 20 == 0 => refresh_ifaces(&shared),
                None => {}
            }
            if shared.restart.swap(false, Relaxed) {
                shared.status.lock().unwrap().error =
                    "Configuracao salva. Para valer, feche e abra de novo o programa de audio (REAPER etc.).".into();
            }
            shared.quit.store(false, Relaxed); // quem encerra o driver e o programa de audio
        }
    });
}

// ---------------------------------------------------------------- estado do driver

type Halves = [Box<[f32]>; 2];

#[derive(Default)]
struct State {
    buffer_frames: usize,
    ins: Vec<Option<Halves>>,
    outs: Vec<Option<Halves>>,
    callbacks: usize, // *const AsioCallbacks
    error: String,
    thread: Option<std::thread::JoinHandle<()>>,
}

static STATE: Mutex<State> = Mutex::new(State {
    buffer_frames: 0, ins: Vec::new(), outs: Vec::new(), callbacks: 0, error: String::new(), thread: None,
});
static RUNNING: AtomicBool = AtomicBool::new(false);
static POSITION: AtomicU64 = AtomicU64::new(0);
static STAMP_NS: AtomicU64 = AtomicU64::new(0);

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

fn system_ns() -> u64 {
    #[cfg(windows)]
    {
        #[link(name = "winmm")]
        extern "system" {
            fn timeGetTime() -> u32;
        }
        // O ASIO usa o relogio multimidia do Windows, em nanossegundos.
        return unsafe { timeGetTime() } as u64 * 1_000_000;
    }
    #[cfg(not(windows))]
    {
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_nanos() as u64
    }
}

fn copy_name(dst: *mut u8, cap: usize, text: &str) {
    let bytes = text.as_bytes();
    let n = bytes.len().min(cap - 1);
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, n);
        *dst.add(n) = 0;
    }
}

fn stop_audio() {
    RUNNING.store(false, Relaxed);
    let handle = state().thread.take();
    if let Some(h) = handle {
        if h.thread().id() != std::thread::current().id() {
            let _ = h.join();
        }
    }
}

/// A "placa": a cada bloco entrega ao programa o que chegou da rede, chama o programa e
/// manda para a rede o que ele tocou.
fn audio_thread(frames: usize, ins: Vec<Option<[usize; 2]>>, outs: Vec<Option<[usize; 2]>>, callbacks: usize) {
    let Ok(engine) = engine() else { return };
    let callbacks = callbacks as *const AsioCallbacks;
    let period = Duration::from_nanos(frames as u64 * 1_000_000_000 / RATE as u64);
    realtime_thread_for(period, period / 2);
    let mut from_net = vec![0f32; frames * CHANNELS];
    let mut to_net = vec![0f32; frames * CHANNELS];
    let (mut index, mut next) = (0usize, Instant::now() + period);
    POSITION.store(0, Relaxed);
    while RUNNING.load(Relaxed) {
        // rede -> entradas do programa
        (engine.playback.lock().unwrap_or_else(|p| p.into_inner()))(&mut from_net);
        for (c, halves) in ins.iter().enumerate() {
            if let Some(h) = halves {
                let buf = unsafe { std::slice::from_raw_parts_mut(h[index] as *mut f32, frames) };
                for (f, s) in buf.iter_mut().enumerate() {
                    *s = from_net[f * CHANNELS + c];
                }
            }
        }
        STAMP_NS.store(system_ns(), Relaxed);
        unsafe { ((*callbacks).buffer_switch)(index as i32, 0) };
        // saidas do programa -> rede
        to_net.fill(0.0);
        for (c, halves) in outs.iter().enumerate() {
            if let Some(h) = halves {
                let buf = unsafe { std::slice::from_raw_parts(h[index] as *const f32, frames) };
                for (f, s) in buf.iter().enumerate() {
                    to_net[f * CHANNELS + c] = *s;
                }
            }
        }
        (engine.capture.lock().unwrap_or_else(|p| p.into_inner()))(&to_net);
        POSITION.fetch_add(frames as u64, Relaxed);
        index ^= 1;

        // espera o proximo bloco: dorme quase tudo e gira no finzinho, para acordar na hora
        let now = Instant::now();
        if now > next + period * 8 {
            next = now; // ficou muito para tras (computador suspenso etc.): recomeca daqui
        }
        if next > now {
            let wait = next - now;
            if wait > Duration::from_micros(1500) {
                std::thread::sleep(wait - Duration::from_micros(1000));
            }
            // Girar (e nao ceder a vez): ceder a vez aqui causava pausas de ate 15 ms.
            while Instant::now() < next {
                std::hint::spin_loop();
            }
        }
        next += period;
    }
}

// ---------------------------------------------------------------- metodos de IASIO

unsafe extern "system" fn query_interface(this: *mut Driver, iid: *const Guid, out: *mut *mut c_void) -> Hresult {
    if out.is_null() {
        return E_POINTER;
    }
    // Os programas ASIO pedem a interface usando o proprio CLSID do driver.
    if iid.is_null() || *iid == IID_IUNKNOWN || *iid == CLSID {
        (*this).refs.fetch_add(1, Relaxed);
        *out = this as *mut c_void;
        return S_OK;
    }
    *out = std::ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "system" fn add_ref(this: *mut Driver) -> u32 {
    (*this).refs.fetch_add(1, Relaxed) + 1
}

unsafe extern "system" fn release(this: *mut Driver) -> u32 {
    let left = (*this).refs.fetch_sub(1, Relaxed) - 1;
    if left == 0 {
        dispose_buffers(this);
        drop(Box::from_raw(this));
    }
    left
}

unsafe extern "system" fn init(_this: *mut Driver, _sys_handle: *mut c_void) -> i32 {
    // A rede so liga em createBuffers. Programas que abrem o driver so para perguntar quantos
    // canais ele tem (a lista de placas do proprio AoIP Bridge, por exemplo) nao devem colocar
    // um segundo remetente, mudo, no mesmo fluxo.
    1
}

fn ensure_engine() -> bool {
    let result = std::panic::catch_unwind(engine).unwrap_or_else(|_| Err("erro interno ao iniciar a rede".into()));
    match result {
        Ok(_) => true,
        Err(e) => {
            state().error = if e.is_empty() { "Nao consegui iniciar a rede.".into() } else { e };
            false
        }
    }
}

unsafe extern "system" fn get_driver_name(_this: *mut Driver, name: *mut u8) {
    copy_name(name, 32, DRIVER_NAME);
}

unsafe extern "system" fn get_driver_version(_this: *mut Driver) -> i32 {
    2
}

unsafe extern "system" fn get_error_message(_this: *mut Driver, text: *mut u8) {
    copy_name(text, 124, &state().error);
}

unsafe extern "system" fn start(_this: *mut Driver) -> AsioError {
    if RUNNING.load(Relaxed) {
        return ASE_OK;
    }
    let mut st = state();
    if st.callbacks == 0 || st.buffer_frames == 0 {
        return ASE_INVALID_MODE;
    }
    let ptrs = |v: &Vec<Option<Halves>>| -> Vec<Option<[usize; 2]>> {
        v.iter().map(|h| h.as_ref().map(|h| [h[0].as_ptr() as usize, h[1].as_ptr() as usize])).collect()
    };
    let (frames, ins, outs, callbacks) = (st.buffer_frames, ptrs(&st.ins), ptrs(&st.outs), st.callbacks);
    RUNNING.store(true, Relaxed);
    st.thread = Some(std::thread::spawn(move || audio_thread(frames, ins, outs, callbacks)));
    ASE_OK
}

unsafe extern "system" fn stop(_this: *mut Driver) -> AsioError {
    stop_audio();
    ASE_OK
}

unsafe extern "system" fn get_channels(_this: *mut Driver, ins: *mut i32, outs: *mut i32) -> AsioError {
    *ins = CHANNELS as i32;
    *outs = CHANNELS as i32;
    ASE_OK
}

unsafe extern "system" fn get_latencies(_this: *mut Driver, input: *mut i32, output: *mut i32) -> AsioError {
    // um bloco do programa + a fila de rede (folga configurada + um bloco)
    let frames = state().buffer_frames.max(BUF_PREFERRED as usize) as i32;
    let running = ENGINE.get();
    let extra = running.map(|e| e.shared.config.lock().unwrap().latency_ms as i32 * RATE as i32 / 1000).unwrap_or(192);
    *input = frames * 2 + extra;
    *output = frames * 2 + extra;
    ASE_OK
}

unsafe extern "system" fn get_buffer_size(_this: *mut Driver, min: *mut i32, max: *mut i32, preferred: *mut i32, granularity: *mut i32) -> AsioError {
    *min = BUF_MIN;
    *max = BUF_MAX;
    *preferred = BUF_PREFERRED;
    *granularity = -1; // potencias de 2
    ASE_OK
}

unsafe extern "system" fn can_sample_rate(_this: *mut Driver, rate: f64) -> AsioError {
    if rate == RATE as f64 { ASE_OK } else { ASE_NO_CLOCK }
}

unsafe extern "system" fn get_sample_rate(_this: *mut Driver, rate: *mut f64) -> AsioError {
    *rate = RATE as f64;
    ASE_OK
}

unsafe extern "system" fn set_sample_rate(this: *mut Driver, rate: f64) -> AsioError {
    can_sample_rate(this, rate)
}

unsafe extern "system" fn get_clock_sources(_this: *mut Driver, clocks: *mut AsioClockSource, count: *mut i32) -> AsioError {
    if clocks.is_null() || count.is_null() || *count < 1 {
        return ASE_INVALID_PARAMETER;
    }
    let c = &mut *clocks;
    (c.index, c.associated_channel, c.associated_group, c.is_current_source) = (0, -1, -1, 1);
    copy_name(c.name.as_mut_ptr(), 32, "Rede (PTP)");
    *count = 1;
    ASE_OK
}

unsafe extern "system" fn set_clock_source(_this: *mut Driver, index: i32) -> AsioError {
    if index == 0 { ASE_OK } else { ASE_INVALID_PARAMETER }
}

unsafe extern "system" fn get_sample_position(_this: *mut Driver, pos: *mut Asio64, stamp: *mut Asio64) -> AsioError {
    let (p, t) = (POSITION.load(Relaxed), STAMP_NS.load(Relaxed));
    *pos = Asio64 { hi: (p >> 32) as u32, lo: p as u32 };
    *stamp = Asio64 { hi: (t >> 32) as u32, lo: t as u32 };
    ASE_OK
}

unsafe extern "system" fn get_channel_info(_this: *mut Driver, info: *mut AsioChannelInfo) -> AsioError {
    let i = &mut *info;
    if i.channel < 0 || i.channel as usize >= CHANNELS {
        return ASE_INVALID_PARAMETER;
    }
    let st = state();
    let list = if i.is_input != 0 { &st.ins } else { &st.outs };
    i.is_active = list.get(i.channel as usize).is_some_and(|h| h.is_some()) as i32;
    i.channel_group = 0;
    i.sample_type = SAMPLE_FLOAT32;
    let name = format!("{} {}", if i.is_input != 0 { "Rede Entrada" } else { "Rede Saida" }, i.channel + 1);
    copy_name(i.name.as_mut_ptr(), 32, &name);
    ASE_OK
}

unsafe extern "system" fn create_buffers(this: *mut Driver, infos: *mut AsioBufferInfo, count: i32, frames: i32, callbacks: *const AsioCallbacks) -> AsioError {
    if infos.is_null() || callbacks.is_null() || count < 1 || !(BUF_MIN..=BUF_MAX).contains(&frames) {
        return ASE_INVALID_PARAMETER;
    }
    dispose_buffers(this);
    if !ensure_engine() {
        return ASE_NOT_PRESENT;
    }
    let infos = std::slice::from_raw_parts_mut(infos, count as usize);
    if infos.iter().any(|i| i.channel_num < 0 || i.channel_num as usize >= CHANNELS) {
        return ASE_INVALID_PARAMETER;
    }
    let made = std::panic::catch_unwind(|| vec![0f32; frames as usize].into_boxed_slice());
    if made.is_err() {
        return ASE_NO_MEMORY;
    }
    let mut st = state();
    st.ins = (0..CHANNELS).map(|_| None).collect();
    st.outs = (0..CHANNELS).map(|_| None).collect();
    for info in infos.iter_mut() {
        let mut halves: Halves = [vec![0f32; frames as usize].into_boxed_slice(), vec![0f32; frames as usize].into_boxed_slice()];
        info.buffers = [halves[0].as_mut_ptr() as *mut c_void, halves[1].as_mut_ptr() as *mut c_void];
        let list = if info.is_input != 0 { &mut st.ins } else { &mut st.outs };
        list[info.channel_num as usize] = Some(halves);
    }
    st.buffer_frames = frames as usize;
    st.callbacks = callbacks as usize;
    ASE_OK
}

unsafe extern "system" fn dispose_buffers(_this: *mut Driver) -> AsioError {
    stop_audio();
    let mut st = state();
    st.ins.clear();
    st.outs.clear();
    st.buffer_frames = 0;
    st.callbacks = 0;
    ASE_OK
}

unsafe extern "system" fn control_panel(_this: *mut Driver) -> AsioError {
    open_browser(&format!("http://127.0.0.1:{PANEL_PORT}"));
    ASE_OK
}

unsafe extern "system" fn future(_this: *mut Driver, _selector: i32, _opt: *mut c_void) -> AsioError {
    ASE_INVALID_PARAMETER
}

unsafe extern "system" fn output_ready(_this: *mut Driver) -> AsioError {
    ASE_NOT_PRESENT
}

static VTBL: AsioVtbl = AsioVtbl {
    query_interface, add_ref, release, init, get_driver_name, get_driver_version, get_error_message, start, stop,
    get_channels, get_latencies, get_buffer_size, can_sample_rate, get_sample_rate, set_sample_rate, get_clock_sources,
    set_clock_source, get_sample_position, get_channel_info, create_buffers, dispose_buffers, control_panel, future, output_ready,
};

// ---------------------------------------------------------------- fabrica COM e entradas da DLL

unsafe extern "system" fn factory_query(this: *mut Factory, iid: *const Guid, out: *mut *mut c_void) -> Hresult {
    if out.is_null() {
        return E_POINTER;
    }
    if iid.is_null() || *iid == IID_IUNKNOWN || *iid == IID_ICLASSFACTORY {
        *out = this as *mut c_void;
        return S_OK;
    }
    *out = std::ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "system" fn factory_ref(_this: *mut Factory) -> u32 {
    1 // a fabrica e estatica
}

unsafe extern "system" fn factory_create(_this: *mut Factory, outer: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> Hresult {
    if out.is_null() {
        return E_POINTER;
    }
    *out = std::ptr::null_mut();
    if !outer.is_null() {
        return CLASS_E_NOAGGREGATION;
    }
    let driver = Box::into_raw(Box::new(Driver { vtbl: &VTBL, refs: AtomicU32::new(0) }));
    let hr = query_interface(driver, iid, out);
    if hr != S_OK {
        drop(Box::from_raw(driver));
    }
    hr
}

unsafe extern "system" fn factory_lock(_this: *mut Factory, _lock: i32) -> Hresult {
    S_OK
}

static FACTORY_VTBL: FactoryVtbl = FactoryVtbl {
    query_interface: factory_query, add_ref: factory_ref, release: factory_ref, create_instance: factory_create, lock_server: factory_lock,
};
static FACTORY: Factory = Factory { vtbl: &FACTORY_VTBL };

/// As threads de rede vivem ate o programa de audio fechar, entao a DLL nunca pode ser
/// descarregada antes disso: fica presa ao processo.
fn pin_module() {
    #[cfg(windows)]
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetModuleHandleExW(flags: u32, address: *const c_void, module: *mut *mut c_void) -> i32;
        }
        const PIN_FROM_ADDRESS: u32 = 0x1 | 0x4;
        let mut module = std::ptr::null_mut();
        GetModuleHandleExW(PIN_FROM_ADDRESS, pin_module as *const c_void, &mut module);
    }
}

#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(clsid: *const Guid, iid: *const Guid, out: *mut *mut c_void) -> Hresult {
    if out.is_null() {
        return E_POINTER;
    }
    if clsid.is_null() || *clsid != CLSID {
        *out = std::ptr::null_mut();
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    pin_module();
    factory_query(&FACTORY as *const Factory as *mut Factory, iid, out)
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> Hresult {
    S_FALSE
}
