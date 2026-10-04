//! Huellas activas de bajo coste para saber **qué es** un dispositivo, preguntándole directamente:
//!
//! * **mDNS** (UDP 5353, unicast): servicios que anuncia (`_airplay`, `_googlecast`, `_ipp`…), su nombre
//!   `.local` y, en equipos Apple, el modelo (`_device-info`).
//! * **NetBIOS** (UDP 137, NBSTAT): nombre y grupo de trabajo de equipos Windows/Samba.
//! * **SSDP/UPnP** (UDP 1900, unicast): cabecera `SERVER` («Linux/5.4 UPnP/1.0 …», «Windows/10 …»).
//!
//! Cada sonda es una sola petición UDP con un tiempo máximo corto y se ejecutan las tres a la vez.
//! Para añadir otra: una función `fn(ip) -> Partial` y una línea en `probe`.

use std::net::{Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant};

#[derive(Default, Clone, Debug, PartialEq)]
pub struct Fingerprint {
    pub hostname: Option<String>,
    pub services: Vec<String>,
    pub model: Option<String>,
    pub netbios: Option<String>,
    pub workgroup: Option<String>,
    pub ssdp_server: Option<String>,
}

const TIMEOUT: Duration = Duration::from_millis(900);

pub fn probe(ip: Ipv4Addr) -> Fingerprint {
    let (mdns, nb, ssdp) = std::thread::scope(|s| {
        let a = s.spawn(|| mdns(ip));
        let b = s.spawn(|| netbios(ip));
        let c = s.spawn(|| ssdp(ip));
        (a.join().unwrap_or_default(), b.join().unwrap_or_default(), c.join().unwrap_or_default())
    });
    Fingerprint {
        hostname: mdns.hostname,
        services: mdns.services,
        model: mdns.model,
        netbios: nb.0,
        workgroup: nb.1,
        ssdp_server: ssdp,
    }
}

// ---- utilidades de nombres DNS (con compresión) --------------------------------------------

fn read_name(b: &[u8], mut p: usize) -> Option<(String, usize)> {
    let (mut name, mut end, mut jumped, mut hops) = (String::new(), 0usize, false, 0);
    loop {
        let l = *b.get(p)? as usize;
        if l == 0 {
            if !jumped {
                end = p + 1;
            }
            break;
        }
        if l & 0xc0 == 0xc0 {
            let off = ((l & 0x3f) << 8) | *b.get(p + 1)? as usize;
            if !jumped {
                end = p + 2;
            }
            jumped = true;
            hops += 1;
            if hops > 16 {
                return None; // bucle de punteros
            }
            p = off;
            continue;
        }
        let label = b.get(p + 1..p + 1 + l)?;
        if !name.is_empty() {
            name.push('.');
        }
        name.push_str(&String::from_utf8_lossy(label));
        p += 1 + l;
    }
    Some((name, end))
}

fn encode_name(name: &str) -> Vec<u8> {
    let mut v = Vec::new();
    for label in name.split('.').filter(|l| !l.is_empty()) {
        v.push(label.len() as u8);
        v.extend_from_slice(label.as_bytes());
    }
    v.push(0);
    v
}

fn exchange(ip: Ipv4Addr, port: u16, request: &[u8], mut on_packet: impl FnMut(&[u8]), max_packets: usize) {
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else { return };
    if sock.send_to(request, (ip, port)).is_err() {
        return;
    }
    let end = Instant::now() + TIMEOUT;
    let mut buf = [0u8; 4096];
    for _ in 0..max_packets {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() || sock.set_read_timeout(Some(left)).is_err() {
            break;
        }
        match sock.recv_from(&mut buf) {
            Ok((n, from)) if from.ip() == std::net::IpAddr::V4(ip) => on_packet(&buf[..n]),
            Ok(_) => {}
            Err(_) => break,
        }
    }
}

// ---- mDNS -----------------------------------------------------------------------------------

#[derive(Default)]
pub struct Mdns {
    pub hostname: Option<String>,
    pub services: Vec<String>,
    pub model: Option<String>,
}

pub fn mdns_query(ip: Ipv4Addr) -> Vec<u8> {
    let rev = format!("{}.{}.{}.{}.in-addr.arpa", ip.octets()[3], ip.octets()[2], ip.octets()[1], ip.octets()[0]);
    let mut q = vec![0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0]; // 3 preguntas
    for (name, qtype) in [("_services._dns-sd._udp.local", 12u16), ("_device-info._tcp.local", 12), (rev.as_str(), 12)] {
        q.extend(encode_name(name));
        q.extend(qtype.to_be_bytes());
        q.extend(0x8001u16.to_be_bytes()); // clase IN + bit «responde por unicast»
    }
    q
}

