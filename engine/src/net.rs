//! Sockets multicast. SO_REUSEADDR/SO_REUSEPORT permitem conviver com o
//! Dante Controller na mesma maquina (ele tambem escuta SAP na porta 9875).
use socket2::{Domain, Protocol, Socket, Type};
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};

pub fn listener(group: Ipv4Addr, port: u16, iface: Ipv4Addr) -> io::Result<UdpSocket> {
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.set_reuse_address(true)?;
    #[cfg(unix)]
    s.set_reuse_port(true)?;
    s.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port).into())?;
    s.join_multicast_v4(&group, &iface)?;
    Ok(s.into())
}

/// `local_only` (autoteste): TTL 0, os pacotes nunca saem para a rede. O "loop" fica sempre
/// ligado: outro programa deste mesmo computador (ex.: o AoIP Bridge com a mesa e a placa
/// virtual) precisa ouvir o relogio e os fluxos deste. Os proprios ecos sao descartados.
pub fn sender(iface: Ipv4Addr, local_only: bool) -> io::Result<UdpSocket> {
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.bind(&SocketAddrV4::new(iface, 0).into())?;
    s.set_multicast_if_v4(&iface)?;
    s.set_multicast_ttl_v4(if local_only { 0 } else { 16 })?;
    s.set_multicast_loop_v4(true)?;
    // DSCP EF (46) para audio, como recomenda o AES67. Falha e ignorada
    // (o Windows so marca DSCP via politica de QoS).
    let _ = s.set_tos(46 << 2);
    Ok(s.into())
}

/// IP local usado para alcancar a rede (palpite quando --iface nao e informado).
pub fn guess_iface() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("239.255.255.255:9875").ok()?;
    match s.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

// ---------------------------------------------------------------- placas de rede e nomes

use aoip_core::state::NetIface;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Placas de rede ligadas, com IPv4 e multicast (sem a de "loopback").
pub fn interfaces() -> Vec<NetIface> {
    let mut list = raw_interfaces();
    list.sort_by(|a, b| a.label.cmp(&b.label));
    list.dedup_by(|a, b| a.id == b.id && a.ip == b.ip);
    list
}

#[cfg(unix)]
fn raw_interfaces() -> Vec<NetIface> {
    let mut out = Vec::new();
    // SAFETY: getifaddrs devolve uma lista ligada que so e lida aqui e liberada no fim.
    unsafe {
        let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut head) != 0 {
            return out;
        }
        let mut p = head;
        while !p.is_null() {
            let a = &*p;
            p = a.ifa_next;
            if a.ifa_addr.is_null() || (*a.ifa_addr).sa_family as i32 != libc::AF_INET {
                continue;
            }
            let flags = a.ifa_flags as i32;
            if flags & libc::IFF_UP == 0 || flags & libc::IFF_LOOPBACK != 0 || flags & libc::IFF_MULTICAST == 0 {
                continue;
            }
            let sin = &*(a.ifa_addr as *const libc::sockaddr_in);
            let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
            let id = std::ffi::CStr::from_ptr(a.ifa_name).to_string_lossy().into_owned();
            let label = match port_names().get(&id) {
                Some(port) => format!("{port} ({id})"),
                None => id.clone(),
            };
            out.push(NetIface { id, label, ip });
        }
        libc::freeifaddrs(head);
    }
    out
}

/// macOS: nome amigavel de cada placa ("en0" -> "Wi-Fi"), lido uma vez do networksetup.
#[cfg(unix)]
fn port_names() -> &'static HashMap<String, String> {
    static NAMES: std::sync::OnceLock<HashMap<String, String>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| {
        let mut map = HashMap::new();
        #[cfg(target_os = "macos")]
        if let Ok(o) = std::process::Command::new("/usr/sbin/networksetup").arg("-listallhardwareports").output() {
            let text = String::from_utf8_lossy(&o.stdout);
            let mut port = None;
            for line in text.lines() {
                if let Some(p) = line.strip_prefix("Hardware Port: ") {
                    port = Some(p.trim().to_string());
                } else if let (Some(d), Some(p)) = (line.strip_prefix("Device: "), port.take()) {
                    map.insert(d.trim().to_string(), p);
                }
            }
        }
        map
    })
}

#[cfg(windows)]
fn raw_interfaces() -> Vec<NetIface> {
    use windows_sys::Win32::NetworkManagement::IpHelper::*;
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows_sys::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};
    const ERROR_BUFFER_OVERFLOW: u32 = 111;
    const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;
    let wide = |p: *const u16| -> String {
        if p.is_null() {
            return String::new();
        }
        // SAFETY: texto UTF-16 terminado em zero, vindo do Windows.
        unsafe {
            let len = (0..).take_while(|&i| *p.add(i) != 0).count();
            String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
        }
    };
    let mut out = Vec::new();
    let mut size: u32 = 32 * 1024;
    for _ in 0..4 {
        let mut buf = vec![0u64; size as usize / 8 + 1]; // alinhado em 8 bytes
        let first = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        // SAFETY: o buffer tem `size` bytes; a lista so e lida enquanto `buf` existe.
        let r = unsafe { GetAdaptersAddresses(AF_INET as u32, flags, std::ptr::null(), first, &mut size) };
        if r == ERROR_BUFFER_OVERFLOW {
            continue;
        }
        if r != 0 {
            return out;
        }
        let mut p = first;
        while !p.is_null() {
            // SAFETY: elementos da lista preenchida acima.
            unsafe {
                let a = &*p;
                p = a.Next;
                if a.OperStatus != IfOperStatusUp || a.IfType == IF_TYPE_SOFTWARE_LOOPBACK {
                    continue;
                }
                let (id, desc) = (wide(a.FriendlyName), wide(a.Description));
                let label = if desc.is_empty() || desc == id { id.clone() } else { format!("{id} ({desc})") };
                let mut u = a.FirstUnicastAddress;
                while !u.is_null() {
                    let ua = &*u;
                    u = ua.Next;
                    let sa = ua.Address.lpSockaddr;
                    if sa.is_null() || (*sa).sa_family != AF_INET {
                        continue;
                    }
                    let sin = &*(sa as *const SOCKADDR_IN);
                    let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.S_un.S_addr));
                    out.push(NetIface { id: id.clone(), label: label.clone(), ip });
                }
            }
        }
        return out;
    }
    out
}

