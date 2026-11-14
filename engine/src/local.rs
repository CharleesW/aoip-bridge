//! Ligacao local entre a placa virtual (driver ASIO, dentro do programa de audio) e o
//! programa AOIP, que fica sempre ligado cuidando da rede. TCP em 127.0.0.1: a cada bloco
//! o driver manda o que o programa de audio tocou (vai para a rede) e recebe de volta o que
//! chegou da rede. So um programa de audio por vez usa a placa.
//!
//! Mensagens: cabecalho de 20 bytes (magic, tipo, a, b, c, em little-endian), seguido dos
//! dados quando houver.
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;
use std::time::Duration;

pub const PORT: u16 = 48790;
const MAGIC: u32 = u32::from_le_bytes(*b"AOIV");
const QUERY: u32 = 1; // pergunta o formato e fecha
const AUDIO: u32 = 2; // pergunta o formato e fica para o audio
const BLOCK: u32 = 3; // a = quadros, b = canais, c = geracao (na resposta)
const SET_RATE: u32 = 4; // a = taxa
const CONFIG: u32 = 10; // a = canais, b = taxa, c = bloco; mais 8 bytes: geracao, latencia
const BUSY: u32 = 11;
const MAX_FRAMES: u32 = 8192;
const MAX_CHANNELS: u32 = 64;

/// Formato da placa virtual, escolhido no programa AOIP.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardInfo {
    pub channels: usize,
    pub rate: u32,
    /// Bloco sugerido ao programa de audio, em amostras.
    pub buffer: usize,
    /// Latencia que a rede acrescenta (folga), em amostras.
    pub latency: usize,
    /// Muda a cada mudanca de formato: o driver pede ao programa de audio para reabrir a placa.
    pub generation: u32,
}

/// Quem atende a placa (o programa AOIP).
pub trait CardHost: Send + Sync + 'static {
    fn info(&self) -> CardInfo;
    /// `to_net`: o que o programa de audio tocou; `from_net`: preencher com o que chegou da
    /// rede. Os dois intercalados, com `channels` canais.
    fn process(&self, to_net: &[f32], from_net: &mut [f32], channels: usize);
    /// O programa de audio pediu outra taxa (44100 ou 48000).
    fn set_rate(&self, rate: u32);
}

fn header(kind: u32, a: u32, b: u32, c: u32) -> [u8; 20] {
    let mut h = [0u8; 20];
    for (i, v) in [MAGIC, kind, a, b, c].into_iter().enumerate() {
        h[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    h
}

fn read_header(s: &mut TcpStream) -> io::Result<(u32, u32, u32, u32)> {
    let mut h = [0u8; 20];
    s.read_exact(&mut h)?;
    let w = |i: usize| u32::from_le_bytes(h[i * 4..i * 4 + 4].try_into().unwrap());
    if w(0) != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "mensagem desconhecida"));
    }
    Ok((w(1), w(2), w(3), w(4)))
}

fn write_config(s: &mut TcpStream, i: &CardInfo) -> io::Result<()> {
    let mut m = header(CONFIG, i.channels as u32, i.rate, i.buffer as u32).to_vec();
    m.extend_from_slice(&i.generation.to_le_bytes());
    m.extend_from_slice(&(i.latency as u32).to_le_bytes());
    s.write_all(&m)
}

fn read_config(s: &mut TcpStream) -> io::Result<CardInfo> {
    let (kind, a, b, c) = read_header(s)?;
    if kind == BUSY {
        return Err(io::Error::new(io::ErrorKind::AddrInUse, "a placa AoIP Virtual ja esta aberta em outro programa de audio"));
    }
    if kind != CONFIG {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "resposta inesperada"));
    }
    let mut extra = [0u8; 8];
    s.read_exact(&mut extra)?;
    Ok(CardInfo {
        channels: a as usize, rate: b, buffer: c as usize,
        generation: u32::from_le_bytes(extra[..4].try_into().unwrap()),
        latency: u32::from_le_bytes(extra[4..].try_into().unwrap()) as usize,
    })
}

