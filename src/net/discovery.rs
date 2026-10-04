//! Descubrimiento de dispositivos de la subred **sin privilegios**: se envía un datagrama UDP
//! (puerto 9, «discard») a cada IP para que el kernel resuelva su MAC por ARP y después se lee la
//! tabla de vecinos de `/proc/net/arp`. No requiere root ni `CAP_NET_RAW`.
//!
//! Limitación del método sin privilegios: el kernel puede conservar unos minutos la entrada de un equipo
//! que ya se apagó. Con `CAP_NET_RAW` se usa en su lugar el ARP propio de `arp.rs`, que es exacto.

use super::iface::IfaceInfo;
use std::net::{Ipv4Addr, UdpSocket};
use std::time::Duration;

/// Direcciones de host de la subred (limitada a /22 para no barrer redes enormes).
pub fn hosts(iface: &IfaceInfo) -> Vec<Ipv4Addr> {
    let prefix = iface.prefix.clamp(22, 30);
    let mask = u32::MAX << (32 - prefix as u32);
    let net = u32::from(iface.ip) & mask;
    let bcast = net | !mask;
    ((net + 1)..bcast).map(Ipv4Addr::from).filter(|ip| *ip != iface.ip).collect()
}

/// Con `CAP_NET_RAW`: ARP propio (presencia exacta). Sin él: UDP + tabla del kernel.
pub fn sweep(iface: &IfaceInfo) -> (Vec<(Ipv4Addr, [u8; 6])>, Method) {
    if let Some(found) = super::arp::sweep(iface) {
        return (found, Method::Arp);
    }
    (sweep_kernel(iface), Method::KernelTable)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Method {
    Arp,
    KernelTable,
}

impl Method {
    pub fn label(self) -> &'static str {
        match self {
            Method::Arp => "ARP propio (presencia exacta)",
            Method::KernelTable => "tabla ARP del kernel (puede tardar en notar equipos apagados)",
        }
    }
}

fn sweep_kernel(iface: &IfaceInfo) -> Vec<(Ipv4Addr, [u8; 6])> {
    if let Ok(sock) = UdpSocket::bind((iface.ip, 0)) {
        for h in hosts(iface) {
            let _ = sock.send_to(&[0u8], (h, 9));
            std::thread::sleep(Duration::from_micros(400)); // no saturar la cola ARP
        }
    }
    std::thread::sleep(Duration::from_millis(1800)); // dar tiempo a las respuestas ARP
    read_arp(&iface.name)
}

pub fn read_arp(dev: &str) -> Vec<(Ipv4Addr, [u8; 6])> {
    let mut out = Vec::new();
    let Ok(text) = std::fs::read_to_string("/proc/net/arp") else { return out };
    for l in text.lines().skip(1) {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() < 6 || f[5] != dev {
            continue;
        }
        let flags = u32::from_str_radix(f[2].trim_start_matches("0x"), 16).unwrap_or(0);
        if flags & 0x2 == 0 {
            continue; // entrada incompleta
        }
        let (Ok(ip), Some(mac)) = (f[0].parse::<Ipv4Addr>(), parse_mac(f[3])) else { continue };
        if mac != [0u8; 6] {
            out.push((ip, mac));
        }
    }
    out
}

pub fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let mut m = [0u8; 6];
    let mut it = s.split(':');
    for b in m.iter_mut() {
        *b = u8::from_str_radix(it.next()?, 16).ok()?;
    }
    Some(m)
}
