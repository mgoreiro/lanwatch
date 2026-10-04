//! lanwatch — monitor de red en terminal, de mínimo consumo, para Linux/ARM.
//! Estructura: `net/` (fuentes de datos), `core/` (estado + hilos), `tabs/` (interfaz).
//! Ver docs/ARCHITECTURE.md para añadir pestañas o fuentes.

mod app;
mod config;
mod core;
mod elevate;
mod net;
mod once;
mod tabs;
mod util;

use std::sync::Arc;

fn main() {
    let cfg = match config::Config::load() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("lanwatch: {e}");
            std::process::exit(2);
        }
    };
    if cfg.setcap {
        println!("Se ejecutará: {}", elevate::command_line());
        match elevate::grant_raw_cap() {
            Ok(()) => println!("Hecho: lanwatch ya puede medir el TTL (CAP_NET_RAW)."),
            Err(e) => {
                eprintln!("lanwatch: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let iface = match net::iface::detect(cfg.iface.as_deref()) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("lanwatch: {e}");
            std::process::exit(1);
        }
    };
    let shared = core::state::new(iface, cfg.snmp.clone());
    core::workers::start(shared.clone(), cfg.clone());
    if cfg.once {
        once::run(&shared);
        return;
    }
    if let Err(e) = app::run(tabs::Ctx::new(shared, cfg)) {
        eprintln!("lanwatch: {e}");
        std::process::exit(1);
    }
}
