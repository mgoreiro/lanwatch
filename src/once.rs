//! Modo `--once`: espera al primer barrido y a que se sondeen los dispositivos, imprime una tabla
//! de texto plano y termina. Útil para scripts, cron y para comprobar el funcionamiento sin TUI.

use crate::core::state::{self, Shared};
use crate::util;
use std::io::Write;
use std::time::{Duration, Instant};

/// `println!` entra en pánico si la salida se cierra (p. ej. `| head`); aquí se ignora.
macro_rules! out {
    ($($a:tt)*) => {{
        let _ = writeln!(std::io::stdout(), $($a)*);
    }};
}

pub fn run(shared: &Shared, cfg: &crate::config::Config) {
    let cfg_source = cfg.source.clone().unwrap_or("(ningún fichero)".into());
    let snmp_txt = cfg.snmp.as_ref().map(|s| s.to_value()).unwrap_or("no configurado".into());
    let start = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let st = state::lock(shared);
        let swept = st.last_sweep.is_some();
        let pending = st.devices.values().filter(|d| d.online && d.scanned.is_none()).count();
        if (swept && pending == 0) || start.elapsed() > Duration::from_secs(90) {
            break;
        }
    }
    // Espera a la primera tasa de la puerta de enlace (SNMP necesita dos lecturas, ~2 s) para poder mostrarla.
    let gw_wait = Instant::now();
    while gw_wait.elapsed() < Duration::from_secs(14) {
        let st = state::lock(shared);
        if !st.gateway.source.is_empty() && st.gateway.hist_in.len() >= 4 {
            break;
        }
        if st.gateway.error.is_some() && cfg.snmp.is_some() {
            break;
        }
        drop(st);
        std::thread::sleep(Duration::from_millis(500));
    }
    let st = state::lock(shared);
    out!("{} · {}/{} · puerta de enlace {}", st.iface.name, st.iface.ip, st.iface.prefix, st.iface.gateway.map(|g| g.to_string()).unwrap_or("?".into()));
    out!("Configuración: {} · SNMP: {}", cfg_source, snmp_txt);
    out!(
        "Puerta de enlace: fuente «{}» · ↓ {} · ↑ {} (pico ↓ {} · ↑ {}){}",
        if st.gateway.source.is_empty() { "–" } else { &st.gateway.source },
        util::rate(st.gateway.in_bps),
        util::rate(st.gateway.out_bps),
        util::rate(st.gateway.peak_in),
        util::rate(st.gateway.peak_out),
        st.gateway.error.as_ref().map(|e| format!(" · error: {e}")).unwrap_or_default()
    );
    out!("CAP_NET_RAW (TTL): {}", if st.raw_icmp { "sí" } else { "no" });
    out!("{:<16} {:<17} {:<22} {:<18} {:<26} {}", "IP", "MAC", "FABRICANTE", "NOMBRE", "SISTEMA", "PUERTOS");
    for d in st.devices.values().filter(|d| d.online) {
        out!(
            "{:<16} {:<17} {:<22} {:<18} {:<26} {}",
            d.ip,
            util::mac_str(&d.mac),
            util::trunc(&d.vendor, 22),
            util::trunc(d.hostname.as_deref().unwrap_or(""), 18),
            util::trunc(&d.os, 26),
            d.ports.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",")
        );
    }
}
