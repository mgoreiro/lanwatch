//! Colector NetFlow v5 (UDP). El router exporta los flujos y aquí se agregan por IP de la LAN:
//! bytes **hacia** el dispositivo = entrante (descarga), bytes **desde** él = saliente (subida).
//!
//! Los flujos se exportan al caducar, así que las tasas son medias sobre una ventana de 60 s, no
//! instantáneas. Soporte de v9/IPFIX: añadir un parser que produzca `Record` (ver `parse_v5`).
//! Configuración del router: `docs/EDGEROUTER.md`.

use crate::core::state::{self, Shared};
use std::collections::{HashMap, VecDeque};
use std::net::{Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(60);

pub struct Record {
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
    pub octets: u64,
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
    let mut agg = Agg::default();
    let mut last_push = Instant::now();
    let mut buf = [0u8; 2048];
    loop {
        if let Ok((n, _)) = sock.recv_from(&mut buf) {
            let mut st = state::lock(&shared);
            st.flow.packets += 1;
            match parse_v5(&buf[..n]) {
                Some(records) => {
                    st.flow.last = Some(Instant::now());
                    st.flow.flows += records.len() as u64;
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
                None => st.flow.unsupported += 1,
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
}
