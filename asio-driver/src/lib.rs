//! Driver ASIO virtual "AoIP Bridge ASIO" (Windows, 64 bits).
//!
//! O programa de audio (REAPER etc.) carrega esta DLL como uma placa de 2 a 64 canais, a
//! 44,1 ou 48 kHz. A rede NAO fica aqui: fica no programa AOIP (aoip-winvirtual.exe), que
//! esta sempre ligado, com o icone perto do relogio e o painel em http://127.0.0.1:8788. Este
//! driver so leva o audio entre o programa de audio e ele (veja `aoip_engine::local`), e
//! abre o programa AOIP se ele nao estiver aberto.
//!
//! O codigo compila em qualquer sistema para que `examples/host_test.rs` possa exercitar o
//! driver como um programa de audio faria; so o registro e o carregamento sao do Windows.
#![allow(non_snake_case, clippy::missing_safety_doc)]

use aoip_engine::local::{self, CardClient, CardInfo};
use aoip_engine::{open_browser, realtime_thread_for};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const DRIVER_NAME: &str = "AoIP Bridge ASIO";
pub const PANEL_PORT: u16 = 8788;
/// Nome do programa AOIP, na mesma pasta desta DLL.
pub const HELPER_EXE: &str = "aoip-winvirtual.exe";
const BUF_MIN: i32 = 32;
const BUF_MAX: i32 = 2048;
/// Pedido ao programa de audio para reabrir a placa (formato mudou).
const K_ASIO_SELECTOR_SUPPORTED: i32 = 1;
const K_ASIO_RESET_REQUEST: i32 = 3;

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

// ---------------------------------------------------------------- ligacao com o programa AOIP

/// Formato atual da placa (vem do programa AOIP; 32x32 a 48 kHz ate a primeira resposta).
static INFO: Mutex<CardInfo> = Mutex::new(CardInfo { channels: 32, rate: 48_000, buffer: 128, latency: 192, generation: 0 });

fn info() -> CardInfo {
    *INFO.lock().unwrap_or_else(|p| p.into_inner())
}

/// Pasta desta DLL (onde tambem fica o programa AOIP).
fn dll_dir() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetModuleHandleExW(flags: u32, address: *const c_void, module: *mut *mut c_void) -> i32;
            fn GetModuleFileNameW(module: *mut c_void, name: *mut u16, size: u32) -> u32;
        }
        const FROM_ADDRESS_NO_REF: u32 = 0x4 | 0x2;
        let mut module = std::ptr::null_mut();
        if GetModuleHandleExW(FROM_ADDRESS_NO_REF, dll_dir as *const c_void, &mut module) == 0 {
            return None;
        }
        let mut buf = vec![0u16; 1024];
        let n = GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) as usize;
        let path = std::path::PathBuf::from(String::from_utf16_lossy(&buf[..n]));
        return path.parent().map(|p| p.to_path_buf());
    }
    #[cfg(not(windows))]
    std::env::var_os("AOIP_HELPER_DIR").map(std::path::PathBuf::from)
}

/// Garante o programa AOIP aberto (abre se preciso) e le o formato da placa.
fn connect_helper() -> Result<CardInfo, String> {
    if let Ok(i) = local::query() {
        *INFO.lock().unwrap_or_else(|p| p.into_inner()) = i;
        return Ok(i);
    }
    static LAUNCHED: Mutex<Option<Instant>> = Mutex::new(None);
    let mut launched = LAUNCHED.lock().unwrap_or_else(|p| p.into_inner());
    if launched.map_or(true, |t| t.elapsed() > Duration::from_secs(10)) {
        if let Some(exe) = dll_dir().map(|d| d.join(HELPER_EXE)).filter(|p| p.exists()) {
            let _ = std::process::Command::new(exe).arg("--tray").spawn();
            *launched = Some(Instant::now());
        }
    }
    drop(launched);
    for _ in 0..25 {
        std::thread::sleep(Duration::from_millis(200));
        if let Ok(i) = local::query() {
            *INFO.lock().unwrap_or_else(|p| p.into_inner()) = i;
            return Ok(i);
        }
    }
    Err("O programa AoIP Virtual nao esta aberto. Abra \"AoIP Virtual\" no menu Iniciar (ou reinstale) e tente de novo.".into())
}

// ---------------------------------------------------------------- estado do driver

type Halves = [Box<[f32]>; 2];

#[derive(Default)]
struct State {
    buffer_frames: usize,
    channels: usize,
    ins: Vec<Option<Halves>>,
    outs: Vec<Option<Halves>>,
    callbacks: usize, // *const AsioCallbacks
    error: String,
    client: Option<CardClient>,
    thread: Option<std::thread::JoinHandle<()>>,
}

