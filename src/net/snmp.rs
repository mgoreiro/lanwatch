//! Cliente SNMP v2c mínimo (GET / GETNEXT) para leer los contadores de una interfaz del router.
//! Solo implementa lo imprescindible de BER; sin dependencias.

use std::net::{Ipv4Addr, UdpSocket};
use std::time::Duration;

pub const IF_NAME: &[u32] = &[1, 3, 6, 1, 2, 1, 31, 1, 1, 1, 1];
pub const IF_HC_IN: &[u32] = &[1, 3, 6, 1, 2, 1, 31, 1, 1, 1, 6];
pub const IF_HC_OUT: &[u32] = &[1, 3, 6, 1, 2, 1, 31, 1, 1, 1, 10];

#[derive(Debug, PartialEq)]
pub enum Value {
    Int(u64),
    Text(String),
    Other,
}

fn enc_len(n: usize) -> Vec<u8> {
    if n < 128 {
        vec![n as u8]
    } else if n < 256 {
        vec![0x81, n as u8]
    } else {
        vec![0x82, (n >> 8) as u8, n as u8]
    }
}

fn tlv(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend(enc_len(body.len()));
    v.extend_from_slice(body);
    v
}

fn enc_int(n: i64) -> Vec<u8> {
    let mut b = n.to_be_bytes().to_vec();
    while b.len() > 1 && ((b[0] == 0 && b[1] & 0x80 == 0) || (b[0] == 0xff && b[1] & 0x80 != 0)) {
        b.remove(0);
    }
    tlv(0x02, &b)
}

fn enc_oid(oid: &[u32]) -> Vec<u8> {
    let mut b = vec![(oid[0] * 40 + oid[1]) as u8];
    for &n in &oid[2..] {
        let mut chunk = vec![(n & 0x7f) as u8];
        let mut m = n >> 7;
        while m > 0 {
            chunk.insert(0, 0x80 | (m & 0x7f) as u8);
            m >>= 7;
        }
        b.extend(chunk);
    }
    tlv(0x06, &b)
}

fn request(community: &str, pdu_tag: u8, req_id: i64, oid: &[u32]) -> Vec<u8> {
    let varbind = tlv(0x30, &[enc_oid(oid), vec![0x05, 0x00]].concat());
    let pdu = tlv(pdu_tag, &[enc_int(req_id), enc_int(0), enc_int(0), tlv(0x30, &varbind)].concat());
    tlv(0x30, &[enc_int(1), tlv(0x04, community.as_bytes()), pdu].concat())
}

/// Lee un TLV en `b[pos..]`: (etiqueta, contenido, siguiente posición).
fn read_tlv(b: &[u8], pos: usize) -> Option<(u8, &[u8], usize)> {
    let tag = *b.get(pos)?;
    let l0 = *b.get(pos + 1)? as usize;
    let (len, hdr) = if l0 < 128 {
        (l0, 2)
    } else {
        let n = l0 & 0x7f;
        let mut len = 0usize;
        for i in 0..n {
            len = (len << 8) | *b.get(pos + 2 + i)? as usize;
        }
        (len, 2 + n)
    };
    let end = pos + hdr + len;
    Some((tag, b.get(pos + hdr..end)?, end))
}

fn dec_oid(b: &[u8]) -> Vec<u32> {
    let mut out = vec![(b[0] / 40) as u32, (b[0] % 40) as u32];
    let mut n = 0u32;
    for &x in &b[1..] {
        n = (n << 7) | (x & 0x7f) as u32;
        if x & 0x80 == 0 {
            out.push(n);
            n = 0;
        }
    }
    out
}

/// Decodifica la respuesta → (OID, valor) del primer varbind.
pub fn parse_response(buf: &[u8]) -> Result<(Vec<u32>, Value), String> {
    let bad = || "respuesta SNMP malformada".to_string();
    let (_, msg, _) = read_tlv(buf, 0).ok_or_else(bad)?;
    let (_, _, p) = read_tlv(msg, 0).ok_or_else(bad)?; // versión
    let (_, _, p) = read_tlv(msg, p).ok_or_else(bad)?; // comunidad
    let (_, pdu, _) = read_tlv(msg, p).ok_or_else(bad)?;
    let (_, _, p) = read_tlv(pdu, 0).ok_or_else(bad)?; // request-id
    let (_, err, p) = read_tlv(pdu, p).ok_or_else(bad)?; // error-status
    if err.iter().any(|b| *b != 0) {
        return Err("el router respondió con error SNMP (¿OID/interfaz inexistente?)".into());
    }
    let (_, _, p) = read_tlv(pdu, p).ok_or_else(bad)?; // error-index
    let (_, vbl, _) = read_tlv(pdu, p).ok_or_else(bad)?;
    let (_, vb, _) = read_tlv(vbl, 0).ok_or_else(bad)?;
    let (_, oid, p) = read_tlv(vb, 0).ok_or_else(bad)?;
    let (tag, val, _) = read_tlv(vb, p).ok_or_else(bad)?;
    let value = match tag {
        0x02 | 0x41 | 0x42 | 0x43 | 0x46 => Value::Int(val.iter().fold(0u64, |a, b| (a << 8) | *b as u64)),
        0x04 => Value::Text(String::from_utf8_lossy(val).into_owned()),
        _ => Value::Other, // noSuchObject, endOfMibView…
    };
    Ok((dec_oid(oid), value))
}

