//! Pestaña de ayuda. El texto vive en `docs/AYUDA.md` y `docs/EDGEROUTER.md` (se embeben en el
//! binario): se editan como documentación normal y aquí solo se les da formato. En la página de
//! NetFlow, la IP de ejemplo se sustituye por la IP real de esta máquina.

use super::{Ctx, Tab};
use crate::core::state;
use crate::util;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

const GENERAL: &str = include_str!("../../docs/AYUDA.md");
const NETFLOW: &str = include_str!("../../docs/EDGEROUTER.md");
const EJEMPLO_IP: &str = "192.168.1.232";

const PAGES: [&str; 2] = ["Uso de la aplicación", "NetFlow y SNMP en el router"];

pub struct Help {
    page: usize,
    scroll: usize,
}

impl Help {
    pub fn new() -> Self {
        Help { page: 0, scroll: 0 }
    }
}

/// Ajuste de línea por palabras conservando la sangría (listas con «- »).
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(20);
    if text.chars().count() <= width {
        return vec![text.to_string()];
    }
    let indent: String = text.chars().take_while(|c| *c == ' ').collect();
    let hang = if text.trim_start().starts_with("- ") { format!("{indent}  ") } else { indent.clone() };
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let lead = if out.is_empty() { &indent } else { &hang };
        if cur.is_empty() {
            cur = format!("{lead}{word}");
        } else if cur.chars().count() + 1 + word.chars().count() > width {
            out.push(std::mem::take(&mut cur));
            cur = format!("{hang}{word}");
        } else {
            cur.push(' ');
            cur.push_str(word);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Markdown mínimo → líneas con estilo: `# títulos`, bloques de código, `> avisos` y listas.
pub fn render(md: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code = false;
    for raw in md.lines() {
        if raw.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            lines.push(Line::from(Span::styled(format!("  {raw}"), Style::new().fg(Color::Green))));
        } else if let Some(h) = raw.strip_prefix("## ").or_else(|| raw.strip_prefix("# ")) {
            if lines.last().is_some_and(|l: &Line| !l.to_string().is_empty()) {
                lines.push(Line::raw(""));
            }
            lines.push(Line::from(Span::styled(h.to_string(), Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
        } else if let Some(q) = raw.strip_prefix("> ") {
            for l in wrap(q, width.saturating_sub(2)) {
                lines.push(Line::from(Span::styled(format!("▌ {l}"), Style::new().fg(Color::Yellow))));
            }
        } else if raw.trim().is_empty() {
            lines.push(Line::raw(""));
        } else {
            for l in wrap(&raw.replace('`', ""), width) {
                lines.push(Line::raw(l));
            }
        }
    }
    lines
}

impl Tab for Help {
    fn title(&self) -> String {
        "Ayuda".into()
    }

    fn help(&self) -> &'static str {
        "←→ cambiar de página · ↑↓ RePág/AvPág desplazar"
    }

    fn on_key(&mut self, key: KeyEvent, _ctx: &Ctx) {
        match key.code {
            KeyCode::Right | KeyCode::Char('l') => (self.page, self.scroll) = ((self.page + 1) % PAGES.len(), 0),
            KeyCode::Left | KeyCode::Char('h') => (self.page, self.scroll) = ((self.page + PAGES.len() - 1) % PAGES.len(), 0),
            KeyCode::Down | KeyCode::Char('j') => self.scroll += 1,
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll += 10,
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::Home => self.scroll = 0,
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame, area: Rect, ctx: &Ctx) {
        let [pages_a, status_a, body_a] = Layout::vertical([Constraint::Length(1), Constraint::Length(2), Constraint::Min(3)]).areas(area);
        let spans: Vec<Span> = PAGES
            .iter()
            .enumerate()
            .flat_map(|(i, p)| {
                let st = if i == self.page { Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD) } else { Style::new().fg(Color::DarkGray) };
                [Span::styled(format!(" {p} "), st), Span::raw("  ")]
            })
            .collect();
        f.render_widget(Paragraph::new(Line::from(spans)), pages_a);

        let st = state::lock(&ctx.shared);
        let my_ip = st.iface.ip.to_string();
        let status = if self.page == 1 {
            let fl = &st.flow;
            let nf = match (fl.listening, &fl.error) {
                (_, Some(e)) => Line::styled(format!(" NetFlow ahora: {e}"), Style::new().fg(Color::Red)),
                (None, _) => Line::styled(" NetFlow ahora: desactivado (arranca con --netflow 2055)", Style::new().fg(Color::Yellow)),
                (Some(p), _) if fl.flows == 0 => Line::styled(format!(" NetFlow ahora: escuchando en UDP {p}, todavía no llega nada del router"), Style::new().fg(Color::Yellow)),
                (Some(p), _) => Line::styled(
                    format!(" NetFlow ahora: ✔ recibiendo en UDP {p} — {} flujos, el último hace {}", fl.flows, fl.last.map(|t| util::ago(t.elapsed().as_secs())).unwrap_or_default()),
                    Style::new().fg(Color::Green),
                ),
            };
            let sn = if ctx.cfg.snmp.is_some() {
                match &st.gateway.error {
                    Some(e) => Line::styled(format!(" SNMP ahora: {e}"), Style::new().fg(Color::Red)),
                    None => Line::styled(format!(" SNMP ahora: ✔ {}", st.gateway.source), Style::new().fg(Color::Green)),
                }
            } else {
                Line::styled(" SNMP ahora: sin configurar (opción --snmp)", Style::new().fg(Color::Yellow))
            };
            vec![nf, sn]
        } else {
            let cap = if st.raw_icmp { Line::styled(" CAP_NET_RAW: ✔ activo", Style::new().fg(Color::Green)) } else { Line::styled(" CAP_NET_RAW: no (pulsa c en Dispositivos)", Style::new().fg(Color::Yellow)) };
            vec![cap, Line::raw("")]
        };
        drop(st);
        f.render_widget(Paragraph::new(status), status_a);

        let width = body_a.width.saturating_sub(4) as usize;
        let md = if self.page == 0 { GENERAL.to_string() } else { NETFLOW.replace(EJEMPLO_IP, &my_ip) };
        let lines = render(&md, width);
        let visible = body_a.height.saturating_sub(2) as usize;
        self.scroll = self.scroll.min(lines.len().saturating_sub(visible));
        let shown: Vec<Line> = lines.iter().skip(self.scroll).take(visible).cloned().collect();
        let title = format!(" {} · {}/{} ", PAGES[self.page], (self.scroll + visible).min(lines.len()), lines.len());
        f.render_widget(Paragraph::new(shown).block(Block::bordered().title(title)), body_a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ajusta_lineas_largas_con_sangria() {
        let l = wrap("- una frase bastante larga que no cabe en veinte columnas", 24);
        assert!(l.len() > 2);
        assert!(l.iter().all(|x| x.chars().count() <= 24));
        assert!(l[1].starts_with("  "));
    }

    #[test]
    fn el_markdown_se_formatea() {
        let l = render("# Título\n\ntexto\n```\ncodigo\n```\n> aviso", 60);
        assert!(l.iter().any(|x| x.to_string() == "Título"));
        assert!(l.iter().any(|x| x.to_string() == "  codigo"));
        assert!(l.iter().any(|x| x.to_string().starts_with("▌ aviso")));
    }
}
