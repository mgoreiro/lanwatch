//! Barrido ARP propio (necesita `CAP_NET_RAW`): se pregunta «¿quién tiene esta IP?» a cada dirección de
//! la subred y solo los equipos que **contestan ahora** cuentan como presentes. A diferencia de la tabla
//! del kernel (ver `discovery.rs`), no conserva equipos ya apagados.
//!
//! Solo Linux (`AF_PACKET`). En otros sistemas, o sin permiso, `sweep` devuelve `None` y el llamador
//! recurre al método sin privilegios.

use super::iface::IfaceInfo;
use std::net::Ipv4Addr;

const ETH_P_ARP: u16 = 0x0806;

/// Trama Ethernet + ARP «who-has» (difusión) de 42 bytes.
pub fn build_request(src_mac: [u8; 6], src_ip: Ipv4Addr, target: Ipv4Addr) -> [u8; 42] {
    let mut f = [0u8; 42];
    f[0..6].copy_from_slice(&[0xff; 6]); // destino: difusión
    f[6..12].copy_from_slice(&src_mac);
    f[12..14].copy_from_slice(&ETH_P_ARP.to_be_bytes());
    f[14..16].copy_from_slice(&1u16.to_be_bytes()); // Ethernet
    f[16..18].copy_from_slice(&0x0800u16.to_be_bytes()); // IPv4
    f[18] = 6;
    f[19] = 4;
    f[20..22].copy_from_slice(&1u16.to_be_bytes()); // petición
    f[22..28].copy_from_slice(&src_mac);
    f[28..32].copy_from_slice(&src_ip.octets());
    // 32..38: MAC destino desconocida (ceros)
    f[38..42].copy_from_slice(&target.octets());
    f
}

/// Cualquier trama ARP válida revela a su emisor: (IP, MAC) del campo «sender», sea petición o respuesta.
pub fn parse_sender(frame: &[u8]) -> Option<(Ipv4Addr, [u8; 6])> {
    if frame.len() < 42 || u16::from_be_bytes([frame[12], frame[13]]) != ETH_P_ARP {
        return None;
    }
    let op = u16::from_be_bytes([frame[20], frame[21]]);
    if op != 1 && op != 2 {
        return None;
    }
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&frame[22..28]);
    let ip = Ipv4Addr::new(frame[28], frame[29], frame[30], frame[31]);
    if mac == [0u8; 6] || ip.is_unspecified() {
        return None;
    }
    Some((ip, mac))
}

#[cfg(target_os = "linux")]
pub fn sweep(iface: &IfaceInfo) -> Option<Vec<(Ipv4Addr, [u8; 6])>> {
    use std::collections::BTreeMap;
    use std::time::{Duration, Instant};

    let name = std::ffi::CString::new(iface.name.clone()).ok()?;
    unsafe {
        let fd = libc::socket(libc::AF_PACKET, libc::SOCK_RAW, (ETH_P_ARP.to_be()) as i32);
        if fd < 0 {
            return None; // sin CAP_NET_RAW
        }
        let ifindex = libc::if_nametoindex(name.as_ptr());
        if ifindex == 0 {
            libc::close(fd);
            return None;
        }
        let mut addr: libc::sockaddr_ll = std::mem::zeroed();
        addr.sll_family = libc::AF_PACKET as u16;
        addr.sll_protocol = ETH_P_ARP.to_be();
        addr.sll_ifindex = ifindex as i32;
        if libc::bind(fd, &addr as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_ll>() as u32) < 0 {
            libc::close(fd);
            return None;
        }
        let tv = libc::timeval { tv_sec: 0, tv_usec: 80_000 };
        libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_RCVTIMEO, &tv as *const _ as *const _, std::mem::size_of::<libc::timeval>() as u32);

        let hosts = super::discovery::hosts(iface);
        let mut seen: BTreeMap<Ipv4Addr, [u8; 6]> = BTreeMap::new();
        let mut buf = [0u8; 256];
        let mut drain = |seen: &mut BTreeMap<Ipv4Addr, [u8; 6]>, until: Instant| {
            while Instant::now() < until {
                let n = libc::recv(fd, buf.as_mut_ptr() as *mut _, buf.len(), 0);
                if n > 0 {
                    if let Some((ip, mac)) = parse_sender(&buf[..n as usize]) {
                        if iface.contains(ip) && ip != iface.ip {
                            seen.insert(ip, mac);
                        }
                    }
                }
            }
        };
        // Dos rondas: los móviles en ahorro de energía a veces pierden la primera pregunta.
        for round in 0..2 {
            for h in &hosts {
                if round == 1 && seen.contains_key(h) {
                    continue; // ya contestó
                }
                let frame = build_request(iface.mac, iface.ip, *h);
                let mut dst: libc::sockaddr_ll = std::mem::zeroed();
                dst.sll_family = libc::AF_PACKET as u16;
                dst.sll_protocol = ETH_P_ARP.to_be();
                dst.sll_ifindex = ifindex as i32;
                dst.sll_halen = 6;
                dst.sll_addr[..6].copy_from_slice(&[0xff; 6]);
                libc::sendto(fd, frame.as_ptr() as *const _, frame.len(), 0, &dst as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_ll>() as u32);
                std::thread::sleep(Duration::from_micros(250));
            }
            drain(&mut seen, Instant::now() + Duration::from_millis(if round == 0 { 900 } else { 1200 }));
        }
        libc::close(fd);
        Some(seen.into_iter().collect())
    }
}

#[cfg(not(target_os = "linux"))]
pub fn sweep(_iface: &IfaceInfo) -> Option<Vec<(Ipv4Addr, [u8; 6])>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_peticion_tiene_el_formato_correcto() {
        let f = build_request([2, 0, 0xab, 1, 2, 3], Ipv4Addr::new(192, 168, 1, 232), Ipv4Addr::new(192, 168, 1, 7));
        assert_eq!(&f[0..6], &[0xff; 6]);
        assert_eq!(&f[12..14], &[0x08, 0x06]);
        assert_eq!(&f[20..22], &[0, 1]); // who-has
        assert_eq!(&f[28..32], &[192, 168, 1, 232]);
        assert_eq!(&f[38..42], &[192, 168, 1, 7]);
    }

    #[test]
    fn se_lee_el_emisor_de_respuestas_y_peticiones() {
        let mut f = build_request([0xf0, 0x9f, 0xc2, 1, 2, 3], Ipv4Addr::new(192, 168, 1, 1), Ipv4Addr::new(192, 168, 1, 232));
        assert_eq!(parse_sender(&f), Some((Ipv4Addr::new(192, 168, 1, 1), [0xf0, 0x9f, 0xc2, 1, 2, 3])));
        f[21] = 2; // respuesta
        assert!(parse_sender(&f).is_some());
        f[21] = 9; // operación desconocida
        assert!(parse_sender(&f).is_none());
    }

    #[test]
    fn descarta_tramas_ajenas_o_cortas() {
        assert!(parse_sender(&[0u8; 10]).is_none());
        let mut f = build_request([1; 6], Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(10, 0, 0, 2));
        f[12] = 0x08;
        f[13] = 0x00; // IPv4, no ARP
        assert!(parse_sender(&f).is_none());
    }
}
