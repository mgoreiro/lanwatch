//! Pestaña «About»: datos del proyecto y del autor (salen de `Cargo.toml`, así que se editan en un solo
//! sitio) más información de esta instalación, útil para diagnosticar o para pedir ayuda.

use super::{Ctx, Tab};
use crate::core::state;
use crate::util;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;
use std::time::Instant;

pub struct About {
    started: Instant,
    scroll: u16,
}

impl About {
    pub fn new() -> Self {
        About { started: Instant::now(), scroll: 0 }
    }
}

const BANNER: [&str; 3] = [
    "┬  ┌─┐┌┐┌┬ ┬┌─┐┌┬┐┌─┐┬ ┬",
    "│  ├─┤│││││││├─┤ │ │  ├─┤",
    "┴─┘┴ ┴┘└┘└┴┘┴ ┴ ┴ └─┘┴ ┴",
];

fn title(t: &str) -> Line<'static> {
    Line::from(Span::styled(t.to_string(), Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
}

fn row(k: &str, v: impl Into<String>) -> Line<'static> {
    Line::from(vec![Span::styled(format!("  {k:<15}"), Style::new().fg(Color::DarkGray)), Span::raw(v.into())])
}

impl Tab for About {
    fn title(&self) -> String {
        "About".into()
    }

    fn help(&self) -> &'static str {
        "↑↓ desplazar · información del proyecto y de esta instalación"
    }

    fn on_key(&mut self, key: KeyEvent, _ctx: &Ctx) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.scroll += 1,
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll += 10,
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::Home => self.scroll = 0,
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame, area: Rect, ctx: &Ctx) {
        let st = state::lock(&ctx.shared);
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "?".into());
        let author = env!("CARGO_PKG_AUTHORS");
        let mut l: Vec<Line> = BANNER.iter().map(|b| Line::from(Span::styled(format!("  {b}"), Style::new().fg(Color::Cyan)))).collect();
        l.push(Line::raw(""));
        l.push(Line::from(Span::styled(format!("  {}", env!("CARGO_PKG_DESCRIPTION")), Style::new().add_modifier(Modifier::ITALIC))));
        l.push(Line::raw(""));

        l.push(title("Proyecto"));
        l.push(row("Nombre", env!("CARGO_PKG_NAME")));
        l.push(row("Versión", env!("CARGO_PKG_VERSION")));
        l.push(row("Licencia", format!("{} (software libre)", env!("CARGO_PKG_LICENSE"))));
        l.push(row("Repositorio", env!("CARGO_PKG_REPOSITORY")));
        l.push(row("Documentación", "pestaña Ayuda · man lanwatch · /usr/share/doc/lanwatch/"));
        l.push(Line::raw(""));

        l.push(title("Autor"));
        l.push(row("Nombre", author.split(" <").next().unwrap_or(author)));
        if let Some(mail) = author.split_once('<').map(|x| x.1.trim_end_matches('>')) {
            l.push(row("Contacto", mail));
        }
        l.push(row("Perfil", env!("CARGO_PKG_HOMEPAGE")));
        l.push(Line::raw(""));

        l.push(title("Esta instalación"));
        l.push(row("Ejecutable", exe));
        l.push(row("Plataforma", format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)));
        l.push(row("Configuración", crate::config::save_path().map(|p| if std::path::Path::new(&p).exists() { p } else { format!("{p} (aún no existe)") }).unwrap_or("–".into())));
        l.push(row("Interfaz", format!("{} · {}/{}", st.iface.name, st.iface.ip, st.iface.prefix)));
        l.push(row("CAP_NET_RAW", if st.raw_icmp { "sí (TTL y ARP propio)" } else { "no (pulsa c en Dispositivos)" }));
        l.push(row("SNMP", st.snmp.as_ref().map(|s| s.to_value()).unwrap_or("sin configurar".into())));
        l.push(row(
            "NetFlow",
            match (st.flow.listening, st.flow.version) {
                (None, _) => "desactivado".to_string(),
                (Some(p), None) => format!("UDP {p}, sin datos todavía"),
                (Some(p), Some(v)) => format!("UDP {p}, recibiendo {}", match v { 5 => "NetFlow v5", 9 => "NetFlow v9", 10 => "IPFIX", _ => "?" }),
            },
        ));
        l.push(row("Descubrimiento", if st.discovery.is_empty() { "iniciando…".to_string() } else { st.discovery.clone() }));
        l.push(row("Dispositivos", format!("{} ({} en línea)", st.devices.len(), st.devices.values().filter(|d| d.online).count())));
        l.push(row("En marcha desde", format!("hace {}", util::ago(self.started.elapsed().as_secs()))));
        l.push(Line::raw(""));

        l.push(title("Créditos"));
        l.push(row("Fabricantes MAC", "registros públicos de la IEEE (MA-L, MA-M, MA-S)"));
        l.push(row("Test velocidad", "servidores públicos de LibreSpeed y Cloudflare"));
        l.push(row("Bibliotecas", "ratatui · ureq · serde · serde_json · libc (MIT / Apache-2.0)"));
        drop(st);
        let visible = area.height.saturating_sub(2);
        self.scroll = self.scroll.min((l.len() as u16).saturating_sub(visible));
        f.render_widget(Paragraph::new(l).scroll((self.scroll, 0)).block(Block::bordered().title(" About ")), area);
    }
}
