//! Hilos de fondo. Cada uno duerme la mayor parte del tiempo (consumo mínimo) y escribe en el
//! estado compartido; la interfaz solo lee. Para añadir una fuente de datos nueva: crear su módulo
//! en `net/`, un bucle aquí y los campos en `state.rs`.

use super::state::{self, Device, FlowCounters, Shared};
use crate::config::Config;
use crate::net::{discovery, fingerprint, iface::IfaceInfo, netflow, osdetect, oui, probe, snmp};
use std::net::Ipv4Addr;
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

const RESCAN_PORTS: Duration = Duration::from_secs(600);

pub fn start(shared: Shared, cfg: Arc<Config>) {
    state::lock(&shared).raw_icmp = probe::raw_icmp_available();

    let (tx, rx) = mpsc::channel::<Ipv4Addr>();
    {
        let (shared, cfg) = (shared.clone(), cfg.clone());
        std::thread::spawn(move || {
            while let Ok(ip) = rx.recv() {
                probe_device(&shared, &cfg, ip);
            }
        });
    }
    {
        let (shared, cfg) = (shared.clone(), cfg.clone());
        std::thread::spawn(move || discovery_loop(shared, cfg, tx));
    }
    if let Some(port) = cfg.netflow_port {
        let shared = shared.clone();
        std::thread::spawn(move || netflow::run(port, shared));
    }
    std::thread::spawn(move || gateway_loop(shared));
}

fn new_device(ip: Ipv4Addr, mac: [u8; 6], iface: &IfaceInfo) -> Device {
    let now = Instant::now();
    Device {
        ip,
        mac,
        vendor: oui::vendor(&mac),
        hostname: None,
        ttl: None,
        ports: Vec::new(),
        scanned: None,
        os: String::new(),
        services: Vec::new(),
        model: None,
        netbios: None,
        ssdp: None,
        first_seen: now,
        last_seen: now,
        online: true,
        missed: 0,
        is_self: ip == iface.ip,
        is_gateway: Some(ip) == iface.gateway,
        flow: FlowCounters::default(),
    }
}