fn floats_to_bytes(f: &[f32], out: &mut Vec<u8>) {
    out.clear();
    for v in f {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

fn bytes_to_floats(b: &[u8], out: &mut [f32]) {
    for (o, c) in out.iter_mut().zip(b.chunks_exact(4)) {
        *o = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
    }
}

// ---------------------------------------------------------------- lado do programa AOIP

/// Atende os drivers em `listener` (127.0.0.1:PORT) para sempre, em threads proprias.
pub fn serve(host: Arc<dyn CardHost>, listener: TcpListener) {
    let busy = Arc::new(AtomicBool::new(false));
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let (host, busy) = (host.clone(), busy.clone());
            std::thread::spawn(move || {
                let _ = conn.set_nodelay(true);
                let _ = handle(conn, &*host, &busy);
            });
        }
    });
}

fn handle(mut s: TcpStream, host: &dyn CardHost, busy: &AtomicBool) -> io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    let (kind, a, _, _) = read_header(&mut s)?;
    match kind {
        QUERY => return write_config(&mut s, &host.info()),
        SET_RATE => {
            host.set_rate(a);
            return write_config(&mut s, &host.info());
        }
        AUDIO => {}
        _ => return Ok(()),
    }
    if busy.swap(true, Relaxed) {
        return s.write_all(&header(BUSY, 0, 0, 0));
    }
    struct Release<'a>(&'a AtomicBool);
    impl Drop for Release<'_> {
        fn drop(&mut self) {
            self.0.store(false, Relaxed);
        }
    }
    let _release = Release(busy);
    write_config(&mut s, &host.info())?;
    // Com o programa de audio parado (pausa, configuracao) nao chegam blocos: espera sem
    // limite e solta a placa quando a ligacao fecha.
    s.set_read_timeout(None)?;
    let (mut raw, mut to_net, mut from_net, mut out) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    loop {
        let (kind, frames, channels, _) = read_header(&mut s)?;
        if kind != BLOCK || frames > MAX_FRAMES || channels == 0 || channels > MAX_CHANNELS {
            return Ok(());
        }
        let n = (frames * channels) as usize;
        raw.resize(n * 4, 0);
        s.read_exact(&mut raw)?;
        to_net.resize(n, 0.0);
        bytes_to_floats(&raw, &mut to_net);
        from_net.clear();
        from_net.resize(n, 0.0);
        host.process(&to_net, &mut from_net, channels as usize);
        floats_to_bytes(&from_net, &mut out);
        let mut m = header(BLOCK, frames, channels, host.info().generation).to_vec();
        m.extend_from_slice(&out);
        s.write_all(&m)?;
    }
}

// ---------------------------------------------------------------- lado do driver

fn open(timeout: Duration) -> io::Result<TcpStream> {
    let s = TcpStream::connect_timeout(&([127, 0, 0, 1], PORT).into(), timeout)?;
    s.set_nodelay(true)?;
    s.set_read_timeout(Some(Duration::from_secs(2)))?;
    Ok(s)
}

/// Pergunta o formato da placa (sem ocupa-la).
pub fn query() -> io::Result<CardInfo> {
    let mut s = open(Duration::from_millis(500))?;
    s.write_all(&header(QUERY, 0, 0, 0))?;
    read_config(&mut s)
}

/// Pede outra taxa de amostragem ao programa AOIP. Devolve o formato novo.
pub fn set_rate(rate: u32) -> io::Result<CardInfo> {
    let mut s = open(Duration::from_millis(500))?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    s.write_all(&header(SET_RATE, rate, 0, 0))?;
    read_config(&mut s)
}

/// Placa ocupada por este programa de audio.
pub struct CardClient {
    s: TcpStream,
    pub info: CardInfo,
    raw: Vec<u8>,
}

