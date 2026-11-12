//! Teste de ida e volta com outro computador. Faz o papel de um programa de audio no driver:
//!   saida 1: rajadas de 1 kHz (0,1 s a cada segundo, nivel 0,2)  -> servem para medir o atraso
//!   saida 2: tom continuo de 440 Hz (nivel 0,1)                  -> serve para contar falhas
//! e espera que o outro computador devolva essas duas saidas nos canais 3 e 4 do fluxo dele
//! (os canais 1 e 2 do fluxo dele ficam livres, por exemplo para uma musica).
//! Uso: HOME=<pasta com .aoip-asio.conf: iface, name=Mac, rx0=ASIO : 1> loop_test <segundos>
use aoip_asio::*;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed};

const FRAMES: usize = 256;
static CALLS: AtomicU64 = AtomicU64::new(0);
static BUFS: [AtomicUsize; 12] = [const { AtomicUsize::new(0) }; 12]; // in1..in4, out1, out2 (duas metades cada)
static PEAK: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];
static PHASE: AtomicU64 = AtomicU64::new(0);
static QUIET: AtomicU64 = AtomicU64::new(0); // quadros seguidos de silencio na entrada 3
static DELAY: AtomicU64 = AtomicU64::new(u64::MAX); // ultimo atraso medido, em quadros
static SEEN: AtomicU64 = AtomicU64::new(0); // blocos com o tom continuo presente na entrada 4
static DROPS: AtomicU64 = AtomicU64::new(0); // blocos em que ele sumiu depois de ter aparecido

unsafe extern "C" fn buffer_switch(index: i32, _direct: i32) {
    let i = index as usize;
    CALLS.fetch_add(1, Relaxed);
    let start = PHASE.fetch_add(FRAMES as u64, Relaxed);
    for ch in 0..4 {
        let buf = std::slice::from_raw_parts(BUFS[ch * 2 + i].load(Relaxed) as *const f32, FRAMES);
        let peak = buf.iter().fold(0f32, |m, s| m.max(s.abs()));
        PEAK[ch].fetch_max(peak.to_bits(), Relaxed);
        if ch == 2 {
            // inicio da rajada: primeiro quadro alto depois de pelo menos 0,5 s de silencio
            let mut quiet = QUIET.load(Relaxed);
            for (f, s) in buf.iter().enumerate() {
                if s.abs() > 0.05 {
                    if quiet > 24_000 {
                        DELAY.store((start + f as u64) % 48_000, Relaxed);
                    }
                    quiet = 0;
                } else {
                    quiet += 1;
                }
            }
            QUIET.store(quiet, Relaxed);
        }
        if ch == 3 {
            if peak > 0.05 {
                SEEN.fetch_add(1, Relaxed);
            } else if SEEN.load(Relaxed) > 200 {
                DROPS.fetch_add(1, Relaxed);
            }
        }
    }
    let tone = |n: u64, hz: f32| (((n % 48_000) as f32) * std::f32::consts::TAU * hz / 48_000.0).sin();
    let out = |k: usize| std::slice::from_raw_parts_mut(BUFS[8 + k * 2 + i].load(Relaxed) as *mut f32, FRAMES);
    for (f, s) in out(0).iter_mut().enumerate() {
        let n = start + f as u64;
        *s = if n % 48_000 < 4_800 { 0.2 * tone(n, 1000.0) } else { 0.0 };
    }
    for (f, s) in out(1).iter_mut().enumerate() {
        *s = 0.1 * tone(start + f as u64, 440.0);
    }
}
unsafe extern "C" fn rate_changed(_rate: f64) {}
unsafe extern "C" fn message(_selector: i32, _value: i32, _message: *mut c_void, _opt: *mut f64) -> i32 { 0 }
unsafe extern "C" fn switch_time_info(_p: *mut c_void, _index: i32, _direct: i32) -> *mut c_void { std::ptr::null_mut() }

