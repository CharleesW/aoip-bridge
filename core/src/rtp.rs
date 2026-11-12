//! RTP com carga L24 (PCM 24 bits big-endian), como exige o AES67.

pub const HEADER_LEN: usize = 12;

/// Carimbo RTP derivado da hora PTP (a=mediaclk:direct=0).
pub fn timestamp_from_ptp(ptp_ns: u128, rate: u32) -> u32 {
    ((ptp_ns * rate as u128 / 1_000_000_000) & 0xffff_ffff) as u32
}

/// Monta um pacote: cabecalho + amostras f32 intercaladas convertidas para L24.
pub fn build(out: &mut Vec<u8>, pt: u8, seq: u16, ts: u32, ssrc: u32, samples: &[f32]) {
    out.clear();
    out.push(0x80);
    out.push(pt & 0x7f);
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(&ts.to_be_bytes());
    out.extend_from_slice(&ssrc.to_be_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
        out.extend_from_slice(&v.to_be_bytes()[1..]);
    }
}

pub struct Packet<'a> {
    pub pt: u8,
    pub seq: u16,
    pub ts: u32,
    pub payload: &'a [u8],
}

pub fn parse(buf: &[u8]) -> Option<Packet<'_>> {
    if buf.len() < HEADER_LEN || buf[0] >> 6 != 2 {
        return None;
    }
    let csrc = (buf[0] & 0x0f) as usize;
    let mut start = HEADER_LEN + csrc * 4;
    if buf[0] & 0x10 != 0 {
        // extensao de cabecalho
        let ext = buf.get(start + 2..start + 4)?;
        start += 4 + u16::from_be_bytes([ext[0], ext[1]]) as usize * 4;
    }
    let mut end = buf.len();
    if buf[0] & 0x20 != 0 {
        end = end.checked_sub(*buf.last()? as usize)?;
    }
    Some(Packet {
        pt: buf[1] & 0x7f,
        seq: u16::from_be_bytes([buf[2], buf[3]]),
        ts: u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]),
        payload: buf.get(start..end)?,
    })
}

/// Decodifica L24 ou L16 e acrescenta em `out` (f32 intercalado).
pub fn decode(payload: &[u8], bits: u8, out: &mut impl Extend<f32>) {
    match bits {
        24 => out.extend(payload.chunks_exact(3).map(|b| {
            (i32::from_be_bytes([b[0], b[1], b[2], 0]) >> 8) as f32 / 8_388_608.0
        })),
        16 => out.extend(
            payload.chunks_exact(2).map(|b| i16::from_be_bytes([b[0], b[1]]) as f32 / 32_768.0),
        ),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let src = [0.0, 0.5, -0.5, 1.0, -1.0, 0.123456];
        let mut pkt = Vec::new();
        build(&mut pkt, 97, 65535, 0xdead_beef, 7, &src);
        assert_eq!(pkt.len(), 12 + 18);
        let p = parse(&pkt).unwrap();
        assert_eq!((p.pt, p.seq, p.ts), (97, 65535, 0xdead_beef));
        let mut out = Vec::new();
        decode(p.payload, 24, &mut out);
        for (a, b) in src.iter().zip(&out) {
            assert!((a - b).abs() < 1e-6, "{a} vs {b}");
        }
    }

    #[test]
    fn ptp_timestamp_wraps() {
        assert_eq!(timestamp_from_ptp(1_000_000_000, 48_000), 48_000);
        let big = (u32::MAX as u128 + 10) * 1_000_000_000 / 48_000 + 20_834;
        assert_eq!(timestamp_from_ptp(big, 48_000), 10);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(&[0u8; 4]).is_none());
        assert!(parse(&[0u8; 20]).is_none()); // versao 0
    }
}