/// Extrae servicios, nombre y modelo de una respuesta mDNS.
pub fn parse_mdns(b: &[u8], out: &mut Mdns) {
    if b.len() < 12 || b[2] & 0x80 == 0 {
        return;
    }
    let (qd, an, ns, ar) = (u16::from_be_bytes([b[4], b[5]]), u16::from_be_bytes([b[6], b[7]]), u16::from_be_bytes([b[8], b[9]]), u16::from_be_bytes([b[10], b[11]]));
    let mut p = 12;
    for _ in 0..qd {
        let Some((_, e)) = read_name(b, p) else { return };
        p = e + 4;
    }
    for _ in 0..(an as usize + ns as usize + ar as usize).min(64) {
        let Some((name, e)) = read_name(b, p) else { return };
        if e + 10 > b.len() {
            return;
        }
        let rtype = u16::from_be_bytes([b[e], b[e + 1]]);
        let rdlen = u16::from_be_bytes([b[e + 8], b[e + 9]]) as usize;
        let rd = e + 10;
        if rd + rdlen > b.len() {
            return;
        }
        match rtype {
            12 => {
                if let Some((target, _)) = read_name(b, rd) {
                    let lname = name.to_lowercase();
                    if lname == "_services._dns-sd._udp.local" {
                        let svc = target.trim_end_matches(".local").to_string();
                        if !out.services.contains(&svc) {
                            out.services.push(svc);
                        }
                    } else if lname.ends_with(".in-addr.arpa") && target.ends_with(".local") {
                        out.hostname = Some(target.trim_end_matches(".local").to_string());
                    }
                }
            }
            16 => {
                // TXT: cadenas «clave=valor» con prefijo de longitud
                let mut q = rd;
                while q < rd + rdlen {
                    let l = b[q] as usize;
                    if let Some(s) = b.get(q + 1..q + 1 + l) {
                        if let Some(m) = String::from_utf8_lossy(s).strip_prefix("model=") {
                            out.model = Some(m.to_string());
                        }
                    }
                    q += 1 + l;
                }
            }
            _ => {}
        }
        p = rd + rdlen;
    }
}

fn mdns(ip: Ipv4Addr) -> Mdns {
    let mut out = Mdns::default();
    exchange(ip, 5353, &mdns_query(ip), |pkt| parse_mdns(pkt, &mut out), 4);
    out.services.truncate(16);
    out
}

// ---- NetBIOS --------------------------------------------------------------------------------

pub fn nbstat_query() -> Vec<u8> {
    let mut q = vec![0x4c, 0x57, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0x20];
    q.extend(b"CKAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"); // el nombre «*» codificado en nivel 1
    q.extend([0, 0, 0x21, 0, 1]); // tipo NBSTAT, clase IN
    q
}

/// → (nombre de máquina, grupo de trabajo)
pub fn parse_nbstat(b: &[u8]) -> (Option<String>, Option<String>) {
    if b.len() < 12 + 2 + 10 + 1 || b[0] != 0x4c || b[1] != 0x57 {
        return (None, None);
    }
    let Some((_, e)) = read_name(b, 12) else { return (None, None) };
    let rd = e + 10;
    let Some(&count) = b.get(rd) else { return (None, None) };
    let (mut host, mut group) = (None, None);
    for i in 0..count as usize {
        let o = rd + 1 + i * 18;
        let Some(rec) = b.get(o..o + 18) else { break };
        let name = String::from_utf8_lossy(&rec[..15]).trim().to_string();
        let (suffix, flags) = (rec[15], u16::from_be_bytes([rec[16], rec[17]]));
        if suffix == 0 && !name.is_empty() {
            if flags & 0x8000 != 0 {
                group.get_or_insert(name);
            } else {
                host.get_or_insert(name);
            }
        }
    }
    (host, group)
}

fn netbios(ip: Ipv4Addr) -> (Option<String>, Option<String>) {
    let mut res = (None, None);
    exchange(ip, 137, &nbstat_query(), |pkt| {
        if res.0.is_none() {
            res = parse_nbstat(pkt);
        }
    }, 1);
    res
}

