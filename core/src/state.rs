//! Estado compartilhado entre o motor de audio, a rede e a interface.
use crate::matrix::Matrix;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;

/// Fluxos que podem ser recebidos ao mesmo tempo (8 canais cada).
pub const RX_SLOTS: usize = 4;
pub const NET_RX: usize = RX_SLOTS * crate::MAX_FLOW_CHANNELS;
pub const MAX_CH: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub device: String,
    pub iface: String,
    pub name: String,
    /// 0 = todas as entradas da placa.
    pub tx_channels: usize,
    pub latency_ms: usize,
    /// Taxa de amostragem pedida para a placa, em Hz. 0 = automatica (48000 se a placa
    /// aceitar; senao a taxa atual dela, com conversao para os 48000 da rede).
    pub rate: u32,
    /// Nome do fluxo recebido em cada posicao ("" = vazio).
    pub rx: Vec<String>,
    pub tx_map: Option<String>,
    pub rx_map: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            device: String::new(), iface: String::new(), name: "Bridge".into(), tx_channels: 0,
            latency_ms: 4, rate: 0, rx: vec![String::new(); RX_SLOTS], tx_map: None, rx_map: None,
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Self {
        let mut c = Self::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "device" => c.device = v.into(),
                "iface" => c.iface = v.into(),
                "name" if !v.is_empty() => c.name = v.into(),
                "tx_channels" => c.tx_channels = v.parse().unwrap_or(0),
                "latency_ms" => c.latency_ms = v.parse().unwrap_or(4).clamp(1, 100),
                "rate" => c.rate = v.parse().unwrap_or(0),
                "tx_map" => c.tx_map = Some(v.into()),
                "rx_map" => c.rx_map = Some(v.into()),
                k => {
                    if let Some(Ok(i)) = k.strip_prefix("rx").map(str::parse::<usize>) {
                        if i < RX_SLOTS {
                            c.rx[i] = v.into();
                        }
                    }
                }
            }
        }
        c
    }

    pub fn to_text(&self) -> String {
        let line = |s: &str| s.replace(['\r', '\n'], " ");
        let mut t = format!(
            "device={}\niface={}\nname={}\ntx_channels={}\nlatency_ms={}\nrate={}\n",
            line(&self.device), line(&self.iface), line(&self.name), self.tx_channels, self.latency_ms, self.rate
        );
        for (i, r) in self.rx.iter().enumerate() {
            t += &format!("rx{i}={}\n", line(r));
        }
        if let Some(m) = &self.tx_map {
            t += &format!("tx_map={m}\n");
        }
        if let Some(m) = &self.rx_map {
            t += &format!("rx_map={m}\n");
        }
        t
    }
}

/// Matriz editada pela interface e lida pelo callback de audio. O callback guarda
/// uma copia e so a troca quando `version` muda, sem esperar por trava.
pub struct MatrixCell {
    m: Mutex<Matrix>,
    version: AtomicU64,
}

impl MatrixCell {
    fn new() -> Self {
        Self { m: Mutex::new(Matrix::new(0, 0)), version: AtomicU64::new(0) }
    }
    pub fn version(&self) -> u64 {
        self.version.load(Relaxed)
    }
    pub fn snapshot(&self) -> Matrix {
        self.m.lock().unwrap().clone()
    }
    /// Para o callback de audio: nao bloqueia.
    pub fn try_snapshot(&self) -> Option<Matrix> {
        self.m.try_lock().ok().map(|m| m.clone())
    }
    pub fn edit<R>(&self, f: impl FnOnce(&mut Matrix) -> R) -> R {
        let r = f(&mut self.m.lock().unwrap());
        self.version.fetch_add(1, Relaxed);
        r
    }
}

#[derive(Clone, Default)]
pub struct Status {
    pub running: bool,
    pub error: String,
    pub device: String,
    pub iface: String,
    pub ins: usize,
    pub outs: usize,
    pub net_tx: usize,
    /// Taxa em que a placa foi aberta (Hz).
    pub rate: u32,
    pub ptp: bool,
    pub grandmaster: String,
    pub tx_fill: usize,
    pub underruns: u64,
    pub overruns: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    pub name: String,
    pub addr: String,
    pub channels: u8,
    /// 48 kHz, L24/L16: da para receber.
    pub compatible: bool,
}

