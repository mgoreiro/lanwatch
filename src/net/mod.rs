//! Fuentes de datos de red. Cada módulo es independiente y no conoce la interfaz de usuario:
//! devuelven datos, y `core::workers` / `tabs` deciden cuándo pedirlos y cómo mostrarlos.

pub mod arp;
pub mod discovery;
pub mod dns;
pub mod fingerprint;
pub mod iface;
pub mod netflow;
pub mod osdetect;
pub mod oui;
pub mod probe;
pub mod snmp;
pub mod speed;
