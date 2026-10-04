//! Cliente DNS mínimo (UDP) para medir latencia y comparar respuestas entre resolvedores.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

pub struct Answer {
    pub ms: f64,
    pub addrs: Vec<Ipv4Addr>,
}

pub const DOMAINS: &[&str] = &[
    "google.com", "youtube.com", "facebook.com", "wikipedia.org", "amazon.es", "netflix.com", "cloudflare.com",
    "github.com", "microsoft.com", "apple.com", "elpais.com", "lavozdegalicia.es",
];

pub fn build_query(id: u16, name: &str) -> Vec<u8> {
    let mut q = vec![(id >> 8) as u8, id as u8, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in name.split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]); // tipo A, clase IN
    q
}

fn skip_name(b: &[u8], mut p: usize) -> Option<usize> {
    loop {
        let l = *b.get(p)? as usize;
        if l == 0 {
            return Some(p + 1);
        }
        if l & 0xc0 == 0xc0 {
            return Some(p + 2); // puntero de compresión
        }
        p += 1 + l;
    }
}

/// → (rcode, direcciones A de la respuesta)
pub fn parse_response(b: &[u8], id: u16) -> Option<(u8, Vec<Ipv4Addr>)> {
    if b.len() < 12 || u16::from_be_bytes([b[0], b[1]]) != id || b[2] & 0x80 == 0 {
        return None;
    }
    let rcode = b[3] & 0x0f;
    let (qd, an) = (u16::from_be_bytes([b[4], b[5]]), u16::from_be_bytes([b[6], b[7]]));
    let mut p = 12;
    for _ in 0..qd {
        p = skip_name(b, p)? + 4;
    }
    let mut addrs = Vec::new();
    for _ in 0..an {
        p = skip_name(b, p)?;
        let rtype = u16::from_be_bytes([*b.get(p)?, *b.get(p + 1)?]);
        let rdlen = u16::from_be_bytes([*b.get(p + 8)?, *b.get(p + 9)?]) as usize;
        p += 10;
        if rtype == 1 && rdlen == 4 {
            addrs.push(Ipv4Addr::new(*b.get(p)?, *b.get(p + 1)?, *b.get(p + 2)?, *b.get(p + 3)?));
        }
        p += rdlen;
    }
    addrs.sort();
    Some((rcode, addrs))
}

pub fn query(resolver: Ipv4Addr, name: &str, timeout: Duration) -> Result<Answer, String> {
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    sock.set_read_timeout(Some(timeout)).ok();
    let id = (Instant::now().elapsed().subsec_nanos() ^ std::process::id() ^ name.len() as u32) as u16;
    let start = Instant::now();
    sock.send_to(&build_query(id, name), SocketAddr::from((resolver, 53))).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 1500];
    loop {
        let n = sock.recv(&mut buf).map_err(|_| "tiempo agotado".to_string())?;
        if let Some((rcode, addrs)) = parse_response(&buf[..n], id) {
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            return match rcode {
                0 => Ok(Answer { ms, addrs }),
                2 => Err("SERVFAIL".into()),
                3 => Err("NXDOMAIN".into()),
                5 => Err("REFUSED".into()),
                n => Err(format!("error DNS {n}")),
            };
        }
    }
}

/// Servidores de `/etc/resolv.conf` (solo IPv4).
pub fn system_resolvers() -> Vec<Ipv4Addr> {
    std::fs::read_to_string("/etc/resolv.conf")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().strip_prefix("nameserver"))
        .filter_map(|r| r.trim().parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consulta_y_respuesta_con_compresion() {
        let q = build_query(0x1234, "ab.es");
        assert_eq!(&q[..2], &[0x12, 0x34]);
        // respuesta: cabecera + pregunta + una respuesta A 1.2.3.4 con puntero al nombre
        let mut r = q.clone();
        r[2] = 0x81;
        r[3] = 0x80;
        r[7] = 1;
        r.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 1, 2, 3, 4]);
        let (rc, a) = parse_response(&r, 0x1234).unwrap();
        assert_eq!(rc, 0);
        assert_eq!(a, vec![Ipv4Addr::new(1, 2, 3, 4)]);
        assert!(parse_response(&r, 0x9999).is_none());
    }

    /// Con red: `cargo test -- --ignored`
    #[test]
    #[ignore]
    fn consulta_real() {
        let a = query(Ipv4Addr::new(1, 1, 1, 1), "cloudflare.com", Duration::from_secs(3)).unwrap();
        assert!(!a.addrs.is_empty());
        assert!(query(Ipv4Addr::new(1, 1, 1, 1), "no-existe-xyz-123.invalid", Duration::from_secs(3)).is_err());
    }
}
