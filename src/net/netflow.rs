//! Colector NetFlow/IPFIX (UDP): v5, v9 e IPFIX (v10). El router exporta los flujos y aquí se agregan
//! por IP de la LAN: bytes **hacia** el dispositivo = entrante (descarga), **desde** él = saliente (subida).
//!
//! v9 e IPFIX describen sus registros con *plantillas* que el router reenvía cada cierto tiempo; se
//! guardan por (exportador, dominio, id de plantilla) y se usan para decodificar los datos que llegan
//! después. Un paquete de datos cuya plantilla aún no se conoce se descarta (llegará otro).
//! Solo se cuentan flujos IPv4; los de IPv6 se ignoran.
//!
//! Los flujos se exportan al caducar, así que las tasas son medias sobre una ventana de 60 s.
//! Configuración del router: `docs/EDGEROUTER.md`.

use crate::core::state::{self, Shared};
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(60);
const MAX_TEMPLATES: usize = 512;

// Elementos de información (mismos números en NetFlow v9 e IPFIX)
const IE_IN_BYTES: u16 = 1;
const IE_SRC_V4: u16 = 8;
const IE_DST_V4: u16 = 12;
const IE_OUT_BYTES: u16 = 23;
const IE_OCTET_TOTAL: u16 = 85;

pub struct Record {
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
    pub octets: u64,
}

#[derive(Clone, Copy, Debug)]
struct Field {
    id: u16,
    len: u16, // 0xFFFF = longitud variable (solo IPFIX)
}

#[derive(Default)]
pub struct Templates {
    map: HashMap<(Ipv4Addr, u32, u16), Vec<Field>>,
}

#[derive(Debug, PartialEq)]
pub enum Parsed {
    /// Registros decodificados (puede estar vacío si solo traía plantillas).
    Flows { version: u16, records: usize, missing_template: bool },
    Unsupported,
}

/// NetFlow v5: cabecera de 24 bytes + registros de 48 bytes.
pub fn parse_v5(buf: &[u8]) -> Option<Vec<Record>> {
    if buf.len() < 24 || u16::from_be_bytes([buf[0], buf[1]]) != 5 {
        return None;
    }
    let count = u16::from_be_bytes([buf[2], buf[3]]) as usize;
    if buf.len() < 24 + count * 48 {
        return None;
    }
    Some(
        (0..count)
            .map(|i| {
                let r = &buf[24 + i * 48..];
                Record {
                    src: Ipv4Addr::new(r[0], r[1], r[2], r[3]),
                    dst: Ipv4Addr::new(r[4], r[5], r[6], r[7]),
                    octets: u32::from_be_bytes([r[20], r[21], r[22], r[23]]) as u64,
                }
            })
            .collect(),
    )
}

fn be(b: &[u8]) -> u64 {
    b.iter().take(8).fold(0u64, |a, x| (a << 8) | *x as u64)
}

fn u16_at(b: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(p)?, *b.get(p + 1)?]))
}

/// Registros de plantilla de un conjunto (v9: id 0; IPFIX: id 2).
fn read_templates(set: &[u8], ipfix: bool, key: (Ipv4Addr, u32), t: &mut Templates) {
    let mut p = 0;
    while p + 4 <= set.len() {
        let (Some(tid), Some(count)) = (u16_at(set, p), u16_at(set, p + 2)) else { return };
        p += 4;
        let mut fields = Vec::new();
        for _ in 0..count {
            let (Some(raw_id), Some(len)) = (u16_at(set, p), u16_at(set, p + 2)) else { return };
            p += 4;
            let mut id = raw_id;
            if ipfix && raw_id & 0x8000 != 0 {
                id = 0; // elemento de empresa: no se interpreta, solo se salta (4 bytes de número de empresa)
                p += 4;
            }
            fields.push(Field { id, len });
        }
        if t.map.len() >= MAX_TEMPLATES {
            t.map.clear(); // evita crecer sin límite si alguien envía basura
        }
        if tid >= 256 {
            t.map.insert((key.0, key.1, tid), fields);
        }
    }
}

