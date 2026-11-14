//! Placa virtual sempre ligada, do lado do programa AOIP: a rede fica no ar o tempo todo
//! (com o painel), e o driver ASIO so leva o audio entre o programa de audio e ela (veja
//! `local`). Sem programa de audio aberto, os fluxos continuam na rede, em silencio.
use crate::local::{CardHost, CardInfo};
use crate::{refresh_status, resolve_iface, start_network, Capture, Network, Playback};
use aoip_core::state::Shared;
use aoip_core::RATE;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Nome da placa no programa de audio e no painel.
pub const CARD_NAME: &str = "AoIP Virtual";
/// Desloca os enderecos dos fluxos (o AoIP Bridge do mesmo computador usa a base 0).
pub const FLOW_BASE: u32 = 64;

struct Running {
    network: Option<Network>,
    capture: Capture,
    playback: Playback,
    channels: usize,
}

pub struct VirtualCard {
    pub shared: Arc<Shared>,
    run: Mutex<Running>,
    info: Mutex<CardInfo>,
    generation: AtomicU32,
    /// Formato de quem entrega o som do computador (canais, taxa), quando houver.
    aux_format: Mutex<Option<(usize, u32)>>,
    aux: Mutex<Option<Capture>>,
}

/// Taxa da placa: 44100 ou 48000 (a rede e sempre 48000; 44100 e convertido).
fn card_rate(r: u32) -> u32 {
    if r == 44_100 { 44_100 } else { RATE }
}

fn silent() -> (Capture, Playback) {
    (Box::new(|_: &[f32]| {}), Box::new(|out: &mut [f32]| out.fill(0.0)))
}

impl VirtualCard {
    pub fn new(shared: Arc<Shared>) -> Arc<Self> {
        let (capture, playback) = silent();
        shared.status.lock().unwrap().virtual_card = true;
        let ch = shared.config.lock().unwrap().card_channels;
        *shared.devices.lock().unwrap() = vec![(CARD_NAME.to_string(), ch, ch)];
        {
            let mut c = shared.config.lock().unwrap();
            c.device = CARD_NAME.into();
            c.rate = card_rate(c.rate);
        }
        Arc::new(Self {
            shared,
            run: Mutex::new(Running { network: None, capture, playback, channels: 0 }),
            info: Mutex::new(CardInfo { channels: 32, rate: RATE, buffer: 128, latency: 192, generation: 0 }),
            generation: AtomicU32::new(1),
            aux_format: Mutex::new(None),
            aux: Mutex::new(None),
        })
    }

    /// Liga (ou religa) a rede com a configuracao atual. Se o formato da placa mudou, o
    /// programa de audio e avisado para reabrir a placa.
    pub fn start(&self) {
        let cfg = {
            let mut c = self.shared.config.lock().unwrap();
            c.device = CARD_NAME.into();
            c.rate = card_rate(c.rate);
            c.clone()
        };
        let (ch, rate) = (cfg.card_channels, cfg.rate);
        *self.shared.devices.lock().unwrap() = vec![(CARD_NAME.to_string(), ch, ch)];
        let old = self.run.lock().unwrap().network.take();
        if let Some(n) = old {
            n.shutdown();
            *self.aux.lock().unwrap() = None;
            std::thread::sleep(Duration::from_millis(700)); // as threads da rede antiga terminam
        }
        {
            let mut st = self.shared.status.lock().unwrap();
            st.running = true; // a placa existe mesmo sem rede
            st.device = CARD_NAME.into();
            (st.ins, st.outs, st.rate) = (ch, ch, rate);
            st.error.clear();
        }
        let started = resolve_iface(&self.shared, &cfg).and_then(|iface| start_network(&self.shared, &cfg, iface, ch, ch, rate, FLOW_BASE));
        let mut run = self.run.lock().unwrap();
        match started {
            Ok((network, capture, playback)) => {
                self.shared.status.lock().unwrap().net_tx = network.net_tx;
                if let (true, Some((ach, arate))) = (cfg.sys_audio, *self.aux_format.lock().unwrap()) {
                    match network.start_aux(ach, arate) {
                        Ok(c) => *self.aux.lock().unwrap() = Some(c),
                        Err(e) => self.shared.status.lock().unwrap().notice = format!("Nao consegui enviar o som do computador: {e}"),
                    }
                }
                *run = Running { network: Some(network), capture, playback, channels: ch };
            }
            Err(e) => {
                let (capture, playback) = silent();
                *run = Running { network: None, capture, playback, channels: ch };
                self.shared.status.lock().unwrap().error =
                    format!("A rede nao abriu: {e} Escolha a placa de rede em Configuracao e salve de novo.");
            }
        }
        drop(run);
        let mut info = self.info.lock().unwrap();
        let changed = info.channels != ch || info.rate != rate || info.buffer != cfg.buffer;
        if changed {
            self.generation.fetch_add(1, Relaxed);
        }
        *info = CardInfo {
            channels: ch, rate, buffer: cfg.buffer,
            latency: cfg.latency_ms.max(1) * rate as usize / 1000,
            generation: self.generation.load(Relaxed),
        };
    }

    /// Chamar a cada ~250 ms: atualiza o painel e aplica a configuracao salva.
    pub fn tick(&self) {
        if self.shared.restart.swap(false, Relaxed) {
            self.start();
        }
        if let Some(n) = &self.run.lock().unwrap().network {
            refresh_status(&self.shared, n);
        }
    }

    /// Avisa o formato do som do computador (ou None quando a captura parou).
    pub fn set_aux_format(&self, format: Option<(usize, u32)>) {
        let changed = *self.aux_format.lock().unwrap() != format;
        *self.aux_format.lock().unwrap() = format;
        if changed && self.shared.config.lock().unwrap().sys_audio {
            self.shared.restart.store(true, Relaxed);
        }
    }

    /// Som do computador capturado (intercalado, no formato de `set_aux_format`).
    pub fn feed_aux(&self, data: &[f32]) {
        if let Some(c) = self.aux.lock().unwrap().as_mut() {
            c(data);
        }
    }

    /// Guarda uma mudanca de configuracao e religa.
    pub fn apply(&self, change: impl FnOnce(&mut aoip_core::state::Config)) {
        change(&mut self.shared.config.lock().unwrap());
        self.shared.save();
        self.shared.restart.store(true, Relaxed);
    }
}

impl CardHost for VirtualCard {
    fn info(&self) -> CardInfo {
        *self.info.lock().unwrap()
    }

    fn process(&self, to_net: &[f32], from_net: &mut [f32], channels: usize) {
        let mut run = self.run.lock().unwrap_or_else(|p| p.into_inner());
        let ch = run.channels.max(1);
        if channels == ch {
            (run.capture)(to_net);
            (run.playback)(from_net);
            return;
        }
        // O programa de audio ainda esta com o formato antigo (vai reabrir): adapta.
        let frames = to_net.len() / channels.max(1);
        let mut a = vec![0f32; frames * ch];
        for f in 0..frames {
            for c in 0..ch.min(channels) {
                a[f * ch + c] = to_net[f * channels + c];
            }
        }
        (run.capture)(&a);
        (run.playback)(&mut a);
        for f in 0..frames {
            for c in 0..ch.min(channels) {
                from_net[f * channels + c] = a[f * ch + c];
            }
        }
    }

    fn set_rate(&self, rate: u32) {
        let rate = card_rate(rate);
        if self.shared.config.lock().unwrap().rate != rate {
            self.shared.config.lock().unwrap().rate = rate;
            self.shared.save();
            self.start();
        }
    }
}