fn discovery_loop(shared: Shared, cfg: Arc<Config>, probe_tx: Sender<Ipv4Addr>) {
    loop {
        let iface = {
            let mut st = state::lock(&shared);
            st.scanning = true;
            st.force_scan = false;
            st.iface.clone()
        };
        let (mut found, method) = discovery::sweep(&iface);
        found.push((iface.ip, iface.mac));

        let mut to_probe = Vec::new();
        {
            let mut st = state::lock(&shared);
            let now = Instant::now();
            st.discovery = method.label().to_string();
            // Un equipo se da por apagado tras 2 barridos seguidos sin respuesta (evita parpadeos).
            for d in st.devices.values_mut() {
                d.missed = d.missed.saturating_add(1);
                d.online = d.missed < 2;
            }
            for (ip, mac) in found {
                let d = st.devices.entry(ip).or_insert_with(|| new_device(ip, mac, &iface));
                if d.mac != mac {
                    d.mac = mac;
                    d.vendor = oui::vendor(&mac);
                    d.scanned = None;
                }
                d.online = true;
                d.missed = 0;
                d.last_seen = now;
            }
            for d in st.devices.values() {
                if d.missed == 0 && d.scanned.is_none_or(|t| t.elapsed() > RESCAN_PORTS) {
                    to_probe.push(d.ip);
                }
            }
            st.scanning = false;
            st.last_sweep = Some(now);
        }
        for ip in to_probe {
            let _ = probe_tx.send(ip);
        }
        // espera interrumpible por «r» (force_scan)
        let wake = Instant::now() + Duration::from_secs(cfg.scan_secs);
        while Instant::now() < wake && !state::lock(&shared).force_scan {
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

fn own_hostname() -> Option<String> {
    let mut buf = [0 as libc::c_char; 128];
    unsafe {
        if libc::gethostname(buf.as_mut_ptr(), buf.len()) != 0 {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
    }
}

fn probe_device(shared: &Shared, cfg: &Config, ip: Ipv4Addr) {
    let (is_self, is_gw, vendor, raw) = {
        let st = state::lock(shared);
        match st.devices.get(&ip) {
            Some(d) => (d.is_self, d.is_gateway, d.vendor.clone(), st.raw_icmp),
            None => return,
        }
    };
    // Escaneo de puertos y huellas (mDNS/NetBIOS/SSDP) a la vez: tardan ~1-2 s por equipo.
    let (ports, fp) = std::thread::scope(|s| {
        let ports = s.spawn(|| if cfg.portscan { probe::scan_ports(ip, Duration::from_millis(350), 16) } else { Vec::new() });
        let fp = s.spawn(|| if cfg.fingerprint && !is_self { fingerprint::probe(ip) } else { Default::default() });
        (ports.join().unwrap_or_default(), fp.join().unwrap_or_default())
    });
    let rdns = if is_self { own_hostname() } else { probe::reverse_name(ip) };
    // Nombre: DNS inverso > mDNS > NetBIOS
    let hostname = rdns.or(fp.hostname.clone()).or(fp.netbios.clone());
    let ttl = if raw && !is_self { probe::icmp_ttl(ip, Duration::from_millis(800)) } else { None };
    let os = if is_self {
        "Linux (esta máquina)".to_string()
    } else {
        osdetect::guess(&osdetect::Signals {
            ttl,
            vendor: &vendor,
            ports: &ports,
            hostname: hostname.as_deref(),
            is_gateway: is_gw,
            services: &fp.services,
            model: fp.model.as_deref(),
            netbios: fp.netbios.as_deref(),
            ssdp: fp.ssdp_server.as_deref(),
        })
    };
    let mut st = state::lock(shared);
    if let Some(d) = st.devices.get_mut(&ip) {
        d.hostname = hostname.or(d.hostname.take());
        d.ttl = ttl.or(d.ttl);
        d.ports = ports;
        d.os = os;
        d.services = fp.services;
        d.model = fp.model.or(d.model.take());
        d.netbios = fp.netbios.map(|n| match &fp.workgroup { Some(g) => format!("{n} ({g})"), None => n });
        d.ssdp = fp.ssdp_server;
        d.scanned = Some(Instant::now());
    }
}

// ---- Tráfico de la puerta de enlace ---------------------------------------------------------

fn local_counters(iface: &str) -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/net/dev").ok()?;
    for l in text.lines() {
        if let Some((name, rest)) = l.split_once(':') {
            if name.trim() == iface {
                let f: Vec<u64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                return Some((*f.first()?, *f.get(8)?));
            }
        }
    }
    None
}

struct SnmpSrc {
    client: snmp::Client,
    index: u32,
    label: String,
}

fn connect_snmp(s: &crate::config::SnmpCfg, gateway: Option<Ipv4Addr>) -> Result<SnmpSrc, String> {
    let host = s.host.or(gateway).ok_or("no hay IP del router para SNMP")?;
    let mut client = snmp::Client::new(host, &s.community)?;
    let (index, name) = client.find_ifindex(s.ifname.as_deref())?;
    client.counters(index)?; // comprobación
    Ok(SnmpSrc { client, index, label: format!("SNMP {host} · {name}") })
}

fn gateway_loop(shared: Shared) {
    let iface = state::lock(&shared).iface.clone();
    let mut src: Option<SnmpSrc> = None;
    let mut last_try = Instant::now() - Duration::from_secs(3600);
    let mut prev: Option<(u64, u64, Instant)> = None;
    let mut prev_source = String::new();
    let mut seen_gen = u64::MAX;
    loop {
        let (snmp_cfg, generation) = {
            let st = state::lock(&shared);
            (st.snmp.clone(), st.snmp_gen)
        };
        if generation != seen_gen {
            // la configuración SNMP cambió (desde la pestaña): reconectar ya
            seen_gen = generation;
            src = None;
            prev = None;
            last_try = Instant::now() - Duration::from_secs(3600);
            state::lock(&shared).gateway.error = None;
        }
        if let Some(sc) = snmp_cfg.as_ref().filter(|_| src.is_none() && last_try.elapsed() > Duration::from_secs(30)) {
            last_try = Instant::now();
            match connect_snmp(sc, iface.gateway) {
                Ok(s) => src = Some(s),
                Err(e) => state::lock(&shared).gateway.error = Some(e),
            }
        }
        let (source, note, reading) = match src.as_mut() {
            Some(s) => match s.client.counters(s.index) {
                Ok(c) => (s.label.clone(), String::new(), Some(c)),
                Err(e) => {
                    state::lock(&shared).gateway.error = Some(e);
                    src = None;
                    (String::new(), String::new(), None)
                }
            },
            None => (
                format!("Local · {}", iface.name),
                if snmp_cfg.is_some() { "SNMP no disponible: se muestran los contadores de esta máquina.".into() } else { "Solo tráfico de esta máquina. Pulsa s para configurar SNMP y ver el del router.".into() },
                local_counters(&iface.name),
            ),
        };
        if let Some((rx, tx)) = reading {
            let now = Instant::now();
            if source != prev_source {
                prev = None;
                prev_source = source.clone();
            }
            let mut st = state::lock(&shared);
            let g = &mut st.gateway;
            g.source = source;
            g.note = note;
            if src.is_some() {
                g.error = None;
            }
            if let Some((prx, ptx, pt)) = prev {
                let dt = now.duration_since(pt).as_secs_f64().max(0.001);
                let (dr, dtx) = (rx.wrapping_sub(prx), tx.wrapping_sub(ptx));
                g.in_bps = dr as f64 / dt;
                g.out_bps = dtx as f64 / dt;
                g.in_total += dr;
                g.out_total += dtx;
                g.peak_in = g.peak_in.max(g.in_bps);
                g.peak_out = g.peak_out.max(g.out_bps);
                for (h, v) in [(&mut g.hist_in, g.in_bps), (&mut g.hist_out, g.out_bps)] {
                    h.push_back(v as u64);
                    if h.len() > 240 {
                        h.pop_front();
                    }
                }
            }
            prev = Some((rx, tx, now));
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}