/// Registros de datos de un conjunto, según su plantilla.
fn read_data(set: &[u8], fields: &[Field], out: &mut Vec<Record>) {
    let fixed: usize = fields.iter().map(|f| if f.len == 0xFFFF { 1 } else { f.len as usize }).sum();
    if fixed == 0 {
        return;
    }
    let mut p = 0;
    while set.len() - p >= fixed {
        let (mut src, mut dst, mut bytes, mut out_bytes, mut total) = (None, None, None, 0u64, None);
        for f in fields {
            let len = if f.len == 0xFFFF {
                let Some(&first) = set.get(p) else { return };
                p += 1;
                if first == 255 {
                    let Some(l) = u16_at(set, p) else { return };
                    p += 2;
                    l as usize
                } else {
                    first as usize
                }
            } else {
                f.len as usize
            };
            let Some(v) = set.get(p..p + len) else { return };
            p += len;
            match f.id {
                IE_SRC_V4 if len == 4 => src = Some(Ipv4Addr::new(v[0], v[1], v[2], v[3])),
                IE_DST_V4 if len == 4 => dst = Some(Ipv4Addr::new(v[0], v[1], v[2], v[3])),
                IE_IN_BYTES => bytes = Some(be(v)),
                IE_OUT_BYTES => out_bytes = be(v),
                IE_OCTET_TOTAL => total = Some(be(v)),
                _ => {}
            }
        }
        if let (Some(s), Some(d)) = (src, dst) {
            out.push(Record { src: s, dst: d, octets: bytes.or(total).unwrap_or(0) + out_bytes });
        }
    }
}

/// Decodifica un datagrama v9 o IPFIX; los registros se añaden a `out`.
pub fn parse_template_based(buf: &[u8], exporter: Ipv4Addr, t: &mut Templates, out: &mut Vec<Record>) -> Parsed {
    let Some(version) = u16_at(buf, 0) else { return Parsed::Unsupported };
    let (hdr, domain, ipfix) = match version {
        9 if buf.len() >= 20 => (20, u32::from_be_bytes([buf[16], buf[17], buf[18], buf[19]]), false),
        10 if buf.len() >= 16 => (16, u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]), true),
        _ => return Parsed::Unsupported,
    };
    let end = if ipfix { u16_at(buf, 2).map(|l| (l as usize).min(buf.len())).unwrap_or(buf.len()) } else { buf.len() };
    let (tpl_id, opt_id) = if ipfix { (2, 3) } else { (0, 1) };
    let (mut p, before, mut missing) = (hdr, out.len(), false);
    while p + 4 <= end {
        let (Some(id), Some(len)) = (u16_at(buf, p), u16_at(buf, p + 2)) else { break };
        let len = len as usize;
        if len < 4 || p + len > end {
            break; // conjunto mal formado o truncado
        }
        let body = &buf[p + 4..p + len];
        match id {
            i if i == tpl_id => read_templates(body, ipfix, (exporter, domain), t),
            i if i == opt_id => {} // plantillas de opciones: no interesan
            i if i >= 256 => match t.map.get(&(exporter, domain, i)) {
                Some(fields) => read_data(body, fields, out),
                None => missing = true,
            },
            _ => {}
        }
        p += len;
    }
    Parsed::Flows { version, records: out.len() - before, missing_template: missing }
}

#[derive(Default)]
struct Agg {
    samples: HashMap<Ipv4Addr, VecDeque<(Instant, u64, u64)>>, // (cuándo, entrante, saliente)
    totals: HashMap<Ipv4Addr, (u64, u64)>,
}

