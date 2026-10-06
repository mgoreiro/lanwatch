//! Pestañas de la interfaz. Para añadir una: crear `tabs/mi_pestana.rs` con un tipo que
//! implemente `Tab` y registrarlo en `all()`. Nada más del programa necesita cambiar.

use crate::config::Config;
use crate::core::state::Shared;
use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;
use std::sync::{Arc, Mutex};

pub mod about;
pub mod devices;
pub mod dns;
pub mod gateway;
pub mod help;
pub mod iperf;
pub mod speed;
pub mod wifi;

/// Acciones que una pestaña no puede hacer por sí sola porque afectan a la terminal o al proceso.
/// La pestaña las pide con `Ctx::request` y `app.rs` las ejecuta.
#[derive(Debug, PartialEq)]
pub enum Action {
    /// Dar `CAP_NET_RAW` al binario con `sudo setcap` y relanzar (ver `elevate.rs`).
    GrantRawCap,
}

/// Contexto que reciben todas las pestañas.
pub struct Ctx {
    pub shared: Shared,
    #[allow(dead_code)] // disponible para pestañas futuras
    pub cfg: Arc<Config>,
    pub actions: Mutex<Vec<Action>>,
}

impl Ctx {
    pub fn new(shared: Shared, cfg: Arc<Config>) -> Ctx {
        Ctx { shared, cfg, actions: Mutex::new(Vec::new()) }
    }
    pub fn request(&self, a: Action) {
        self.actions.lock().unwrap_or_else(|e| e.into_inner()).push(a);
    }
    pub fn take_actions(&self) -> Vec<Action> {
        std::mem::take(&mut *self.actions.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

pub trait Tab {
    fn title(&self) -> String;
    /// Teclas propias de la pestaña, para el pie de pantalla.
    fn help(&self) -> &'static str;
    /// `true` mientras se escribe texto: desactiva los atajos globales (q, Tab, números).
    fn capturing_input(&self) -> bool {
        false
    }
    /// `true` si hay algo en curso que merece refrescar más a menudo (un test, una carga…).
    fn busy(&self) -> bool {
        false
    }
    fn on_key(&mut self, key: KeyEvent, ctx: &Ctx);
    fn on_tick(&mut self, _ctx: &Ctx) {}
    fn draw(&mut self, f: &mut Frame, area: Rect, ctx: &Ctx);
}

pub fn all() -> Vec<Box<dyn Tab>> {
    vec![
        Box::new(devices::Devices::new()),
        Box::new(gateway::Gateway::new()),
        Box::new(dns::DnsTab::new()),
        Box::new(speed::SpeedTab::new()),
        Box::new(iperf::IperfTab::new()),
        Box::new(wifi::WifiTab::new()),
        Box::new(help::Help::new()),
        Box::new(about::About::new()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::{self, Device, FlowCounters};
    use crate::net::iface::IfaceInfo;
    use ratatui::{backend::TestBackend, Terminal};
    use std::net::Ipv4Addr;
    use std::time::Instant;

    fn ctx() -> Ctx {
        let iface = IfaceInfo { name: "end0".into(), ip: Ipv4Addr::new(192, 168, 1, 232), prefix: 24, gateway: Some(Ipv4Addr::new(192, 168, 1, 1)), mac: [2, 0, 0xab, 1, 2, 3] };
        let shared = state::new(iface, None);
        {
            let mut st = state::lock(&shared);
            let now = Instant::now();
            for (i, (name, vendor, os, ports)) in [("router", "Ubiquiti", "Router / puerta de enlace", vec![22, 80, 443]), ("iphone", "Apple", "iOS (iPhone/iPad)", vec![62078])].into_iter().enumerate() {
                let ip = Ipv4Addr::new(192, 168, 1, 1 + i as u8 * 100);
                st.devices.insert(ip, Device {
                    ip, mac: [0xf0, 0x9f, 0xc2, 0, 0, i as u8], vendor: vendor.into(), hostname: Some(name.into()), ttl: Some(64), ports, scanned: Some(now),
                    os: os.into(), services: Vec::new(), model: None, netbios: None, ssdp: None, first_seen: now, last_seen: now, online: true, missed: 0, is_self: false, is_gateway: i == 0,
                    flow: FlowCounters { in_bytes: 5_000_000, out_bytes: 900_000, in_bps: 120_000.0, out_bps: 8_000.0, seen: true },
                });
            }
            st.flow.listening = Some(2055);
            st.gateway.source = "SNMP 192.168.1.1 · eth0".into();
            st.gateway.in_bps = 2_500_000.0;
            st.gateway.hist_in = (0..100u64).map(|x| x * 1000).collect();
            st.gateway.hist_out = (0..100u64).map(|x| (100 - x) * 500).collect();
        }
        Ctx::new(shared, Arc::new(Config::default()))
    }

    /// Dibuja cada pestaña en un terminal virtual (comprueba que no haya pánicos ni desbordes).
    /// Con `cargo test render -- --nocapture` se ve el resultado.
    #[test]
    fn render_de_todas_las_pestanas() {
        let ctx = ctx();
        for mut tab in all() {
            let mut term = Terminal::new(TestBackend::new(130, 22)).unwrap();
            term.draw(|f| tab.draw(f, f.area(), &ctx)).unwrap();
            let text: String = term.backend().buffer().content().chunks(130).map(|r| r.iter().map(|c| c.symbol()).collect::<String>().trim_end().to_string() + "\n").collect();
            println!("===== {} =====\n{text}", tab.title());
            assert!(!text.trim().is_empty());
        }
    }

    fn key(c: char) -> ratatui::crossterm::event::KeyEvent {
        ratatui::crossterm::event::KeyEvent::new(ratatui::crossterm::event::KeyCode::Char(c), ratatui::crossterm::event::KeyModifiers::NONE)
    }

    /// `c` pide confirmación (y suspende los atajos globales); solo «s» lanza la acción.
    #[test]
    fn dar_cap_net_raw_pide_confirmacion() {
        let ctx = ctx();
        let mut t = devices::Devices::new();
        t.on_key(key('c'), &ctx);
        assert!(t.capturing_input());
        t.on_key(key('n'), &ctx);
        assert!(!t.capturing_input());
        assert!(ctx.take_actions().is_empty());
        t.on_key(key('c'), &ctx);
        t.on_key(key('s'), &ctx);
        assert_eq!(ctx.take_actions(), vec![Action::GrantRawCap]);
    }

    #[test]
    fn no_pide_nada_si_ya_hay_cap_net_raw() {
        let ctx = ctx();
        state::lock(&ctx.shared).raw_icmp = true;
        let mut t = devices::Devices::new();
        t.on_key(key('c'), &ctx);
        assert!(!t.capturing_input());
    }

    /// La página de NetFlow de la ayuda sustituye la IP de ejemplo por la de esta máquina.
    #[test]
    fn la_ayuda_usa_la_ip_real() {
        let ctx = ctx(); // la máquina del contexto es 192.168.1.232; la cambiamos para notar la sustitución
        state::lock(&ctx.shared).iface.ip = Ipv4Addr::new(10, 9, 8, 7);
        let mut t = help::Help::new();
        t.on_key(key('l'), &ctx);
        let mut term = Terminal::new(TestBackend::new(110, 60)).unwrap();
        term.draw(|f| t.draw(f, f.area(), &ctx)).unwrap();
        let text: String = term.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("netflow server 10.9.8.7 port 2055"), "{text}");
        assert!(!text.contains("netflow server 192.168.1.232"));
    }
}