fn main() {
    unsafe {
        let mut factory: *mut c_void = std::ptr::null_mut();
        assert_eq!(DllGetClassObject(&CLSID, &IID_ICLASSFACTORY, &mut factory), 0, "fabrica");
        let factory = factory as *mut Factory;
        let mut raw: *mut c_void = std::ptr::null_mut();
        assert_eq!(((*(*factory).vtbl).create_instance)(factory, std::ptr::null_mut(), &CLSID, &mut raw), 0, "criar driver");
        let d = raw as *mut Driver;
        let v = &*(*d).vtbl;

        let mut name = [0u8; 32];
        (v.get_driver_name)(d, name.as_mut_ptr());
        println!("driver: {}", String::from_utf8_lossy(&name).trim_end_matches('\0'));
        if (v.init)(d, std::ptr::null_mut()) != 1 {
            let mut msg = [0u8; 124];
            (v.get_error_message)(d, msg.as_mut_ptr());
            panic!("init falhou: {}", String::from_utf8_lossy(&msg));
        }
        let (mut ins, mut outs) = (0, 0);
        assert_eq!((v.get_channels)(d, &mut ins, &mut outs), ASE_OK);
        let (mut min, mut max, mut pref, mut gran) = (0, 0, 0, 0);
        assert_eq!((v.get_buffer_size)(d, &mut min, &mut max, &mut pref, &mut gran), ASE_OK);
        assert_eq!((v.can_sample_rate)(d, 48_000.0), ASE_OK);
        assert_ne!((v.can_sample_rate)(d, 44_100.0), ASE_OK);
        println!("canais {ins}x{outs}, bloco {min}..{max} (preferido {pref})");

        let mut infos = [(1, 0), (1, 1), (1, 2), (1, 3), (0, 0), (0, 1)].map(|(is_input, channel_num)| AsioBufferInfo { is_input, channel_num, buffers: [std::ptr::null_mut(); 2] });
        let callbacks = AsioCallbacks { buffer_switch, sample_rate_did_change: rate_changed, asio_message: message, buffer_switch_time_info: switch_time_info };
        assert_eq!((v.create_buffers)(d, infos.as_mut_ptr(), 6, FRAMES as i32, &callbacks), ASE_OK, "createBuffers");
        for (k, info) in infos.iter().enumerate() {
            BUFS[k * 2].store(info.buffers[0] as usize, Relaxed);
            BUFS[k * 2 + 1].store(info.buffers[1] as usize, Relaxed);
        }
        let mut info = AsioChannelInfo { channel: 1, is_input: 1, is_active: 0, channel_group: 0, sample_type: 0, name: [0; 32] };
        assert_eq!((v.get_channel_info)(d, &mut info), ASE_OK);
        println!("canal: {} ativo={} tipo={}", String::from_utf8_lossy(&info.name).trim_end_matches('\0'), info.is_active, info.sample_type);

        assert_eq!((v.start)(d), ASE_OK, "start");
        let secs: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(12);
        for s in 1..=secs {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let p = |c: usize| f32::from_bits(PEAK[c].swap(0, Relaxed));
            let delay = match DELAY.swap(u64::MAX, Relaxed) {
                u64::MAX => "   -  ".to_string(),
                d => format!("{:5.1}", d as f64 / 48.0),
            };
            println!("{s:4} s | musica 1: {:.3}  2: {:.3} | retorno 3: {:.3}  4: {:.3} | ida e volta {delay} ms | falhas no retorno: {}",
                p(0), p(1), p(2), p(3), DROPS.load(Relaxed));
        }
        assert_eq!((v.stop)(d), ASE_OK);
        let calls = CALLS.load(Relaxed);
        let expected = secs * 48_000 / FRAMES as u64;
        println!("blocos: {calls} (esperado ~{expected})");
        assert_eq!((v.dispose_buffers)(d), ASE_OK);
        assert_eq!((v.release)(d), 0, "release libera o driver");
        assert!(calls.abs_diff(expected) <= expected / 50 + 2, "ritmo dos blocos fora do esperado");
        println!("OK");
    }
}