pub fn run(port: u16, shared: Shared) {
    let sock = match UdpSocket::bind(("0.0.0.0", port)) {
        Ok(s) => s,
        Err(e) => {
            state::lock(&shared).flow.error = Some(format!("no se pudo abrir UDP {port}: {e}"));
            return;
        }
    };
    let _ = sock.set_read_timeout(Some(Duration::from_secs(1)));
    state::lock(&shared).flow.listening = Some(port);

    let started = Instant::now();
    let (mut agg, mut templates) = (Agg::default(), Templates::default());
    let mut last_push = Instant::now();
    let mut buf = [0u8; 9000];
    loop {
        if let Ok((n, from)) = sock.recv_from(&mut buf) {
            let exporter = match from.ip() {
                IpAddr::V4(a) => a,
                _ => Ipv4Addr::UNSPECIFIED,
            };
            let mut records: Vec<Record> = Vec::new();
            let version = u16_at(&buf[..n], 0).unwrap_or(0);
            let result = match version {
                5 => parse_v5(&buf[..n]).map(|r| {
                    let c = r.len();
                    records = r;
                    Parsed::Flows { version: 5, records: c, missing_template: false }
                }),
                9 | 10 => Some(parse_template_based(&buf[..n], exporter, &mut templates, &mut records)),
                _ => None,
            };
            let mut st = state::lock(&shared);
            st.flow.packets += 1;
            match result {
                Some(Parsed::Flows { version, records: count, missing_template }) => {
                    st.flow.version = Some(version);
                    if missing_template {
                        st.flow.waiting_template += 1;
                    }
                    if count > 0 {
                        st.flow.last = Some(Instant::now());
                        st.flow.flows += count as u64;
                    }
                    let iface = st.iface.clone();
                    drop(st);
                    let now = Instant::now();
                    for r in records {
                        if iface.contains(r.src) {
                            agg.samples.entry(r.src).or_default().push_back((now, 0, r.octets));
                            agg.totals.entry(r.src).or_default().1 += r.octets;
                        }
                        if iface.contains(r.dst) {
                            agg.samples.entry(r.dst).or_default().push_back((now, r.octets, 0));
                            agg.totals.entry(r.dst).or_default().0 += r.octets;
                        }
                    }
                }
                _ => st.flow.unsupported += 1,
            }
        }
        if last_push.elapsed() >= Duration::from_secs(1) {
            last_push = Instant::now();
            let span = started.elapsed().min(WINDOW).as_secs_f64().max(1.0);
            let cutoff = Instant::now() - WINDOW;
            let mut st = state::lock(&shared);
            for q in agg.samples.values_mut() {
                while q.front().is_some_and(|s| s.0 < cutoff) {
                    q.pop_front();
                }
            }
            for (ip, dev) in st.devices.iter_mut() {
                if let Some(q) = agg.samples.get(ip) {
                    let (i, o) = q.iter().fold((0u64, 0u64), |a, s| (a.0 + s.1, a.1 + s.2));
                    let (ti, to) = agg.totals.get(ip).copied().unwrap_or_default();
                    dev.flow = state::FlowCounters { in_bytes: ti, out_bytes: to, in_bps: i as f64 / span, out_bps: o as f64 / span, seen: true };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXP: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);

    #[test]
    fn parsea_un_registro_v5() {
        let mut p = vec![0u8; 24 + 48];
        p[1] = 5;
        p[3] = 1;
        p[24..28].copy_from_slice(&[192, 168, 1, 10]);
        p[28..32].copy_from_slice(&[8, 8, 8, 8]);
        p[44..48].copy_from_slice(&1500u32.to_be_bytes());
        let r = parse_v5(&p).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].src, Ipv4Addr::new(192, 168, 1, 10));
        assert_eq!(r[0].dst, Ipv4Addr::new(8, 8, 8, 8));
        assert_eq!(r[0].octets, 1500);
    }

    #[test]
    fn rechaza_otras_versiones() {
        assert!(parse_v5(&[0, 9, 0, 0]).is_none());
    }

    fn set(id: u16, body: &[u8]) -> Vec<u8> {
        let mut v = id.to_be_bytes().to_vec();
        v.extend(((body.len() + 4) as u16).to_be_bytes());
        v.extend_from_slice(body);
        v
    }

    /// Plantilla 256 = [src v4 (4), dst v4 (4), in_bytes (4), puerto (2)].
    fn template_body(ipfix: bool) -> Vec<u8> {
        let mut b = vec![1, 0, 0, 4]; // id 256, 4 campos
        for (id, len) in [(8u16, 4u16), (12, 4), (1, 4), (7, 2)] {
            b.extend(id.to_be_bytes());
            b.extend(len.to_be_bytes());
        }
        let _ = ipfix;
        b
    }

    fn data_body() -> Vec<u8> {
        let mut b = Vec::new();
        for (s, d, n) in [([192, 168, 1, 50], [1, 1, 1, 1], 1000u32), ([8, 8, 8, 8], [192, 168, 1, 50], 4000u32)] {
            b.extend(s);
            b.extend(d);
            b.extend(n.to_be_bytes());
            b.extend([0, 53]);
        }
        b
    }

    fn v9(sets: &[Vec<u8>]) -> Vec<u8> {
        let mut p = vec![0, 9, 0, sets.len() as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        p.extend([0, 0, 0, 7]); // source id 7
        for s in sets {
            p.extend_from_slice(s);
        }
        p
    }

    fn ipfix(sets: &[Vec<u8>]) -> Vec<u8> {
        let mut body = Vec::new();
        for s in sets {
            body.extend_from_slice(s);
        }
        let mut p = vec![0, 10];
        p.extend(((16 + body.len()) as u16).to_be_bytes());
        p.extend([0, 0, 0, 0, 0, 0, 0, 1]); // export time, secuencia
        p.extend([0, 0, 0, 9]); // dominio 9
        p.extend(body);
        p
    }

    #[test]
    fn v9_plantilla_y_datos_en_el_mismo_paquete() {
        let pkt = v9(&[set(0, &template_body(false)), set(256, &data_body())]);
        let (mut t, mut out) = (Templates::default(), Vec::new());
        let r = parse_template_based(&pkt, EXP, &mut t, &mut out);
        assert_eq!(r, Parsed::Flows { version: 9, records: 2, missing_template: false });
        assert_eq!((out[0].src, out[0].dst, out[0].octets), (Ipv4Addr::new(192, 168, 1, 50), Ipv4Addr::new(1, 1, 1, 1), 1000));
        assert_eq!(out[1].octets, 4000);
    }

    #[test]
    fn v9_datos_sin_plantilla_se_descartan_y_despues_funcionan() {
        let (mut t, mut out) = (Templates::default(), Vec::new());
        let r = parse_template_based(&v9(&[set(256, &data_body())]), EXP, &mut t, &mut out);
        assert_eq!(r, Parsed::Flows { version: 9, records: 0, missing_template: true });
        parse_template_based(&v9(&[set(0, &template_body(false))]), EXP, &mut t, &mut out);
        let r = parse_template_based(&v9(&[set(256, &data_body())]), EXP, &mut t, &mut out);
        assert_eq!(r, Parsed::Flows { version: 9, records: 2, missing_template: false });
    }

    #[test]
    fn las_plantillas_son_por_exportador() {
        let (mut t, mut out) = (Templates::default(), Vec::new());
        parse_template_based(&v9(&[set(0, &template_body(false))]), EXP, &mut t, &mut out);
        let otro = Ipv4Addr::new(10, 0, 0, 9);
        let r = parse_template_based(&v9(&[set(256, &data_body())]), otro, &mut t, &mut out);
        assert_eq!(r, Parsed::Flows { version: 9, records: 0, missing_template: true });
    }

    #[test]
    fn ipfix_con_conjuntos_2_y_256() {
        let pkt = ipfix(&[set(2, &template_body(true)), set(256, &data_body())]);
        let (mut t, mut out) = (Templates::default(), Vec::new());
        let r = parse_template_based(&pkt, EXP, &mut t, &mut out);
        assert_eq!(r, Parsed::Flows { version: 10, records: 2, missing_template: false });
        assert_eq!(out.iter().map(|r| r.octets).sum::<u64>(), 5000);
    }

    #[test]
    fn ipfix_con_campo_de_empresa_y_longitud_variable() {
        // plantilla 257: src, dst, campo de empresa (bit 15 + 4 bytes de empresa, 2 de longitud), bytes (8), variable
        let mut tb = vec![1, 1, 0, 5];
        for (id, len) in [(8u16, 4u16), (12, 4)] {
            tb.extend(id.to_be_bytes());
            tb.extend(len.to_be_bytes());
        }
        tb.extend([0x80, 0x05, 0, 2, 0, 0, 0x00, 0x1d]); // id 5|0x8000, len 2, empresa 29
        tb.extend([0, 1, 0, 8]); // IN_BYTES de 8 bytes
        tb.extend([0, 0x60, 0xff, 0xff]); // campo 96 de longitud variable
        let mut data = vec![10, 0, 0, 5, 8, 8, 4, 4, 0xaa, 0xbb];
        data.extend(123456789u64.to_be_bytes());
        data.extend([3, b'a', b'b', b'c']);
        let pkt = ipfix(&[set(2, &tb), set(257, &data)]);
        let (mut t, mut out) = (Templates::default(), Vec::new());
        let r = parse_template_based(&pkt, EXP, &mut t, &mut out);
        assert_eq!(r, Parsed::Flows { version: 10, records: 1, missing_template: false });
        assert_eq!(out[0].octets, 123456789);
    }

    /// Un colector expuesto a la red no puede caer por paquetes basura: cualquier prefijo es válido.
    #[test]
    fn no_entra_en_panico_con_paquetes_truncados_o_basura() {
        let validos = [v9(&[set(0, &template_body(false)), set(256, &data_body())]), ipfix(&[set(2, &template_body(true)), set(256, &data_body())])];
        for pkt in &validos {
            for n in 0..pkt.len() {
                let (mut t, mut out) = (Templates::default(), Vec::new());
                parse_template_based(&pkt[..n], EXP, &mut t, &mut out);
            }
        }
        let mut seed = 0x1234_5678u32;
        for _ in 0..2000 {
            let len = (seed % 200) as usize;
            let junk: Vec<u8> = (0..len).map(|_| { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); (seed >> 24) as u8 }).collect();
            let mut forced = junk.clone();
            if forced.len() > 2 {
                forced[0] = 0;
                forced[1] = if seed & 1 == 0 { 9 } else { 10 };
            }
            let (mut t, mut out) = (Templates::default(), Vec::new());
            parse_template_based(&forced, EXP, &mut t, &mut out);
            let _ = parse_v5(&junk);
        }
    }
}
