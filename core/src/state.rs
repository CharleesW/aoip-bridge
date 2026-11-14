//! Estado compartilhado entre o motor de audio, a rede e a interface.
use crate::matrix::Matrix;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;

/// Fluxos que podem ser recebidos ao mesmo tempo (8 canais cada).
pub const RX_SLOTS: usize = 8;
pub const NET_RX: usize = RX_SLOTS * crate::MAX_FLOW_CHANNELS;
pub const MAX_CH: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub device: String,
    /// Placa de rede: "" = automatica; o nome da placa ("Ethernet", "en0"); ou um IP fixo.
    pub iface: String,
    /// Nome que aparece na rede. "" = automatico (o nome do computador).
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
    /// Placa virtual: canais de entrada e de saida (2, 4, 8, 16, 32 ou 64).
    pub card_channels: usize,
    /// Placa virtual: tamanho do bloco de audio em amostras (64, 128, 256 ou 512).
    pub buffer: usize,
    /// Placa virtual (Windows): mandar o som do Windows para a rede, num fluxo a parte.
    pub sys_audio: bool,
    /// Nome de cada canal enviado, a parte depois de "Ch 01" ("Bumbo"). Vazio = so "Ch 01".
    pub tx_names: Vec<String>,
}

/// Opcoes da placa virtual.
pub const CARD_CHANNELS: [usize; 6] = [2, 4, 8, 16, 32, 64];
pub const BUFFERS: [usize; 4] = [64, 128, 256, 512];

