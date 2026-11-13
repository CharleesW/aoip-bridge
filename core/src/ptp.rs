//! Seguidor PTPv2 (IEEE 1588-2008) somente-escuta, com carimbo de tempo por software.
//!
//! Nao mede o atraso do caminho (sem Delay_Req), entao o relogio fica atrasado pelo
//! tempo de transito na rede local (dezenas de microssegundos), o que cabe com folga
//! na latencia de recepcao dos dispositivos AES67.

pub const GROUP: std::net::Ipv4Addr = std::net::Ipv4Addr::new(224, 0, 1, 129);
pub const EVENT_PORT: u16 = 319;
pub const GENERAL_PORT: u16 = 320;

#[derive(Debug, PartialEq)]
pub enum Msg {
    Sync { seq: u16, two_step: bool, ts_ns: i128 },
    FollowUp { seq: u16, ts_ns: i128 },
    Announce { gmid: [u8; 8] },
}

/// Devolve (dominio, mensagem) para as mensagens que interessam.
pub fn parse(b: &[u8]) -> Option<(u8, Msg)> {
    if b.len() < 44 || b[1] & 0x0f != 2 {
        return None;
    }
    let seq = u16::from_be_bytes([b[30], b[31]]);
    let corr = i64::from_be_bytes(b[8..16].try_into().ok()?) >> 16;
    let ts = || {
        let secs = u64::from_be_bytes([0, 0, b[34], b[35], b[36], b[37], b[38], b[39]]);
        let ns = u32::from_be_bytes([b[40], b[41], b[42], b[43]]);
        secs as i128 * 1_000_000_000 + ns as i128 + corr as i128
    };
    let msg = match b[0] & 0x0f {
        0x0 => Msg::Sync { seq, two_step: b[6] & 0x02 != 0, ts_ns: ts() },
        0x8 => Msg::FollowUp { seq, ts_ns: ts() },
        0xb => Msg::Announce { gmid: b.get(53..61)?.try_into().ok()? },
        _ => return None,
    };
    Some((b[4], msg))
}

/// Identidade do relogio que enviou a mensagem (sourcePortIdentity).
pub fn source_id(b: &[u8]) -> Option<[u8; 8]> {
    if b.len() < 34 || b[1] & 0x0f != 2 {
        return None;
    }
    b[20..28].try_into().ok()
}

/// De um Announce: (prioridade 1, identidade do grandmaster). Menor ganha.
pub fn announce_rank(b: &[u8]) -> Option<(u8, [u8; 8])> {
    if b.len() < 64 || b[1] & 0x0f != 2 || b[0] & 0x0f != 0xb {
        return None;
    }
    Some((b[47], b[53..61].try_into().ok()?))
}

// ---- Relogio proprio (mestre de ultima instancia) ----
//
// Quando nao ha nenhum relogio PTPv2 na rede (nenhum aparelho Dante em AES67 Mode), um
// dos computadores com o programa passa a gerar o relogio, para que os outros possam
// segui-lo. Ele se anuncia com a pior prioridade possivel, entao qualquer aparelho de
// verdade ganha a eleicao e o programa volta a ser seguidor.

/// Prioridade 1 anunciada pelo relogio proprio (a pior que nao e "so seguidor").
pub const OWN_PRIORITY1: u8 = 254;
/// Prioridade de quem esta com uma placa de verdade (a mesa de som): ganha das placas
/// virtuais, que so assumem o relogio se ele sair da rede. Aparelhos Dante (128) ganham dele.
pub const CARD_PRIORITY1: u8 = 250;
/// Sem Announce de um relogio melhor por este tempo, assume o papel de mestre.
pub const MASTER_TIMEOUT_NS: i128 = 7_000_000_000;

/// Identidade de relogio derivada do IP (formato EUI-64 de uso local). `instance` diferencia
/// dois programas no mesmo computador (o AoIP Bridge e a placa virtual), que senao teriam a
/// mesma identidade e poderiam virar mestres ao mesmo tempo.
pub fn clock_id(ip: std::net::Ipv4Addr, instance: u8) -> [u8; 8] {
    let o = ip.octets();
    [0x02, instance, o[0], 0xff, 0xfe, o[1], o[2], o[3]]
}

