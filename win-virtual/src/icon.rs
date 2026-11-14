//! Icone do AoIP Virtual (o simbolo da marca, assets/brand/tray-icon.svg) desenhado em
//! pixels, sem arquivo de imagem: traco claro com as duas pontas e o no verde no meio.

/// Pixels BGRA (de cima para baixo) de um icone `size` x `size`. `light`: barra de tarefas
/// clara (traco escuro #171B21 em vez do claro #E8ECF0).
pub fn bgra(size: usize, light: bool) -> Vec<u8> {
    let ink: [f32; 3] = if light { [0x21 as f32, 0x1B as f32, 0x17 as f32] } else { [0xF0 as f32, 0xEC as f32, 0xE8 as f32] }; // B, G, R
    // Curva do simbolo em unidades de 24 x 24: M4 18 H8 C12.5 18 11.5 6 16 6 H20.
    let mut line: Vec<(f32, f32)> = vec![(4.0, 18.0), (8.0, 18.0)];
    for k in 1..=24 {
        let t = k as f32 / 24.0;
        let u = 1.0 - t;
        let x = u * u * u * 8.0 + 3.0 * u * u * t * 12.5 + 3.0 * u * t * t * 11.5 + t * t * t * 16.0;
        let y = u * u * u * 18.0 + 3.0 * u * u * t * 18.0 + 3.0 * u * t * t * 6.0 + t * t * t * 6.0;
        line.push((x, y));
    }
    line.push((20.0, 6.0));
    let near_line = |px: f32, py: f32| {
        line.windows(2).any(|w| {
            let ((ax, ay), (bx, by)) = (w[0], w[1]);
            let (dx, dy) = (bx - ax, by - ay);
            let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
            let (cx, cy) = (ax + t * dx - px, ay + t * dy - py);
            cx * cx + cy * cy <= 1.2 * 1.2 // meia espessura do traco (um pouco mais grossa que a do SVG, para ficar nitido pequeno)
        })
    };
    let dist = |px: f32, py: f32, cx: f32, cy: f32| ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
    let mut out = vec![0u8; size * size * 4];
    const SS: usize = 4; // amostras por lado em cada pixel (bordas suaves)
    for y in 0..size {
        for x in 0..size {
            let (mut ink_n, mut green) = (0usize, 0usize);
            for sy in 0..SS {
                for sx in 0..SS {
                    let px = (x as f32 + (sx as f32 + 0.5) / SS as f32) * 24.0 / size as f32;
                    let py = (y as f32 + (sy as f32 + 0.5) / SS as f32) * 24.0 / size as f32;
                    if dist(px, py, 12.0, 12.0) <= 3.2 {
                        green += 1;
                    } else if near_line(px, py) || dist(px, py, 4.0, 18.0) <= 2.8 || dist(px, py, 20.0, 6.0) <= 2.8 {
                        ink_n += 1;
                    }
                }
            }
            let n = (SS * SS) as f32;
            let a = (ink_n + green) as f32 / n;
            let i = (y * size + x) * 4;
            if a > 0.0 {
                // mistura das duas cores pela cobertura: o traco e o verde #26A88B
                let (wi, wg) = (ink_n as f32 / (ink_n + green) as f32, green as f32 / (ink_n + green) as f32);
                let mix = |c1: f32, c2: f32| (c1 * wi + c2 * wg).round() as u8;
                out[i] = mix(ink[0], 0x8B as f32); // B
                out[i + 1] = mix(ink[1], 0xA8 as f32); // G
                out[i + 2] = mix(ink[2], 0x26 as f32); // R
                out[i + 3] = (a * 255.0).round() as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn draws_the_symbol() {
        let p = super::bgra(16, false);
        assert_ne!(super::bgra(16, true), p, "barra clara usa traco escuro");
        assert_eq!(p.len(), 16 * 16 * 4);
        let alpha = |x: usize, y: usize| p[(y * 16 + x) * 4 + 3];
        let green = |x: usize, y: usize| p[(y * 16 + x) * 4 + 1] > p[(y * 16 + x) * 4 + 2];
        assert!(alpha(8, 8) > 200 && green(8, 8), "no verde no centro");
        assert!(alpha(2, 12) > 150, "ponta de baixo, a esquerda");
        assert!(alpha(13, 4) > 150, "ponta de cima, a direita");
        assert_eq!(alpha(0, 0), 0, "canto transparente");
        assert_eq!(alpha(15, 15), 0);
    }
}