impl CardClient {
    pub fn connect() -> io::Result<Self> {
        let mut s = open(Duration::from_millis(500))?;
        s.write_all(&header(AUDIO, 0, 0, 0))?;
        let info = read_config(&mut s)?;
        Ok(Self { s, info, raw: Vec::new() })
    }

    /// Troca um bloco. Devolve a geracao atual do formato (se mudou, o programa de audio
    /// deve reabrir a placa).
    pub fn exchange(&mut self, to_net: &[f32], from_net: &mut [f32], channels: usize) -> io::Result<u32> {
        let frames = to_net.len() / channels.max(1);
        floats_to_bytes(to_net, &mut self.raw);
        let mut m = header(BLOCK, frames as u32, channels as u32, 0).to_vec();
        m.extend_from_slice(&self.raw);
        self.s.write_all(&m)?;
        let (kind, f, c, generation) = read_header(&mut self.s)?;
        if kind != BLOCK || f as usize != frames || c as usize != channels {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "bloco inesperado"));
        }
        self.raw.resize(frames * channels * 4, 0);
        self.s.read_exact(&mut self.raw)?;
        bytes_to_floats(&self.raw, from_net);
        Ok(generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
    use std::sync::Mutex;

    /// Placa de mentira: devolve o que recebeu, multiplicado por 2 (como se a rede
    /// devolvesse), e guarda a taxa pedida.
    struct Fake {
        generation: AtomicU32,
        rate: Mutex<u32>,
    }
    impl CardHost for Fake {
        fn info(&self) -> CardInfo {
            CardInfo { channels: 4, rate: *self.rate.lock().unwrap(), buffer: 128, latency: 192, generation: self.generation.load(Relaxed) }
        }
        fn process(&self, to_net: &[f32], from_net: &mut [f32], _channels: usize) {
            for (o, i) in from_net.iter_mut().zip(to_net) {
                *o = i * 2.0;
            }
        }
        fn set_rate(&self, rate: u32) {
            *self.rate.lock().unwrap() = rate;
            self.generation.fetch_add(1, Relaxed);
        }
    }

    #[test]
    fn driver_and_program_talk() {
        // A porta fixa pode estar em uso por um programa AOIP de verdade: testa noutra.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let fake = Arc::new(Fake { generation: AtomicU32::new(7), rate: Mutex::new(48000) });
        serve(fake.clone(), listener);
        let connect = |kind| {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(&header(kind, 0, 0, 0)).unwrap();
            s
        };
        // pergunta o formato
        let mut q = connect(QUERY);
        let info = read_config(&mut q).unwrap();
        assert_eq!((info.channels, info.rate, info.buffer, info.latency, info.generation), (4, 48000, 128, 192, 7));
        // ocupa a placa e troca blocos
        let mut s = connect(AUDIO);
        let info = read_config(&mut s).unwrap();
        let mut c = CardClient { s, info, raw: Vec::new() };
        let to_net: Vec<f32> = (0..4 * 64).map(|i| i as f32 / 1000.0).collect();
        let mut from_net = vec![0.0; 4 * 64];
        assert_eq!(c.exchange(&to_net, &mut from_net, 4).unwrap(), 7);
        assert_eq!(from_net[10], to_net[10] * 2.0);
        // segundo programa de audio: placa ocupada
        let mut other = connect(AUDIO);
        assert_eq!(read_config(&mut other).unwrap_err().kind(), io::ErrorKind::AddrInUse);
        // troca de taxa: a geracao muda e o driver fica sabendo no proximo bloco
        let mut r = TcpStream::connect(("127.0.0.1", port)).unwrap();
        r.write_all(&header(SET_RATE, 44100, 0, 0)).unwrap();
        assert_eq!(read_config(&mut r).unwrap().rate, 44100);
        assert_eq!(c.exchange(&to_net, &mut from_net, 4).unwrap(), 8);
        // fechou: a placa fica livre de novo
        drop(c);
        std::thread::sleep(Duration::from_millis(100));
        let mut again = connect(AUDIO);
        assert!(read_config(&mut again).is_ok());
    }
}