fn header(kind: u8, len: u16, flags0: u8, id: [u8; 8], seq: u16, control: u8, log_interval: i8) -> Vec<u8> {
    let mut b = vec![0u8; len as usize];
    b[0] = kind;
    b[1] = 2;
    b[2..4].copy_from_slice(&len.to_be_bytes());
    b[6] = flags0;
    b[7] = 0x08; // escala de tempo PTP
    b[20..28].copy_from_slice(&id);
    b[29] = 1; // porta 1
    b[30..32].copy_from_slice(&seq.to_be_bytes());
    b[32] = control;
    b[33] = log_interval as u8;
    b
}

fn put_ts(b: &mut [u8], ts_ns: u128) {
    let (secs, ns) = ((ts_ns / 1_000_000_000) as u64, (ts_ns % 1_000_000_000) as u32);
    b[34..40].copy_from_slice(&secs.to_be_bytes()[2..]);
    b[40..44].copy_from_slice(&ns.to_be_bytes());
}

/// Sync em dois passos (a hora exata vai no Follow_Up). 8 por segundo.
pub fn build_sync(id: [u8; 8], seq: u16) -> Vec<u8> {
    header(0x0, 44, 0x02, id, seq, 0, -3)
}

pub fn build_follow_up(id: [u8; 8], seq: u16, ts_ns: u128) -> Vec<u8> {
    let mut b = header(0x8, 44, 0, id, seq, 2, -3);
    put_ts(&mut b, ts_ns);
    b
}

/// Announce do relogio proprio. 1 por segundo.
pub fn build_announce(id: [u8; 8], seq: u16, ts_ns: u128, priority1: u8) -> Vec<u8> {
    let mut b = header(0xb, 64, 0, id, seq, 5, 0);
    put_ts(&mut b, ts_ns);
    b[47] = priority1;
    b[48] = 248; // classe: relogio livre, sem referencia externa
    b[49] = 0xfe; // exatidao desconhecida
    b[50..52].copy_from_slice(&0xffffu16.to_be_bytes());
    b[52] = 255; // prioridade 2
    b[53..61].copy_from_slice(&id);
    b[63] = 0xa0; // fonte: oscilador interno
    b
}

/// Decide se este computador deve gerar o relogio.
pub struct Election {
    own: [u8; 8],
    priority1: u8,
    last_better: i128,
}

impl Election {
    /// `now_ns`: relogio monotonico local. Comeca esperando, para ouvir a rede primeiro.
    /// `priority1`: `CARD_PRIORITY1` com placa de verdade, `OWN_PRIORITY1` na placa virtual.
    pub fn new(own: [u8; 8], priority1: u8, now_ns: i128) -> Self {
        Self { own, priority1, last_better: now_ns }
    }

    pub fn priority1(&self) -> u8 {
        self.priority1
    }

    /// Registra um Announce recebido (prioridade 1 e identidade do grandmaster).
    pub fn on_announce(&mut self, priority1: u8, gmid: [u8; 8], now_ns: i128) {
        if gmid != self.own && (priority1, gmid) < (self.priority1, self.own) {
            self.last_better = now_ns;
        }
    }

    pub fn is_master(&self, now_ns: i128) -> bool {
        now_ns - self.last_better > MASTER_TIMEOUT_NS
    }
}

/// Estima `deslocamento = hora PTP - relogio local` a partir de Sync/Follow_Up.
#[derive(Default)]
pub struct Follower {
    pending: Option<(u16, i128)>,
    offset: Option<i128>,
    outliers: u8,
    pub gmid: Option<[u8; 8]>,
}

