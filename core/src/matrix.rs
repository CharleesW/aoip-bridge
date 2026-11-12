//! Matriz de roteamento N entradas x M saidas com ganho por ponto de cruzamento.

#[derive(Clone, Debug)]
pub struct Matrix {
    ins: usize,
    outs: usize,
    gains: Vec<f32>, // [out * ins + in]
}

impl Matrix {
    pub fn new(ins: usize, outs: usize) -> Self {
        Self { ins, outs, gains: vec![0.0; ins * outs] }
    }

    /// Preset 1:1 (entrada 1 -> saida 1, 2 -> 2, ...).
    pub fn identity(ins: usize, outs: usize) -> Self {
        let mut m = Self::new(ins, outs);
        for i in 0..ins.min(outs) {
            m.set(i, i, 1.0);
        }
        m
    }

    pub fn ins(&self) -> usize { self.ins }
    pub fn outs(&self) -> usize { self.outs }

    /// Indices base 0. Ganho linear (1.0 = 0 dB, 0.0 = desligado).
    pub fn set(&mut self, input: usize, output: usize, gain: f32) {
        if input < self.ins && output < self.outs {
            self.gains[output * self.ins + input] = gain;
        }
    }

    pub fn get(&self, input: usize, output: usize) -> f32 {
        self.gains[output * self.ins + input]
    }

    /// Texto "1>1,2>2,5>3@-6": entrada>saida (base 1), ganho opcional em dB.
    pub fn parse(spec: &str, ins: usize, outs: usize) -> Result<Self, String> {
        let mut m = Self::new(ins, outs);
        for item in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (route, db) = match item.split_once('@') {
                Some((r, d)) => (r, d.parse::<f32>().map_err(|_| format!("ganho invalido: {item}"))?),
                None => (item, 0.0),
            };
            let (i, o) = route.split_once('>').ok_or(format!("rota invalida: {item}"))?;
            let i: usize = i.trim().parse().map_err(|_| format!("entrada invalida: {item}"))?;
            let o: usize = o.trim().parse().map_err(|_| format!("saida invalida: {item}"))?;
            if i == 0 || o == 0 || i > ins || o > outs {
                return Err(format!("rota fora do limite ({ins} entradas, {outs} saidas): {item}"));
            }
            m.set(i - 1, o - 1, 10f32.powf(db / 20.0));
        }
        Ok(m)
    }

    pub fn clear(&mut self) {
        self.gains.fill(0.0);
    }

    /// Como `parse`, mas ignora rotas invalidas (config antiga, placa trocada).
    /// Sem texto salvo (`None`), devolve o preset 1:1.
    pub fn from_saved(spec: Option<&str>, ins: usize, outs: usize) -> Self {
        let Some(spec) = spec else { return Self::identity(ins, outs) };
        let mut m = Self::new(ins, outs);
        for item in spec.split(',') {
            if let Ok(one) = Self::parse(item, ins, outs) {
                for (g, o) in m.gains.iter_mut().zip(&one.gains) {
                    *g += o;
                }
            }
        }
        m
    }

    /// Pontos ligados: (entrada, saida, ganho em dB), base 0.
    pub fn points(&self) -> Vec<(usize, usize, f32)> {
        let mut v = Vec::new();
        for o in 0..self.outs {
            for i in 0..self.ins {
                let g = self.get(i, o);
                if g != 0.0 {
                    v.push((i, o, (20.0 * g.abs().log10() * 10.0).round() / 10.0));
                }
            }
        }
        v
    }

    /// Texto aceito por `parse`.
    pub fn to_spec(&self) -> String {
        let p: Vec<String> = self.points().iter().map(|(i, o, db)| format!("{}>{}@{}", i + 1, o + 1, db)).collect();
        p.join(",")
    }

    /// `input` e `output` intercalados. `output` e sobrescrito.
    pub fn process(&self, input: &[f32], output: &mut [f32]) {
        let frames = (input.len() / self.ins.max(1)).min(output.len() / self.outs.max(1));
        for f in 0..frames {
            let src = &input[f * self.ins..(f + 1) * self.ins];
            let dst = &mut output[f * self.outs..(f + 1) * self.outs];
            for (o, d) in dst.iter_mut().enumerate() {
                let row = &self.gains[o * self.ins..(o + 1) * self.ins];
                let mut acc = 0.0;
                for (g, s) in row.iter().zip(src) {
                    if *g != 0.0 {
                        acc += g * s;
                    }
                }
                *d = acc;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_and_mix() {
        let mut m = Matrix::identity(3, 2);
        m.set(2, 0, 0.5); // entrada 3 somada na saida 1
        let mut out = [0.0; 4];
        m.process(&[1.0, 2.0, 4.0, 0.0, 1.0, 0.0], &mut out);
        assert_eq!(out, [3.0, 2.0, 0.0, 1.0]);
    }

    #[test]
    fn spec_roundtrip_and_lenient_load() {
        let m = Matrix::parse("1>2,3>1@-6,4>2@3.5", 4, 2).unwrap();
        assert_eq!(m.to_spec(), "3>1@-6,1>2@0,4>2@3.5");
        let back = Matrix::from_saved(Some(&m.to_spec()), 4, 2);
        assert_eq!(back.points(), m.points());
        // placa menor que a da config salva: rotas fora do limite somem
        assert_eq!(Matrix::from_saved(Some("1>1,9>9"), 2, 2).to_spec(), "1>1@0");
        assert_eq!(Matrix::from_saved(None, 2, 3).to_spec(), "1>1@0,2>2@0");
        assert_eq!(Matrix::from_saved(Some(""), 2, 2).to_spec(), "");
    }

    #[test]
    fn parse_spec() {
        let m = Matrix::parse("1>2, 3>1@-6", 4, 2).unwrap();
        assert_eq!(m.get(0, 1), 1.0);
        assert!((m.get(2, 0) - 0.501).abs() < 0.001);
        assert!(Matrix::parse("5>1", 4, 2).is_err());
        assert!(Matrix::parse("1-1", 4, 2).is_err());
    }
}
