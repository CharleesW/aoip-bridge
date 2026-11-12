//! SDP no formato que o Dante Controller aceita para fluxos AES67.
use std::net::Ipv4Addr;

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub name: String,
    pub origin: Ipv4Addr,
    pub session_id: u64,
    pub group: Ipv4Addr,
    pub port: u16,
    pub pt: u8,
    pub channels: u8,
    pub rate: u32,
    pub bits: u8,
    /// Identidade do grandmaster PTP e dominio (a=ts-refclk).
    pub refclk: Option<([u8; 8], u8)>,
}

impl Session {
    pub fn to_sdp(&self) -> String {
        let mut s = format!(
            "v=0\r\no=- {id} {id} IN IP4 {o}\r\ns={n}\r\nc=IN IP4 {g}/32\r\nt=0 0\r\n\
             m=audio {p} RTP/AVP {pt}\r\na=rtpmap:{pt} L{b}/{r}/{ch}\r\na=ptime:1\r\n",
            id = self.session_id, o = self.origin, n = self.name, g = self.group,
            p = self.port, pt = self.pt, b = self.bits, r = self.rate, ch = self.channels,
        );
        if let Some((gm, dom)) = self.refclk {
            let id: Vec<String> = gm.iter().map(|b| format!("{b:02X}")).collect();
            s += &format!("a=ts-refclk:ptp=IEEE1588-2008:{}:{}\r\n", id.join("-"), dom);
        }
        s += "a=mediaclk:direct=0\r\n";
        s
    }

    pub fn parse(text: &str) -> Option<Session> {
        let mut s = Session {
            name: String::new(), origin: Ipv4Addr::UNSPECIFIED, session_id: 0,
            group: Ipv4Addr::UNSPECIFIED, port: 0, pt: 0, channels: 0, rate: 0, bits: 0,
            refclk: None,
        };
        for line in text.lines().map(str::trim) {
            let Some((k, v)) = line.split_once('=') else { continue };
            match k {
                "o" => {
                    let f: Vec<&str> = v.split_whitespace().collect();
                    s.session_id = f.get(1)?.parse().unwrap_or(0);
                    s.origin = f.get(5)?.parse().ok()?;
                }
                "s" => s.name = v.to_string(),
                "c" => {
                    let addr = v.split_whitespace().nth(2)?;
                    s.group = addr.split('/').next()?.parse().ok()?;
                }
                "m" => {
                    let f: Vec<&str> = v.split_whitespace().collect();
                    if f.first() != Some(&"audio") {
                        return None;
                    }
                    s.port = f.get(1)?.split('/').next()?.parse().ok()?;
                    s.pt = f.get(3)?.parse().ok()?;
                }
                "a" => {
                    if let Some(map) = v.strip_prefix("rtpmap:") {
                        let (pt, fmt) = map.split_once(' ')?;
                        if pt.parse::<u8>().ok() != Some(s.pt) {
                            continue;
                        }
                        let mut p = fmt.split('/');
                        s.bits = p.next()?.strip_prefix('L')?.parse().ok()?;
                        s.rate = p.next()?.parse().ok()?;
                        s.channels = p.next().unwrap_or("1").parse().ok()?;
                    }
                }
                _ => {}
            }
        }
        (s.port != 0 && s.channels != 0 && !s.group.is_unspecified()).then_some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = Session {
            name: "Ui24R : 1".into(), origin: "192.168.1.50".parse().unwrap(), session_id: 42,
            group: "239.69.1.50".parse().unwrap(), port: 5004, pt: 97, channels: 8,
            rate: 48_000, bits: 24, refclk: Some(([0, 0x1d, 0xc1, 0xff, 0xfe, 1, 2, 3], 0)),
        };
        let text = s.to_sdp();
        assert!(text.contains("a=ts-refclk:ptp=IEEE1588-2008:00-1D-C1-FF-FE-01-02-03:0"));
        let mut back = Session::parse(&text).unwrap();
        back.refclk = s.refclk;
        assert_eq!(back, s);
    }

    #[test]
    fn parses_dante_style() {
        // Formato tipico de um fluxo AES67 criado no Dante Controller.
        let text = "v=0\r\no=- 1311738121 1311738121 IN IP4 192.168.1.20\r\ns=MESA : 2\r\n\
            c=IN IP4 239.69.83.67/32\r\nt=0 0\r\na=keywds:Dante\r\n\
            m=audio 5004 RTP/AVP 97\r\ni=2 channels: Left, Right\r\na=recvonly\r\n\
            a=rtpmap:97 L24/48000/2\r\na=ptime:1\r\n\
            a=ts-refclk:ptp=IEEE1588-2008:00-1D-C1-FF-FE-12-34-56:0\r\na=mediaclk:direct=0\r\n";
        let s = Session::parse(text).unwrap();
        assert_eq!(s.name, "MESA : 2");
        assert_eq!(s.group, "239.69.83.67".parse::<Ipv4Addr>().unwrap());
        assert_eq!((s.port, s.pt, s.bits, s.rate, s.channels), (5004, 97, 24, 48_000, 2));
    }

    #[test]
    fn rejects_non_audio() {
        assert!(Session::parse("v=0\r\nm=video 5004 RTP/AVP 96\r\n").is_none());
    }
}
