//! Estado compartido entre los hilos de trabajo (escriben) y las pestañas (leen).
//! Un único `Mutex`: los accesos son breves y no hay contención real.

use crate::config::SnmpCfg;
use crate::net::iface::IfaceInfo;
use std::collections::{BTreeMap, VecDeque};
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

#[derive(Clone, Default)]
pub struct FlowCounters {
    pub in_bytes: u64,  // hacia el dispositivo (descarga)
    pub out_bytes: u64, // desde el dispositivo (subida)
    pub in_bps: f64,
    pub out_bps: f64,
    pub seen: bool, // ha llegado algún flujo de este dispositivo
}

#[derive(Clone)]
pub struct Device {
    pub ip: Ipv4Addr,
    pub mac: [u8; 6],
    pub vendor: String,
    pub hostname: Option<String>,
    pub ttl: Option<u8>,
    pub ports: Vec<u16>,
    pub scanned: Option<Instant>,
    pub os: String,
    pub first_seen: Instant,
    pub last_seen: Instant,
    pub online: bool,
    pub is_self: bool,
    pub is_gateway: bool,
    pub flow: FlowCounters,
}

#[derive(Default)]
pub struct GatewayStats {
    pub source: String,
    pub note: String,
    pub in_bps: f64,
    pub out_bps: f64,
    pub in_total: u64,
    pub out_total: u64,
    pub peak_in: f64,
    pub peak_out: f64,
    pub hist_in: VecDeque<u64>,
    pub hist_out: VecDeque<u64>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct FlowStatus {
    pub listening: Option<u16>,
    pub packets: u64,
    pub flows: u64,
    pub unsupported: u64,
    pub version: Option<u16>,     // versión del protocolo recibido (5, 9 o 10 = IPFIX)
    pub waiting_template: u64,    // datos recibidos antes de conocer su plantilla
    pub last: Option<Instant>,
    pub error: Option<String>,
}

pub struct State {
    pub iface: IfaceInfo,
    pub devices: BTreeMap<Ipv4Addr, Device>,
    pub gateway: GatewayStats,
    pub flow: FlowStatus,
    pub scanning: bool,
    pub last_sweep: Option<Instant>,
    pub force_scan: bool,
    pub raw_icmp: bool, // ¿hay CAP_NET_RAW para medir el TTL?
    /// SNMP del router. Se puede cambiar en caliente desde la pestaña Puerta de enlace;
    /// `snmp_gen` sube con cada cambio para que el hilo de la puerta de enlace se reconecte.
    pub snmp: Option<SnmpCfg>,
    pub snmp_gen: u64,
}

pub type Shared = Arc<Mutex<State>>;

pub fn new(iface: IfaceInfo, snmp: Option<SnmpCfg>) -> Shared {
    Arc::new(Mutex::new(State {
        iface,
        devices: BTreeMap::new(),
        gateway: GatewayStats::default(),
        flow: FlowStatus::default(),
        scanning: false,
        last_sweep: None,
        force_scan: false,
        raw_icmp: false,
        snmp,
        snmp_gen: 0,
    }))
}

pub fn lock(s: &Shared) -> MutexGuard<'_, State> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}
