//! Fila com reamostragem adaptativa: compensa a diferenca entre o relogio da placa
//! (Ui24R) e o relogio PTP da rede, sem estalos por descarte de amostras.
use std::collections::VecDeque;

pub struct DriftFifo {
    ch: usize,
    target: usize, // quadros
    base: usize,   // enchimento pedido, sem a folga do bloco
    block: usize,  // maior bloco visto (quadros)
    q: VecDeque<f32>,
    pos: f64,
    ratio: f64,
    nominal: f64, // amostras de entrada por amostra de saida (1.0 = mesma taxa)
    primed: bool,
    pub underruns: u64,
    pub overruns: u64,
}

impl DriftFifo {
    /// `target_frames`: enchimento desejado (a latencia que esta fila acrescenta).
    pub fn new(channels: usize, target_frames: usize) -> Self {
        Self {
            ch: channels.max(1), target: target_frames.max(8), base: target_frames.max(8), block: 0, q: VecDeque::new(),
            pos: 0.0, ratio: 1.0, nominal: 1.0, primed: false, underruns: 0, overruns: 0,
        }
    }

    /// Fila que tambem converte a taxa: `nominal` = taxa de entrada / taxa de saida
    /// (ex.: placa a 44,1 kHz para rede a 48 kHz = 44100/48000). A conversao e por
    /// interpolacao linear: simples e sem atraso, com leve perda nos agudos.
    pub fn with_ratio(channels: usize, target_frames: usize, nominal: f64) -> Self {
        Self { nominal: nominal.clamp(0.25, 4.0), ..Self::new(channels, target_frames) }
    }

    pub fn channels(&self) -> usize { self.ch }
    pub fn fill(&self) -> usize { self.q.len() / self.ch }
    pub fn target(&self) -> usize { self.target }

    /// Avisa o tamanho do bloco (em quadros) que a placa entrega ou retira de uma vez.
    /// A placa trabalha em rajadas (ex.: 512 quadros) e a rede em pacotes de 1 ms; sem
    /// uma folga do tamanho do bloco a fila esvazia a cada rajada. O alvo passa a ser
    /// o enchimento pedido mais o maior bloco visto.
    pub fn note_block(&mut self, frames: usize) {
        if frames > self.block {
            self.block = frames;
            if self.base + frames > self.target {
                self.target = self.base + frames;
                self.primed = false; // enche de novo ate o alvo novo
            }
        }
    }

    pub fn push(&mut self, interleaved: &[f32]) {
        self.q.extend(interleaved.iter().copied());
        if self.fill() > self.target * 4 {
            // O consumidor parou: descarta o excesso para nao acumular atraso.
            let drop = (self.fill() - self.target) * self.ch;
            self.q.drain(..drop);
            self.overruns += 1;
        }
    }