static STATE: Mutex<State> = Mutex::new(State {
    buffer_frames: 0, channels: 0, ins: Vec::new(), outs: Vec::new(), callbacks: 0, error: String::new(), client: None, thread: None,
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

/// Pede ao programa de audio para reabrir a placa (o formato mudou no programa AOIP).
unsafe fn request_reset(callbacks: *const AsioCallbacks) {
    let cb = &*callbacks;
    if (cb.asio_message)(K_ASIO_SELECTOR_SUPPORTED, K_ASIO_RESET_REQUEST, std::ptr::null_mut(), std::ptr::null_mut()) != 0 {
        (cb.asio_message)(K_ASIO_RESET_REQUEST, 0, std::ptr::null_mut(), std::ptr::null_mut());
    }
}

/// A "placa": a cada bloco troca com o programa AOIP (vai o que o programa de audio tocou,
/// volta o que chegou da rede) e chama o programa de audio. O ritmo e deste relogio.
fn audio_thread(frames: usize, ch: usize, ins: Vec<Option<[usize; 2]>>, outs: Vec<Option<[usize; 2]>>, callbacks: usize, mut client: Option<CardClient>) {
    let callbacks = callbacks as *const AsioCallbacks;
    let rate = info().rate.max(8000);
    let period = Duration::from_nanos(frames as u64 * 1_000_000_000 / rate as u64);
    realtime_thread_for(period, period / 2);
    let mut from_net = vec![0f32; frames * ch];
    let mut to_net = vec![0f32; frames * ch];
    let generation = client.as_ref().map(|c| c.info.generation).unwrap_or(0);
    let (mut index, mut next, mut last_try, mut reset_sent) = (0usize, Instant::now() + period, Instant::now(), false);
    POSITION.store(0, Relaxed);
    while RUNNING.load(Relaxed) {
        // programa AOIP: manda o bloco anterior, recebe o da rede
        from_net.fill(0.0);
        if let Some(c) = client.as_mut() {
            match c.exchange(&to_net, &mut from_net, ch) {
                Ok(g) if g != generation && !reset_sent => {
                    reset_sent = true;
                    unsafe { request_reset(callbacks) };
                }
                Ok(_) => {}
                Err(_) => {
                    client = None; // caiu: segue em silencio e tenta de novo
                    state().error = "Perdi a ligacao com o programa AoIP Virtual; tentando de novo.".into();
                }
            }
        } else if last_try.elapsed() > Duration::from_secs(1) {
            last_try = Instant::now();
            if let Ok(c) = CardClient::connect() {
                if c.info.channels != ch || c.info.rate != rate {
                    if !reset_sent {
                        reset_sent = true;
                        unsafe { request_reset(callbacks) };
                    }
                } else {
                    state().error.clear();
                    client = Some(c);
                }
            }
        }
        for (c, halves) in ins.iter().enumerate() {
            if let Some(h) = halves {
                let buf = unsafe { std::slice::from_raw_parts_mut(h[index] as *mut f32, frames) };
                for (f, s) in buf.iter_mut().enumerate() {
                    *s = from_net[f * ch + c];
                }
            }
        }
        STAMP_NS.store(system_ns(), Relaxed);
        unsafe { ((*callbacks).buffer_switch)(index as i32, 0) };
        to_net.fill(0.0);
        for (c, halves) in outs.iter().enumerate() {
            if let Some(h) = halves {
                let buf = unsafe { std::slice::from_raw_parts(h[index] as *const f32, frames) };
                for (f, s) in buf.iter().enumerate() {
                    to_net[f * ch + c] = *s;
                }
            }
        }
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
    // Abre o programa AOIP se preciso e le o formato. Mesmo sem ele a placa aparece (a
    // ligacao e tentada de novo ao abrir o audio).
    if let Err(e) = connect_helper() {
        state().error = e;
    }
    1
}

unsafe extern "system" fn get_driver_name(_this: *mut Driver, name: *mut u8) {
    copy_name(name, 32, DRIVER_NAME);
}

unsafe extern "system" fn get_driver_version(_this: *mut Driver) -> i32 {
    3
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
    let (frames, ch, ins, outs, callbacks, client) = (st.buffer_frames, st.channels, ptrs(&st.ins), ptrs(&st.outs), st.callbacks, st.client.take());
    RUNNING.store(true, Relaxed);
    st.thread = Some(std::thread::spawn(move || audio_thread(frames, ch, ins, outs, callbacks, client)));
    ASE_OK
}

unsafe extern "system" fn stop(_this: *mut Driver) -> AsioError {
    stop_audio();
    ASE_OK
}

unsafe extern "system" fn get_channels(_this: *mut Driver, ins: *mut i32, outs: *mut i32) -> AsioError {
    let ch = info().channels as i32;
    *ins = ch;
    *outs = ch;
    ASE_OK
}

unsafe extern "system" fn get_latencies(_this: *mut Driver, input: *mut i32, output: *mut i32) -> AsioError {
    // um bloco do programa + um bloco da ligacao local + a folga da rede
    let i = info();
    let frames = state().buffer_frames.max(i.buffer) as i32;
    *input = frames * 2 + i.latency as i32;
    *output = frames * 2 + i.latency as i32;
    ASE_OK
}

unsafe extern "system" fn get_buffer_size(_this: *mut Driver, min: *mut i32, max: *mut i32, preferred: *mut i32, granularity: *mut i32) -> AsioError {
    *min = BUF_MIN;
    *max = BUF_MAX;
    *preferred = info().buffer as i32;
    *granularity = -1; // potencias de 2
    ASE_OK
}

unsafe extern "system" fn can_sample_rate(_this: *mut Driver, rate: f64) -> AsioError {
    if rate == 44_100.0 || rate == 48_000.0 { ASE_OK } else { ASE_NO_CLOCK }
}

unsafe extern "system" fn get_sample_rate(_this: *mut Driver, rate: *mut f64) -> AsioError {
    *rate = info().rate as f64;
    ASE_OK
}

unsafe extern "system" fn set_sample_rate(this: *mut Driver, rate: f64) -> AsioError {
    if can_sample_rate(this, rate) != ASE_OK {
        return ASE_NO_CLOCK;
    }
    if rate as u32 == info().rate {
        return ASE_OK;
    }
    // O programa de audio pediu outra taxa: o programa AOIP religa a rede nela.
    match local::set_rate(rate as u32) {
        Ok(i) => {
            *INFO.lock().unwrap_or_else(|p| p.into_inner()) = i;
            ASE_OK
        }
        Err(_) => ASE_NO_CLOCK,
    }
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

unsafe extern "system" fn get_channel_info(_this: *mut Driver, info_out: *mut AsioChannelInfo) -> AsioError {
    let i = &mut *info_out;
    if i.channel < 0 || i.channel as usize >= info().channels {
        return ASE_INVALID_PARAMETER;
    }
    let st = state();
    let list = if i.is_input != 0 { &st.ins } else { &st.outs };
    i.is_active = list.get(i.channel as usize).is_some_and(|h| h.is_some()) as i32;
    i.channel_group = 0;
    i.sample_type = SAMPLE_FLOAT32;
    let name = format!("{} {}", if i.is_input != 0 { "AoIP Entrada" } else { "AoIP Saida" }, i.channel + 1);
    copy_name(i.name.as_mut_ptr(), 32, &name);
    ASE_OK
}

unsafe extern "system" fn create_buffers(this: *mut Driver, infos: *mut AsioBufferInfo, count: i32, frames: i32, callbacks: *const AsioCallbacks) -> AsioError {
    if infos.is_null() || callbacks.is_null() || count < 1 || !(BUF_MIN..=BUF_MAX).contains(&frames) {
        return ASE_INVALID_PARAMETER;
    }
    dispose_buffers(this);
    // Ocupa a placa no programa AOIP. Ocupada por outro programa de audio: "indisponivel".
    // Programa AOIP fechado: abre a placa mesmo assim (silencio) e liga quando ele abrir.
    let _ = connect_helper();
    let client = match CardClient::connect() {
        Ok(c) => Some(c),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            state().error = "A placa AoIP ja esta aberta em outro programa de audio. Feche o outro programa e tente de novo.".into();
            return ASE_NOT_PRESENT;
        }
        Err(_) => None,
    };
    let ch = client.as_ref().map(|c| c.info.channels).unwrap_or_else(|| info().channels);
    if let Some(c) = &client {
        *INFO.lock().unwrap_or_else(|p| p.into_inner()) = c.info;
    }
    let infos = std::slice::from_raw_parts_mut(infos, count as usize);
    if infos.iter().any(|i| i.channel_num < 0 || i.channel_num as usize >= ch) {
        return ASE_INVALID_PARAMETER;
    }
    let made = std::panic::catch_unwind(|| vec![0f32; frames as usize].into_boxed_slice());
    if made.is_err() {
        return ASE_NO_MEMORY;
    }
    let mut st = state();
    st.ins = (0..ch).map(|_| None).collect();
    st.outs = (0..ch).map(|_| None).collect();
    for info in infos.iter_mut() {
        let mut halves: Halves = [vec![0f32; frames as usize].into_boxed_slice(), vec![0f32; frames as usize].into_boxed_slice()];
        info.buffers = [halves[0].as_mut_ptr() as *mut c_void, halves[1].as_mut_ptr() as *mut c_void];
        let list = if info.is_input != 0 { &mut st.ins } else { &mut st.outs };
        list[info.channel_num as usize] = Some(halves);
    }
    st.buffer_frames = frames as usize;
    st.channels = ch;
    st.callbacks = callbacks as usize;
    st.client = client;
    if st.client.is_some() {
        st.error.clear();
    }
    ASE_OK
}

unsafe extern "system" fn dispose_buffers(_this: *mut Driver) -> AsioError {
    stop_audio();
    let mut st = state();
    st.ins.clear();
    st.outs.clear();
    st.buffer_frames = 0;
    st.callbacks = 0;
    st.client = None; // solta a placa no programa AOIP
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
