//! Imita um programa de audio (REAPER) usando a placa virtual que esta ligada NESTE computador:
//! manda silencio por alguns segundos, no ritmo certo, e fecha. Serve para ver o envio sair da
//! pausa e voltar a ela (veja /api/diag).   programa [segundos]
use aoip_engine::local::{self, CardClient};
use std::time::{Duration, Instant};

fn main() {
    let info = local::query().expect("a placa virtual esta ligada?");
    let mut drv = CardClient::connect().expect("a placa ja esta em uso?");
    let secs: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(6);
    let (ch, frames) = (info.channels, info.buffer.max(64));
    let (out, mut inp) = (vec![0f32; frames * ch], vec![0f32; frames * ch]);
    let (start, mut n) = (Instant::now(), 0u64);
    while start.elapsed() < Duration::from_secs(secs) {
        drv.exchange(&out, &mut inp, ch).expect("a placa fechou");
        n += 1;
        let due = start + Duration::from_nanos(n * frames as u64 * 1_000_000_000 / info.rate as u64);
        std::thread::sleep(due.saturating_duration_since(Instant::now()));
    }
    println!("{} canais, {} Hz, bloco {frames}: {n} blocos", ch, info.rate);
}