impl Default for Config {
    fn default() -> Self {
        Self {
            device: String::new(), iface: String::new(), name: String::new(), tx_channels: 0,
            latency_ms: 4, rate: 0, rx: vec![String::new(); RX_SLOTS], tx_map: None, rx_map: None,
            card_channels: 32, buffer: 128, sys_audio: false, tx_names: Vec::new(),
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
                // Os nomes fixos das versoes antigas viram "automatico" (nome do computador).
                "name" => c.name = if LEGACY_NAMES.contains(&v) { String::new() } else { v.into() },
                "tx_channels" => c.tx_channels = v.parse().unwrap_or(0),
                "latency_ms" => c.latency_ms = v.parse().unwrap_or(4).clamp(1, 100),
                "rate" => c.rate = v.parse().unwrap_or(0),
                "tx_map" => c.tx_map = Some(v.into()),
                "rx_map" => c.rx_map = Some(v.into()),
                "card_channels" => c.card_channels = v.parse().ok().filter(|n| CARD_CHANNELS.contains(n)).unwrap_or(32),
                "buffer" => c.buffer = v.parse().ok().filter(|n| BUFFERS.contains(n)).unwrap_or(128),
                "sys_audio" => c.sys_audio = v == "1" || v == "true",
                k => {
                    if let Some(Ok(i)) = k.strip_prefix("txname").map(str::parse::<usize>) {
                        if i < MAX_CH {
                            if c.tx_names.len() <= i {
                                c.tx_names.resize(i + 1, String::new());
                            }
                            c.tx_names[i] = v.into();
                        }
                    } else if let Some(Ok(i)) = k.strip_prefix("rx").map(str::parse::<usize>) {
                        if i < RX_SLOTS {
                            c.rx[i] = v.into();
                        }
                    }
                }
            }
        }
        c
    }

    /// Nome dos fluxos na rede: o escolhido, ou o automatico.
    pub fn flow_name(&self, auto: &str) -> String {
        let n = self.name.trim();
        if n.is_empty() { auto.to_string() } else { clip(n, NAME_MAX) }
    }

    pub fn to_text(&self) -> String {
        let line = |s: &str| s.replace(['\r', '\n'], " ");
        let mut t = format!(
            "device={}\niface={}\nname={}\ntx_channels={}\nlatency_ms={}\nrate={}\n",
            line(&self.device), line(&self.iface), line(&self.name), self.tx_channels, self.latency_ms, self.rate
        );
        t += &format!("card_channels={}\nbuffer={}\nsys_audio={}\n", self.card_channels, self.buffer, self.sys_audio as u8);
        for (i, n) in self.tx_names.iter().enumerate().filter(|(_, n)| !n.trim().is_empty()) {
            t += &format!("txname{i}={}\n", line(n));
        }
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

/// Nomes padrao das versoes antigas (tratados como "automatico").
const LEGACY_NAMES: [&str; 3] = ["Bridge", "ASIO", "Mac"];
/// Tamanho maximo do nome na rede (o Dante Controller corta nomes longos).
pub const NAME_MAX: usize = 24;

fn clip(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

/// Nome automatico: o nome do computador, mais um sufixo para diferenciar programas no
/// mesmo computador (ex.: "PC-IGREJA" e "PC-IGREJA Virtual").
pub fn auto_name(computer: &str, suffix: &str) -> String {
    let base = computer.trim().trim_end_matches(".local");
    let base = if base.is_empty() { "AoIP" } else { base };
    let room = NAME_MAX.saturating_sub(suffix.chars().count());
    format!("{}{}", clip(base, room), suffix)
}

/// Nome do aparelho a partir do nome de um fluxo ("Ui24R-ab12 : 3" -> "Ui24R-ab12").
pub fn device_of_flow(flow: &str) -> &str {
    flow.rsplit_once(" : ").map_or(flow, |(d, _)| d).trim()
}

/// Uma placa de rede do computador.
#[derive(Clone, Debug, PartialEq)]
pub struct NetIface {
    /// Nome no sistema, guardado na configuracao ("Ethernet", "en0").
    pub id: String,
    /// Como aparece no painel ("Wi-Fi (en0)").
    pub label: String,
    pub ip: std::net::Ipv4Addr,
}

/// Decide a placa de rede a partir do que foi salvo. Devolve o IP a usar (None = escolher
/// sozinho) e um aviso quando o que foi salvo nao existe mais (outra rede, cabo solto):
/// nesse caso volta para o automatico em vez de falhar.
pub fn choose_iface(saved: &str, list: &[NetIface]) -> (Option<std::net::Ipv4Addr>, Option<String>) {
    let saved = saved.trim();
    if saved.is_empty() {
        return (None, None);
    }
    if let Ok(ip) = saved.parse::<std::net::Ipv4Addr>() {
        if list.iter().any(|i| i.ip == ip) {
            return (Some(ip), None);
        }
        return (None, Some(format!("O IP salvo ({saved}) nao existe nesta rede; usando a placa de rede automatica.")));
    }
    match list.iter().find(|i| i.id == saved) {
        Some(i) => (Some(i.ip), None),
        None => (None, Some(format!("A placa de rede \"{saved}\" nao esta ligada; usando a placa de rede automatica."))),
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
    /// Identidade do relogio mestre (hexadecimal), o IP e o nome de quem e o mestre.
    pub grandmaster: String,
    pub gm_ip: String,
    pub gm_name: String,
    /// Este programa e o relogio mestre.
    pub gm_self: bool,
    /// Nome em uso na rede e o nome automatico (para o painel sugerir).
    pub name: String,
    pub auto_name: String,
    /// Placa de rede em uso, como aparece no painel.
    pub iface_label: String,
    /// Aviso que nao impede o funcionamento (ex.: voltou para a rede automatica).
    pub notice: String,
    /// Este painel e o da placa virtual (canais, bloco e taxa escolhidos aqui).
    pub virtual_card: bool,
    /// Nome do fluxo a parte com o som do computador.
    pub aux_name: String,
    pub tx_fill: usize,
    pub underruns: u64,
    pub overruns: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    pub name: String,
    pub addr: String,
    pub channels: u8,
    /// IP de quem envia.
    pub origin: String,
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
    /// Placas de rede do computador (para escolher no painel).
    pub ifaces: Mutex<Vec<NetIface>>,
    /// Maquinas AOIP vistas na rede (Controller), inclusive esta.
    pub nodes: Mutex<Vec<crate::routing::SeenNode>>,
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
            devices: Mutex::new(vec![]), ifaces: Mutex::new(vec![]), nodes: Mutex::new(vec![]), status: Mutex::new(Status::default()), flows: Mutex::new(vec![]),
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
        c.card_channels = 16;
        c.buffer = 256;
        c.sys_audio = true;
        c.tx_names = vec!["Bumbo".into(), String::new(), "Caixa".into()];
        assert_eq!(Config::parse(&c.to_text()), c);
        assert_eq!(Config::parse("card_channels=7\nbuffer=100\n").card_channels, 32, "valor fora das opcoes volta ao padrao");
        assert_eq!(Config::parse("lixo\nlatency_ms=999\nrx9=x\n").latency_ms, 100);
    }

    #[test]
    fn names() {
        assert_eq!(Config::parse("name=Bridge\n").name, "", "nome antigo vira automatico");
        assert_eq!(Config::parse("name=ASIO\n").flow_name("PC-IGREJA Virtual"), "PC-IGREJA Virtual");
        assert_eq!(Config::parse("name=Mesa Louvor\n").flow_name("PC"), "Mesa Louvor");
        assert_eq!(auto_name("PC-IGREJA", ""), "PC-IGREJA");
        assert_eq!(auto_name("MacBook-Pro.local", " Virtual"), "MacBook-Pro Virtual");
        assert_eq!(auto_name("", ""), "AoIP");
        assert_eq!(auto_name("UM-NOME-DE-COMPUTADOR-MUITO-COMPRIDO", " Virtual").chars().count(), NAME_MAX);
        assert_eq!(device_of_flow("Ui24R-ab12 : 3"), "Ui24R-ab12");
        assert_eq!(device_of_flow("Fluxo solto"), "Fluxo solto");
    }

    #[test]
    fn iface_choice() {
        let list = vec![
            NetIface { id: "Ethernet".into(), label: "Ethernet".into(), ip: "10.0.0.7".parse().unwrap() },
            NetIface { id: "Wi-Fi".into(), label: "Wi-Fi".into(), ip: "192.168.5.20".parse().unwrap() },
        ];
        assert_eq!(choose_iface("", &list), (None, None));
        assert_eq!(choose_iface("Wi-Fi", &list).0, Some("192.168.5.20".parse().unwrap()));
        assert_eq!(choose_iface("10.0.0.7", &list).0, Some("10.0.0.7".parse().unwrap()));
        // IP de outra rede (a de casa, levada para a igreja): volta para o automatico com aviso
        let (ip, note) = choose_iface("192.168.5.49", &list);
        assert!(ip.is_none() && note.unwrap().contains("192.168.5.49"));
        let (ip, note) = choose_iface("Ethernet 2", &list);
        assert!(ip.is_none() && note.unwrap().contains("Ethernet 2"));
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