impl Follower {
    /// `local_ns`: relogio monotonico local no instante em que o pacote chegou.
    /// Devolve o deslocamento filtrado quando ha uma nova medida.
    pub fn on_msg(&mut self, msg: Msg, local_ns: i128) -> Option<i128> {
        let raw = match msg {
            Msg::Announce { gmid } => {
                self.gmid = Some(gmid);
                return None;
            }
            Msg::Sync { seq, two_step: true, .. } => {
                self.pending = Some((seq, local_ns));
                return None;
            }
            Msg::Sync { ts_ns, .. } => ts_ns - local_ns,
            Msg::FollowUp { seq, ts_ns } => match self.pending.take() {
                Some((s, rx)) if s == seq => ts_ns - rx,
                _ => return None,
            },
        };
        let est = match self.offset {
            None => raw,
            Some(est) => {
                let err = raw - est;
                if err.abs() > 5_000_000 {
                    // Pacote atrasado pelo sistema, ou o mestre mudou: so aceita
                    // o salto se persistir.
                    self.outliers += 1;
                    if self.outliers < 8 {
                        return None;
                    }
                    raw
                } else {
                    est + err / 8
                }
            }
        };
        self.outliers = 0;
        self.offset = Some(est);
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(kind: u8, flags0: u8, seq: u16, secs: u64, ns: u32) -> Vec<u8> {
        let mut b = vec![0u8; 64];
        b[0] = kind;
        b[1] = 2;
        b[4] = 0; // dominio
        b[6] = flags0;
        b[30..32].copy_from_slice(&seq.to_be_bytes());
        b[34..40].copy_from_slice(&secs.to_be_bytes()[2..]);
        b[40..44].copy_from_slice(&ns.to_be_bytes());
        b
    }

    #[test]
    fn two_step_sync() {
        let mut f = Follower::default();
        let (_, sync) = parse(&header(0x0, 0x02, 5, 0, 0)).unwrap();
        assert_eq!(f.on_msg(sync, 1_000), None);
        let (dom, fu) = parse(&header(0x8, 0, 5, 100, 500)).unwrap();
        assert_eq!(dom, 0);
        assert_eq!(f.on_msg(fu, 9_999), Some(100_000_000_500 - 1_000));
    }

    #[test]
    fn follow_up_with_wrong_seq_is_ignored() {
        let mut f = Follower::default();
        f.on_msg(parse(&header(0x0, 0x02, 5, 0, 0)).unwrap().1, 0);
        assert_eq!(f.on_msg(parse(&header(0x8, 0, 6, 1, 0)).unwrap().1, 0), None);
    }

    #[test]
    fn filters_jitter_and_accepts_persistent_step() {
        let mut f = Follower::default();
        let one = |f: &mut Follower, ptp: i128, local: i128| {
            f.on_msg(Msg::Sync { seq: 0, two_step: false, ts_ns: ptp }, local)
        };
        assert_eq!(one(&mut f, 1_000_000, 0), Some(1_000_000));
        // medida 80 us fora: so 1/8 entra
        assert_eq!(one(&mut f, 1_080_000, 0), Some(1_010_000));
        // pacote isolado atrasado 20 ms: descartado
        assert_eq!(one(&mut f, 1_000_000, 20_000_000), None);
        // salto persistente de 1 s: aceito na 8a medida seguida fora da faixa
        // (a medida atrasada acima ja contou como a 1a)
        for _ in 0..6 {
            assert_eq!(one(&mut f, 1_000_000_000, 0), None);
        }
        assert_eq!(one(&mut f, 1_000_000_000, 0), Some(1_000_000_000));
    }

    #[test]
    fn announce_and_correction_field() {
        let mut b = header(0xb, 0, 0, 0, 0);
        b[53..61].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(parse(&b).unwrap().1, Msg::Announce { gmid: [1, 2, 3, 4, 5, 6, 7, 8] });
        let mut s = header(0x0, 0, 0, 1, 0);
        s[8..16].copy_from_slice(&((250i64) << 16).to_be_bytes());
        assert_eq!(parse(&s).unwrap().1, Msg::Sync { seq: 0, two_step: false, ts_ns: 1_000_000_250 });
        assert!(parse(&header(0x0, 0, 0, 0, 0)[..20]).is_none());
    }

    #[test]
    fn own_clock_messages_round_trip() {
        let id = clock_id(std::net::Ipv4Addr::new(192, 168, 5, 10), 0);
        assert_eq!(id, [0x02, 0x00, 192, 0xff, 0xfe, 168, 5, 10]);
        let sync = build_sync(id, 7);
        assert_eq!(parse(&sync).unwrap(), (0, Msg::Sync { seq: 7, two_step: true, ts_ns: 0 }));
        assert_eq!(source_id(&sync), Some(id));
        let fu = build_follow_up(id, 7, 1_700_000_000_123_456_789);
        assert_eq!(parse(&fu).unwrap().1, Msg::FollowUp { seq: 7, ts_ns: 1_700_000_000_123_456_789 });
        let ann = build_announce(id, 1, 0, OWN_PRIORITY1);
        assert_eq!(ann.len(), 64);
        assert_eq!(parse(&ann).unwrap().1, Msg::Announce { gmid: id });
        assert_eq!(announce_rank(&ann), Some((OWN_PRIORITY1, id)));
        assert_eq!(announce_rank(&sync), None);
        // um seguidor trava no relogio proprio
        let mut f = Follower::default();
        f.on_msg(parse(&sync).unwrap().1, 500);
        assert_eq!(f.on_msg(parse(&fu).unwrap().1, 900), Some(1_700_000_000_123_456_789 - 500));
    }

    #[test]
    fn election_waits_yields_and_breaks_ties() {
        let (a, b) = (clock_id(std::net::Ipv4Addr::new(192, 168, 5, 10), 0), clock_id(std::net::Ipv4Addr::new(192, 168, 5, 48), 0));
        // dois programas no mesmo computador nao tem a mesma identidade
        assert_ne!(clock_id(std::net::Ipv4Addr::new(192, 168, 5, 10), 1), a);
        let sec = 1_000_000_000i128;
        let mut e = Election::new(b, OWN_PRIORITY1, 0);
        assert!(!e.is_master(6 * sec), "escuta a rede antes de assumir");
        assert!(e.is_master(8 * sec));
        // aparelho de verdade (prioridade 128): cede enquanto ele anuncia
        e.on_announce(128, [9; 8], 8 * sec);
        assert!(!e.is_master(10 * sec));
        assert!(e.is_master(16 * sec), "reassume se o aparelho sumir");
        // outro computador com o programa: ganha a identidade menor (a), b cede
        e.on_announce(OWN_PRIORITY1, a, 20 * sec);
        assert!(!e.is_master(21 * sec));
        let mut ea = Election::new(a, OWN_PRIORITY1, 0);
        ea.on_announce(OWN_PRIORITY1, b, 20 * sec);
        assert!(ea.is_master(21 * sec));
        // o proprio eco nao conta
        ea.on_announce(OWN_PRIORITY1, a, 30 * sec);
        assert!(ea.is_master(31 * sec));
        // quem esta com a mesa (placa de verdade) ganha da placa virtual, mesmo com
        // identidade maior; a virtual so assume quando ele some da rede
        let (card, virt) = (clock_id(std::net::Ipv4Addr::new(192, 168, 5, 90), 0), clock_id(std::net::Ipv4Addr::new(192, 168, 5, 2), 1));
        let mut ev = Election::new(virt, OWN_PRIORITY1, 0);
        let mut ec = Election::new(card, CARD_PRIORITY1, 0);
        ev.on_announce(CARD_PRIORITY1, card, 40 * sec);
        ec.on_announce(OWN_PRIORITY1, virt, 40 * sec);
        assert!(!ev.is_master(41 * sec) && ec.is_master(41 * sec));
        assert!(ev.is_master(48 * sec), "a mesa caiu: a virtual assume");
        assert_eq!(announce_rank(&build_announce(card, 1, 0, CARD_PRIORITY1)), Some((CARD_PRIORITY1, card)));
    }
}
