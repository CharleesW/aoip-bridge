//! Faz o papel de um programa de audio (REAPER etc.) e exercita o driver pela mesma porta
//! que o Windows usaria: fabrica COM -> init -> createBuffers -> start -> bufferSwitch.
//! Roda em qualquer sistema. Use com AOIP_SELFTEST=1 e uma pasta HOME temporaria contendo
//! `.aoip-asio.conf` com `rx0=ASIO : 1`, para o driver assinar o proprio fluxo.
use aoip_asio::*;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed};

const FRAMES: usize = 256;
static CALLS: AtomicU64 = AtomicU64::new(0);
static BUFS: [AtomicUsize; 8] = [const { AtomicUsize::new(0) }; 8]; // in1[0,1] in2[0,1] out1[0,1] out2[0,1]
static PEAK: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
static PHASE: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn buffer_switch(index: i32, _direct: i32) {
    let i = index as usize;
    CALLS.fetch_add(1, Relaxed);
    // le as entradas (o que veio da rede) e guarda o pico
    for ch in 0..2 {
        let buf = std::slice::from_raw_parts(BUFS[ch * 2 + i].load(Relaxed) as *const f32, FRAMES);
        let peak = buf.iter().fold(0f32, |m, s| m.max(s.abs()));
        PEAK[ch].fetch_max(peak.to_bits(), Relaxed);
    }
    // toca um tom de 1 kHz nas saidas 1 (0,2) e 2 (0,1)
    let start = PHASE.fetch_add(FRAMES as u64, Relaxed);
    for (ch, gain) in [(0usize, 0.2f32), (1, 0.1)] {
        let buf = std::slice::from_raw_parts_mut(BUFS[4 + ch * 2 + i].load(Relaxed) as *mut f32, FRAMES);
        for (f, s) in buf.iter_mut().enumerate() {
            *s = gain * ((start + f as u64) as f32 * std::f32::consts::TAU * 1000.0 / 48_000.0).sin();
        }
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

        let mut infos = [(1, 0), (1, 1), (0, 0), (0, 1)].map(|(is_input, channel_num)| AsioBufferInfo { is_input, channel_num, buffers: [std::ptr::null_mut(); 2] });
        let callbacks = AsioCallbacks { buffer_switch, sample_rate_did_change: rate_changed, asio_message: message, buffer_switch_time_info: switch_time_info };
        assert_eq!((v.create_buffers)(d, infos.as_mut_ptr(), 4, FRAMES as i32, &callbacks), ASE_OK, "createBuffers");
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
            println!("{s:2} s: {} blocos | pico nas entradas: canal 1 {:.3}, canal 2 {:.3}", CALLS.load(Relaxed), p(0), p(1));
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