pub struct Shared {
    pub config: Mutex<Config>,
    pub config_path: PathBuf,
    pub tx: MatrixCell,
    pub rx: MatrixCell,
    /// (nome, entradas, saidas)
    pub devices: Mutex<Vec<(String, usize, usize)>>,
    pub status: Mutex<Status>,
    pub flows: Mutex<Vec<Flow>>,
    pub slot_active: Mutex<Vec<bool>>,
    pub in_peak: Vec<AtomicU32>,
    pub rx_peak: Vec<AtomicU32>,
    pub restart: AtomicBool,
    pub quit: AtomicBool,
}

impl Shared {
    pub fn new(config: Config, config_path: PathBuf) -> Self {
        let peaks = || (0..MAX_CH).map(|_| AtomicU32::new(0)).collect();
        Self {
            config: Mutex::new(config), config_path, tx: MatrixCell::new(), rx: MatrixCell::new(),
            devices: Mutex::new(vec![]), status: Mutex::new(Status::default()), flows: Mutex::new(vec![]),
            slot_active: Mutex::new(vec![false; RX_SLOTS]), in_peak: peaks(), rx_peak: peaks(),
            restart: AtomicBool::new(false), quit: AtomicBool::new(false),
        }
    }

    pub fn load(path: PathBuf) -> Self {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        Self::new(Config::parse(&text), path)
    }

    pub fn save(&self) {
        let text = self.config.lock().unwrap().to_text();
        if let Err(e) = std::fs::write(&self.config_path, text) {
            eprintln!("nao consegui salvar {}: {e}", self.config_path.display());
        }
    }
}

/// Registra o pico por canal de um bloco intercalado (para os medidores).
pub fn note_peaks(peaks: &[AtomicU32], interleaved: &[f32], ch: usize) {
    for (c, slot) in peaks.iter().enumerate().take(ch) {
        let mut max = 0f32;
        let mut i = c;
        while i < interleaved.len() {
            max = max.max(interleaved[i].abs());
            i += ch;
        }
        // Para floats nao negativos, a ordem dos bits e a ordem dos valores.
        slot.fetch_max(max.to_bits(), Relaxed);
    }
}

/// Le e zera os picos (cada leitura mostra o maximo desde a anterior).
pub fn take_peaks(peaks: &[AtomicU32], ch: usize) -> Vec<f32> {
    peaks.iter().take(ch).map(|p| f32::from_bits(p.swap(0, Relaxed))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip() {
        let mut c = Config::default();
        c.device = "Soundcraft Ui24R".into();
        c.iface = "192.168.1.50".into();
        c.rx[1] = "MESA : 2".into();
        c.tx_map = Some("1>1@0".into());
        c.rx_map = Some(String::new());
        assert_eq!(Config::parse(&c.to_text()), c);
        assert_eq!(Config::parse("lixo\nlatency_ms=999\nrx9=x\n").latency_ms, 100);
    }

    #[test]
    fn peaks() {
        let p: Vec<AtomicU32> = (0..2).map(|_| AtomicU32::new(0)).collect();
        note_peaks(&p, &[0.1, -0.9, -0.5, 0.2], 2);
        note_peaks(&p, &[0.3, 0.0], 2);
        assert_eq!(take_peaks(&p, 2), vec![0.5, 0.9]);
        assert_eq!(take_peaks(&p, 2), vec![0.0, 0.0]);
    }

    #[test]
    fn matrix_cell_versions() {
        let s = Shared::new(Config::default(), PathBuf::from("/nonexistent/x"));
        let v = s.tx.version();
        s.tx.edit(|m| *m = Matrix::identity(2, 2));
        assert_ne!(s.tx.version(), v);
        assert_eq!(s.tx.try_snapshot().unwrap().to_spec(), "1>1@0,2>2@0");
    }
}
