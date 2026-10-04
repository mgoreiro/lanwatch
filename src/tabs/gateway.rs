//! Pestaña b) Tráfico total de la puerta de enlace.

use super::{Ctx, Tab};
use crate::core::state;
use crate::util;
use ratatui::crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Sparkline, Wrap};
use ratatui::Frame;

pub struct Gateway;

impl Gateway {
    pub fn new() -> Self {
        Gateway
    }
}

fn tail(h: &std::collections::VecDeque<u64>, width: usize) -> Vec<u64> {
    h.iter().skip(h.len().saturating_sub(width)).copied().collect()
}

impl Tab for Gateway {
    fn title(&self) -> String {
        "Puerta de enlace".into()
    }

    fn help(&self) -> &'static str {
        "se actualiza cada 2 s"
    }

    fn on_key(&mut self, _key: KeyEvent, _ctx: &Ctx) {}

    fn draw(&mut self, f: &mut Frame, area: Rect, ctx: &Ctx) {
        let st = state::lock(&ctx.shared);
        let g = &st.gateway;
        let gw = st.iface.gateway.map(|x| x.to_string()).unwrap_or("?".into());
        let [top, a, b] = Layout::vertical([Constraint::Length(6), Constraint::Min(4), Constraint::Min(4)]).areas(area);

        let mut lines = vec![
            Line::from(format!("Puerta de enlace: {gw}   ·   Fuente: {}", if g.source.is_empty() { "iniciando…" } else { &g.source })),
            Line::from(format!("↓ Entrante {:>12}   (pico {})   total {}", util::rate(g.in_bps), util::rate(g.peak_in), util::bytes(g.in_total))),
            Line::from(format!("↑ Saliente {:>12}   (pico {})   total {}", util::rate(g.out_bps), util::rate(g.peak_out), util::bytes(g.out_total))),
        ];
        if !g.note.is_empty() {
            lines.push(Line::styled(g.note.clone(), Style::new().fg(Color::Yellow)));
        }
        if let Some(e) = &g.error {
            lines.push(Line::styled(format!("SNMP: {e}"), Style::new().fg(Color::Red)));
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }).block(Block::new().borders(Borders::BOTTOM)), top);

        let w = (a.width as usize).saturating_sub(2);
        let d_in = tail(&g.hist_in, w);
        let d_out = tail(&g.hist_out, w);
        f.render_widget(
            Sparkline::default().block(Block::bordered().title(format!(" ↓ Entrante · máx. visible {} ", util::rate(d_in.iter().max().copied().unwrap_or(0) as f64)))).data(&d_in).style(Style::new().fg(Color::Cyan)),
            a,
        );
        f.render_widget(
            Sparkline::default().block(Block::bordered().title(format!(" ↑ Saliente · máx. visible {} ", util::rate(d_out.iter().max().copied().unwrap_or(0) as f64)))).data(&d_out).style(Style::new().fg(Color::Magenta)),
            b,
        );
    }
}