#[cfg(not(any(unix, windows)))]
fn raw_interfaces() -> Vec<NetIface> {
    Vec::new()
}

/// Nome deste computador na rede ("PC-IGREJA", "MacBook-Pro-de-Fulano").
pub fn computer_name() -> String {
    #[cfg(windows)]
    if let Ok(n) = std::env::var("COMPUTERNAME") {
        return n;
    }
    #[cfg(target_os = "macos")]
    if let Ok(o) = std::process::Command::new("/usr/sbin/scutil").args(["--get", "LocalHostName"]).output() {
        let n = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if o.status.success() && !n.is_empty() {
            return n;
        }
    }
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: o buffer tem o tamanho informado.
        if unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) } == 0 {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            return String::from_utf8_lossy(&buf[..end]).split('.').next().unwrap_or("").to_string();
        }
    }
    String::new()
}

/// Nome de outro computador pelo IP (DNS reverso, NetBIOS ou mDNS, conforme o sistema).
/// Nao trava: na primeira vez devolve None e procura em segundo plano; o resultado fica
/// guardado por 2 minutos.
pub fn name_of(ip: Ipv4Addr) -> Option<String> {
    static CACHE: Mutex<Option<HashMap<Ipv4Addr, (Instant, Option<String>)>>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    match cache.get(&ip) {
        Some((when, name)) if when.elapsed() < Duration::from_secs(120) => return name.clone(),
        _ => {}
    }
    let old = cache.get(&ip).and_then(|(_, n)| n.clone());
    cache.insert(ip, (Instant::now(), old.clone())); // marca como "procurando"
    drop(guard);
    std::thread::spawn(move || {
        let found = reverse_lookup(ip);
        let mut guard = CACHE.lock().unwrap_or_else(|p| p.into_inner());
        guard.get_or_insert_with(HashMap::new).insert(ip, (Instant::now(), found));
    });
    old
}

fn short_host(name: &str) -> Option<String> {
    let name = name.trim();
    if name.parse::<Ipv4Addr>().is_ok() {
        return None; // sem nome: o sistema devolveu o proprio IP
    }
    let first = name.split('.').next().unwrap_or("").trim();
    (!first.is_empty()).then(|| first.to_string())
}

#[cfg(unix)]
fn reverse_lookup(ip: Ipv4Addr) -> Option<String> {
    // SAFETY: estrutura sockaddr_in preenchida e buffer com o tamanho informado.
    unsafe {
        let mut sin: libc::sockaddr_in = std::mem::zeroed();
        sin.sin_family = libc::AF_INET as _;
        sin.sin_addr.s_addr = u32::from(ip).to_be();
        #[cfg(target_os = "macos")]
        {
            sin.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
        }
        let mut host = [0 as libc::c_char; 256];
        let r = libc::getnameinfo(
            &sin as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            host.as_mut_ptr(), host.len() as _, std::ptr::null_mut(), 0, libc::NI_NAMEREQD,
        );
        if r != 0 {
            return None;
        }
        short_host(&std::ffi::CStr::from_ptr(host.as_ptr()).to_string_lossy())
    }
}

#[cfg(windows)]
fn reverse_lookup(ip: Ipv4Addr) -> Option<String> {
    use windows_sys::Win32::Networking::WinSock::{getnameinfo, AF_INET, NI_NAMEREQD, SOCKADDR, SOCKADDR_IN};
    let _ = std::net::UdpSocket::bind("0.0.0.0:0"); // garante a rede do Windows (WSAStartup) iniciada
    // SAFETY: estrutura SOCKADDR_IN preenchida e buffer com o tamanho informado.
    unsafe {
        let mut sin: SOCKADDR_IN = std::mem::zeroed();
        sin.sin_family = AF_INET;
        sin.sin_addr.S_un.S_addr = u32::from(ip).to_be();
        let mut host = [0u8; 256];
        let r = getnameinfo(
            &sin as *const _ as *const SOCKADDR, std::mem::size_of::<SOCKADDR_IN>() as i32,
            host.as_mut_ptr(), host.len() as u32, std::ptr::null_mut(), 0, NI_NAMEREQD as i32,
        );
        if r != 0 {
            return None;
        }
        let end = host.iter().position(|&b| b == 0).unwrap_or(host.len());
        short_host(&String::from_utf8_lossy(&host[..end]))
    }
}

#[cfg(not(any(unix, windows)))]
fn reverse_lookup(_ip: Ipv4Addr) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_this_computer() {
        let list = interfaces();
        // Este computador tem pelo menos uma rede ligada com IPv4 (o teste roda com rede).
        assert!(list.iter().all(|i| !i.id.is_empty() && !i.ip.is_loopback()), "{list:?}");
        assert!(!computer_name().is_empty());
        assert_eq!(short_host("PC-IGREJA.local").as_deref(), Some("PC-IGREJA"));
        assert_eq!(short_host("192.168.0.4"), None);
    }
}
