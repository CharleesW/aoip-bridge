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

/// `local_only` (autoteste): TTL 0 e loop ligado, os pacotes voltam para este
/// computador e nunca saem para a rede.
pub fn sender(iface: Ipv4Addr, local_only: bool) -> io::Result<UdpSocket> {
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.bind(&SocketAddrV4::new(iface, 0).into())?;
    s.set_multicast_if_v4(&iface)?;
    s.set_multicast_ttl_v4(if local_only { 0 } else { 16 })?;
    s.set_multicast_loop_v4(local_only)?;
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