pub struct Client {
    sock: UdpSocket,
    community: String,
    id: i64,
}

impl Client {
    pub fn new(host: Ipv4Addr, community: &str) -> Result<Client, String> {
        let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
        sock.connect((host, 161)).map_err(|e| e.to_string())?;
        sock.set_read_timeout(Some(Duration::from_secs(2))).ok();
        Ok(Client { sock, community: community.to_string(), id: 1 })
    }

    fn exchange(&mut self, tag: u8, oid: &[u32]) -> Result<(Vec<u32>, Value), String> {
        self.id += 1;
        self.sock.send(&request(&self.community, tag, self.id, oid)).map_err(|e| e.to_string())?;
        let mut buf = [0u8; 1500];
        let n = self.sock.recv(&mut buf).map_err(|_| "sin respuesta SNMP (¿SNMP activado y comunidad correcta?)".to_string())?;
        parse_response(&buf[..n])
    }

    pub fn get(&mut self, oid: &[u32]) -> Result<Value, String> {
        self.exchange(0xa0, oid).map(|r| r.1)
    }

    /// Índice de la interfaz con ese nombre (recorre ifName con GETNEXT).
    pub fn find_ifindex(&mut self, name: Option<&str>) -> Result<(u32, String), String> {
        let mut cur = IF_NAME.to_vec();
        let mut first: Option<(u32, String)> = None;
        for _ in 0..64 {
            let (oid, val) = self.exchange(0xa1, &cur)?;
            if !oid.starts_with(IF_NAME) || oid.len() != IF_NAME.len() + 1 {
                break;
            }
            let Value::Text(n) = val else { break };
            let idx = *oid.last().unwrap();
            if name.is_some_and(|w| w == n) {
                return Ok((idx, n));
            }
            if first.is_none() && n != "lo" && !n.starts_with("imq") {
                first = Some((idx, n));
            }
            cur = oid;
        }
        match (name, first) {
            (None, Some(f)) => Ok(f),
            (Some(n), _) => Err(format!("el router no tiene una interfaz llamada «{n}»")),
            _ => Err("el router no devolvió interfaces".into()),
        }
    }

    pub fn counters(&mut self, ifindex: u32) -> Result<(u64, u64), String> {
        let mut o = IF_HC_IN.to_vec();
        o.push(ifindex);
        let rx = self.get(&o)?;
        let mut o = IF_HC_OUT.to_vec();
        o.push(ifindex);
        let tx = self.get(&o)?;
        match (rx, tx) {
            (Value::Int(a), Value::Int(b)) => Ok((a, b)),
            _ => Err("el router no expone contadores de 64 bit para esa interfaz".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codifica_oid_y_peticion() {
        assert_eq!(enc_oid(&[1, 3, 6, 1, 2, 1]), vec![0x06, 5, 0x2b, 6, 1, 2, 1]);
        let r = request("public", 0xa0, 1, &[1, 3, 6, 1]);
        assert_eq!(r[0], 0x30);
        assert_eq!(r[r.len() - 2..], [0x05, 0x00]);
    }

    #[test]
    fn decodifica_counter64() {
        // GetResponse con un Counter64 = 0x0102
        let vb = tlv(0x30, &[enc_oid(&[1, 3, 6, 1, 2, 1, 31, 1, 1, 1, 6, 3]), tlv(0x46, &[1, 2])].concat());
        let pdu = tlv(0xa2, &[enc_int(1), enc_int(0), enc_int(0), tlv(0x30, &vb)].concat());
        let msg = tlv(0x30, &[enc_int(1), tlv(0x04, b"public"), pdu].concat());
        let (oid, v) = parse_response(&msg).unwrap();
        assert_eq!(oid.last(), Some(&3));
        assert_eq!(v, Value::Int(0x0102));
    }
}
