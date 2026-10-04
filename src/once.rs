//! Modo `--once`: espera al primer barrido y a que se sondeen los dispositivos, imprime una tabla
//! de texto plano y termina. Útil para scripts, cron y para comprobar el funcionamiento sin TUI.

use crate::core::state::{self, Shared};
use crate::util;
use std::time::{Duration, Instant};

pub fn run(shared: &Shared) {
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
    let st = state::lock(shared);
    println!("{} · {}/{} · puerta de enlace {}", st.iface.name, st.iface.ip, st.iface.prefix, st.iface.gateway.map(|g| g.to_string()).unwrap_or("?".into()));
    println!("CAP_NET_RAW (TTL): {}", if st.raw_icmp { "sí" } else { "no" });
    println!("{:<16} {:<17} {:<22} {:<18} {:<26} {}", "IP", "MAC", "FABRICANTE", "NOMBRE", "SISTEMA", "PUERTOS");
    for d in st.devices.values().filter(|d| d.online) {
        println!(
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
