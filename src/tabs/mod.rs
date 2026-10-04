//! Pestañas de la interfaz. Para añadir una: crear `tabs/mi_pestana.rs` con un tipo que
//! implemente `Tab` y registrarlo en `all()`. Nada más del programa necesita cambiar.

use crate::config::Config;
use crate::core::state::Shared;
use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;
use std::sync::Arc;

pub mod devices;
pub mod dns;
pub mod gateway;
pub mod speed;

/// Contexto que reciben todas las pestañas.
pub struct Ctx {
    pub shared: Shared,
    #[allow(dead_code)] // disponible para pestañas futuras
    pub cfg: Arc<Config>,
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
    ]
}