// ---- SSDP / UPnP ----------------------------------------------------------------------------

pub fn ssdp_request() -> Vec<u8> {
    b"M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: ssdp:all\r\n\r\n".to_vec()
}

pub fn parse_ssdp_server(b: &[u8]) -> Option<String> {
    String::from_utf8_lossy(b).lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim().eq_ignore_ascii_case("server") && !v.trim().is_empty()).then(|| v.trim().chars().take(80).collect())
    })
}

fn ssdp(ip: Ipv4Addr) -> Option<String> {
    let mut server = None;
    exchange(ip, 1900, &ssdp_request(), |pkt| {
        if server.is_none() {
            server = parse_ssdp_server(pkt);
        }
    }, 4);
    server
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rr(name: &[u8], rtype: u16, rdata: &[u8]) -> Vec<u8> {
        let mut v = name.to_vec();
        v.extend(rtype.to_be_bytes());
        v.extend([0x80, 1, 0, 0, 0, 120]);
        v.extend((rdata.len() as u16).to_be_bytes());
        v.extend_from_slice(rdata);
        v
    }

    #[test]
    fn mdns_servicios_nombre_y_modelo() {
        let mut p = vec![0, 0, 0x84, 0, 0, 0, 0, 3, 0, 0, 0, 0];
        p.extend(rr(&encode_name("_services._dns-sd._udp.local"), 12, &encode_name("_airplay._tcp.local")));
        p.extend(rr(&encode_name("_services._dns-sd._udp.local"), 12, &encode_name("_googlecast._tcp.local")));
        p.extend(rr(&encode_name("7.1.168.192.in-addr.arpa"), 12, &encode_name("Salon-TV.local")));
        let mut m = Mdns::default();
        parse_mdns(&p, &mut m);
        assert_eq!(m.services, vec!["_airplay._tcp", "_googlecast._tcp"]);
        assert_eq!(m.hostname.as_deref(), Some("Salon-TV"));

        let txt = [vec![16u8], b"model=MacBookAir".to_vec()].concat();
        let mut p = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0];
        p.extend(rr(&encode_name("Mac._device-info._tcp.local"), 16, &txt));
        let mut m = Mdns::default();
        parse_mdns(&p, &mut m);
        assert_eq!(m.model.as_deref(), Some("MacBookAir"));
    }

    #[test]
    fn mdns_resiste_basura_y_bucles_de_punteros() {
        let mut m = Mdns::default();
        parse_mdns(&[0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0xc0, 12, 0, 12, 0, 1, 0, 0, 0, 1, 0, 2, 0xc0, 12], &mut m); // puntero a sí mismo
        let mut seed = 7u32;
        for _ in 0..2000 {
            let junk: Vec<u8> = (0..(seed % 120) as usize).map(|_| { seed = seed.wrapping_mul(1103515245).wrapping_add(12345); (seed >> 16) as u8 }).collect();
            let mut forced = junk.clone();
            if forced.len() > 3 {
                forced[2] = 0x84;
            }
            parse_mdns(&forced, &mut m);
            parse_nbstat(&junk);
            parse_ssdp_server(&junk);
        }
    }

    #[test]
    fn netbios_nombre_y_grupo() {
        let mut p = vec![0x4c, 0x57, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0];
        p.push(0x20);
        p.extend(b"CKAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
        p.extend([0, 0, 0x21, 0, 1, 0, 0, 0, 0, 0, 0x39]); // tipo, clase, ttl, rdlen
        p.push(2); // dos nombres
        for (n, suffix, flags) in [("PC-SALON", 0u8, 0x0400u16), ("WORKGROUP", 0, 0x8400)] {
            let mut name = n.as_bytes().to_vec();
            name.resize(15, b' ');
            p.extend(name);
            p.push(suffix);
            p.extend(flags.to_be_bytes());
        }
        assert_eq!(parse_nbstat(&p), (Some("PC-SALON".into()), Some("WORKGROUP".into())));
    }

    #[test]
    fn ssdp_cabecera_server() {
        let r = b"HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nServer: Linux/4.9 UPnP/1.0 MiniUPnPd/2.1\r\nST: upnp:rootdevice\r\n\r\n";
        assert_eq!(parse_ssdp_server(r).as_deref(), Some("Linux/4.9 UPnP/1.0 MiniUPnPd/2.1"));
        assert_eq!(parse_ssdp_server(b"HTTP/1.1 200 OK\r\n\r\n"), None);
    }
}