    /// Preenche `out` (intercalado). Entrega silencio enquanto a fila nao enche.
    pub fn pull(&mut self, out: &mut [f32]) {
        if !self.primed {
            if self.fill() < self.target {
                out.fill(0.0);
                return;
            }
            self.primed = true;
            self.pos = 0.0;
            self.ratio = 1.0;
        }
        // Controle proporcional suave: no maximo 0,5 % de correcao de velocidade.
        let err = (self.fill() as f64 - self.target as f64) / self.target as f64;
        let want = 1.0 + (err * 0.01).clamp(-0.005, 0.005);
        self.ratio += (want - self.ratio) * 0.05;

        let ch = self.ch;
        for frame in out.chunks_exact_mut(ch) {
            if self.q.len() < 2 * ch {
                frame.fill(0.0);
                if self.primed {
                    self.primed = false;
                    self.underruns += 1;
                }
                continue;
            }
            if !self.primed {
                frame.fill(0.0);
                continue;
            }
            let p = self.pos as f32;
            for (c, s) in frame.iter_mut().enumerate() {
                let a = self.q[c];
                *s = a + (self.q[ch + c] - a) * p;
            }
            self.pos += self.ratio * self.nominal;
            while self.pos >= 1.0 && self.q.len() >= ch {
                self.q.drain(..ch);
                self.pos -= 1.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_until_primed_then_passthrough() {
        let mut f = DriftFifo::new(1, 16);
        let mut out = [9.0; 4];
        f.push(&[1.0; 8]);
        f.pull(&mut out);
        assert_eq!(out, [0.0; 4]);
        f.push(&[1.0; 8]);
        f.pull(&mut out);
        assert_eq!(out, [1.0; 4]);
    }

    /// Simula produtor mais rapido/lento que o consumidor e confere que o
    /// enchimento estabiliza sem estouro nem esvaziamento.
    fn run(prod_per_block: usize, every: usize) -> (usize, u64, u64) {
        let mut f = DriftFifo::new(2, 480);
        let mut out = [0.0; 96];
        for i in 0..200_000 {
            let n = if i % every == 0 { prod_per_block } else { 48 };
            f.push(&vec![0.25; n * 2]);
            f.pull(&mut out);
        }
        assert!(out.iter().all(|s| (*s - 0.25).abs() < 1e-6));
        (f.fill(), f.underruns, f.overruns)
    }

    #[test]
    fn tracks_fast_producer() {
        let (fill, under, over) = run(49, 100); // +208 ppm
        assert_eq!((under, over), (0, 0));
        assert!(fill > 400 && fill < 600, "fill={fill}");
    }

    #[test]
    fn tracks_slow_producer() {
        let (fill, under, over) = run(47, 100); // -208 ppm
        assert_eq!((under, over), (0, 0));
        assert!(fill > 360 && fill < 500, "fill={fill}");
    }

    #[test]
    fn recovers_from_underrun_and_overrun() {
        let mut f = DriftFifo::new(1, 16);
        f.push(&[1.0; 16]);
        let mut out = [0.0; 64];
        f.pull(&mut out);
        assert_eq!(f.underruns, 1);
        assert_eq!(out[63], 0.0);
        f.push(&[1.0; 200]);
        assert_eq!(f.overruns, 1);
        assert_eq!(f.fill(), 16);
    }

    #[test]
    fn block_bursts_do_not_underrun_after_note_block() {
        // Placa entrega 512 quadros de uma vez; a rede retira 48 por pacote.
        let run = |note: bool| {
            let mut f = DriftFifo::new(1, 192);
            if note {
                f.note_block(512);
                assert_eq!(f.target(), 192 + 512);
            }
            let (mut out, mut pushed, mut pulled) = ([0.0f32; 48], 0usize, 0usize);
            for _ in 0..400 {
                f.push(&[0.5; 512]);
                pushed += 512;
                while pulled + 48 <= pushed {
                    f.pull(&mut out);
                    pulled += 48;
                }
            }
            f.underruns
        };
        assert!(run(false) > 10, "sem a folga do bloco a fila esvazia a cada rajada");
        assert!(run(true) <= 1, "com a folga a fila nao esvazia");
    }

    #[test]
    fn converts_44100_to_48000() {
        // Entra uma rampa a 44,1 kHz; sai a 48 kHz. Em 1 s consome ~44100 e entrega 48000.
        let mut f = DriftFifo::with_ratio(1, 512, 44_100.0 / 48_000.0);
        let (mut next, mut out, mut last, mut got) = (0f32, [0f32; 480], -1f32, 0usize);
        for _ in 0..100 {
            let block: Vec<f32> = (0..441).map(|_| { next += 1.0; next }).collect();
            f.push(&block);
            f.pull(&mut out);
            for &v in out.iter().filter(|v| **v > 0.0) {
                assert!(v > last, "a saida segue a entrada sem voltar no tempo");
                // passo medio de 0,91875 amostra de entrada por amostra de saida
                assert!(last < 0.0 || (v - last - 0.91875).abs() < 0.02, "passo {}", v - last);
                last = v;
                got += 1;
            }
        }
        assert!(got > 47_000, "entregou {got} amostras");
        assert_eq!(f.underruns, 0);
        assert_eq!(f.overruns, 0);
        assert!(f.fill() > 256 && f.fill() < 1024, "fila estavel: {}", f.fill());
    }
}
