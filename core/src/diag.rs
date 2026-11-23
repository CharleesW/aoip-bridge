//! Diagnostico: numeros que dizem ONDE o audio falha, para o painel mostrar sem precisar
//! entrar no computador. Os contadores sao so somados (o painel faz a conta por minuto) e os
//! "maximos" sao zerados a cada leitura, para mostrar o pior caso recente.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

/// Envio: se `send_us` cresce, cada envio esta custando caro; se `wake_us` cresce, o
/// Windows/macOS esta acordando o programa atrasado; se `lag_us` cresce, o envio nao
/// acompanha o relogio (cada vez que passa de 100 ms ha um salto, `jumps`).
#[derive(Default)]
pub struct TxDiag {
    pub packets: AtomicU64,
    pub errors: AtomicU64,
    pub bytes: AtomicU64,
    pub jumps: AtomicU64,
    pub send_us_sum: AtomicU64,
    pub send_us_max: AtomicU64,
    pub wake_us_max: AtomicU64,
    pub lag_us_max: AtomicU64,
    /// Placa virtual sem programa de audio: os pacotes estao suspensos (nada se perde).
    pub idle: AtomicBool,
}

/// Depois deste tempo sem audio de nenhum programa, a placa virtual para de mandar silencio.
pub const IDLE_AFTER_MS: u64 = 2000;

/// `fed`: hora (ms) do ultimo bloco que a placa entregou; 0 = nenhum ainda.
pub fn is_idle(fed: u64, now: u64) -> bool {
    fed == 0 || now.saturating_sub(fed) > IDLE_AFTER_MS
}

impl TxDiag {
    /// Um bloco (todos os fluxos) levou `us` microssegundos para ser enviado.
    pub fn sent(&self, us: u64, packets: u64, bytes: u64) {
        self.packets.fetch_add(packets, Relaxed);
        self.bytes.fetch_add(bytes, Relaxed);
        self.send_us_sum.fetch_add(us, Relaxed);
        self.send_us_max.fetch_max(us, Relaxed);
    }
}

/// Recepcao de um fluxo.
#[derive(Default)]
pub struct RxDiag {
    pub packets: AtomicU64,
    pub lost: AtomicU64,
    /// Quem envia pulou a hora (o outro programa realinhou).
    pub jumps: AtomicU64,
    /// Maior intervalo entre dois pacotes, em microssegundos.
    pub gap_us_max: AtomicU64,
    /// Vezes em que a fila esvaziou (o som falhou aqui).
    pub underruns: AtomicU64,
    /// Ultima chegada, em ms desde o inicio do programa (0 = nunca).
    pub last_ms: AtomicU64,
}

/// Acompanha numeros de sequencia e de hora para achar perdas e saltos.
#[derive(Default)]
pub struct Sequence {
    last: Option<(u16, u32)>,
}

impl Sequence {
    /// (pacotes perdidos, houve salto de hora). `frames` = amostras por pacote.
    pub fn check(&mut self, seq: u16, ts: u32, frames: u32) -> (u64, bool) {
        let before = self.last.replace((seq, ts));
        let Some((s0, t0)) = before else { return (0, false) };
        let d = seq.wrapping_sub(s0);
        if d == 0 || d > 0x8000 {
            self.last = before; // repetido ou fora de ordem: nao conta nem muda a referencia
            return (0, false);
        }
        let expected = frames.wrapping_mul(d as u32);
        (d as u64 - 1, ts.wrapping_sub(t0) != expected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_losses_jumps_and_reordering() {
        let mut s = Sequence::default();
        assert_eq!(s.check(10, 480, 48), (0, false), "primeiro pacote");
        assert_eq!(s.check(11, 528, 48), (0, false));
        assert_eq!(s.check(14, 672, 48), (2, false), "dois perdidos, hora certa");
        assert_eq!(s.check(15, 720 + 48 * 100, 48), (0, true), "salto de 100 ms");
        assert_eq!(s.check(16, 720 + 48 * 101, 48), (0, false), "segue do salto em diante");
        assert_eq!(s.check(16, 720 + 48 * 101, 48), (0, false), "repetido nao conta");
        assert_eq!(s.check(13, 600, 48), (0, false), "fora de ordem nao conta");
        assert_eq!(s.check(17, 720 + 48 * 102, 48), (0, false));
        // a volta do contador de 16 bits nao e perda
        let mut w = Sequence::default();
        w.check(65_535, 0, 48);
        assert_eq!(w.check(0, 48, 48), (0, false));
    }

    #[test]
    fn a_long_pause_is_not_a_jump() {
        let mut s = Sequence::default();
        s.check(10, 480, 48);
        s = Sequence::default(); // quem recebe recomeca a conta depois de 1 s sem pacotes
        assert_eq!(s.check(11, 480 + 48_000 * 30, 48), (0, false), "voltou depois de 30 s parada");
        assert_eq!(s.check(12, 480 + 48_000 * 30 + 48, 48), (0, false));
    }

    #[test]
    fn idle_only_without_any_audio_program() {
        assert!(is_idle(0, 1), "nunca tocou");
        assert!(!is_idle(5000, 5001));
        assert!(!is_idle(5000, 5000 + IDLE_AFTER_MS));
        assert!(is_idle(5000, 5001 + IDLE_AFTER_MS));
        assert!(!is_idle(5000, 4990), "relogio lido um instante antes");
    }

    #[test]
    fn counters() {
        let t = TxDiag::default();
        t.sent(250, 4, 4800);
        t.sent(900, 4, 4800);
        assert_eq!((t.packets.load(Relaxed), t.bytes.load(Relaxed)), (8, 9600));
        assert_eq!((t.send_us_sum.load(Relaxed), t.send_us_max.load(Relaxed)), (1150, 900));
    }
}
