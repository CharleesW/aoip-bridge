//! SAP (RFC 2974): anuncio dos fluxos. E por aqui que o Dante Controller nos descobre.
use std::net::Ipv4Addr;

pub const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 255);
pub const PORT: u16 = 9875;
const MIME: &[u8] = b"application/sdp\0";

pub fn build(delete: bool, hash: u16, origin: Ipv4Addr, sdp: &str) -> Vec<u8> {
    let mut p = vec![if delete { 0x24 } else { 0x20 }, 0];
    p.extend_from_slice(&hash.to_be_bytes());
    p.extend_from_slice(&origin.octets());
    p.extend_from_slice(MIME);
    p.extend_from_slice(sdp.as_bytes());
    p
}

pub struct Announcement<'a> {
    pub delete: bool,
    pub hash: u16,
    pub origin: Ipv4Addr,
    pub sdp: &'a str,
}

pub fn parse(buf: &[u8]) -> Option<Announcement<'_>> {
    if buf.len() < 8 || buf[0] >> 5 != 1 || buf[0] & 0x13 != 0 {
        return None; // versao errada, IPv6, cifrado ou comprimido
    }
    let mut body = buf.get(8 + buf[1] as usize * 4..)?;
    if !body.starts_with(b"v=0") {
        let nul = body.iter().position(|b| *b == 0)?;
        if &body[..nul] != b"application/sdp" {
            return None;
        }
        body = &body[nul + 1..];
    }
    Some(Announcement {
        delete: buf[0] & 0x04 != 0,
        hash: u16::from_be_bytes([buf[2], buf[3]]),
        origin: Ipv4Addr::new(buf[4], buf[5], buf[6], buf[7]),
        sdp: std::str::from_utf8(body).ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let ip = Ipv4Addr::new(10, 0, 0, 9);
        let p = build(false, 0xabcd, ip, "v=0\r\ns=x\r\n");
        let a = parse(&p).unwrap();
        assert!(!a.delete);
        assert_eq!((a.hash, a.origin, a.sdp), (0xabcd, ip, "v=0\r\ns=x\r\n"));
        assert!(parse(&build(true, 1, ip, "v=0")).unwrap().delete);
    }

    #[test]
    fn without_mime_and_garbage() {
        let mut p = vec![0x20, 0, 0, 1, 1, 2, 3, 4];
        p.extend_from_slice(b"v=0\r\n");
        assert_eq!(parse(&p).unwrap().sdp, "v=0\r\n");
        assert!(parse(&[0x20, 9, 0, 0]).is_none());
        assert!(parse(&[0x30, 0, 0, 1, 1, 2, 3, 4, b'v']).is_none()); // IPv6
    }
}
