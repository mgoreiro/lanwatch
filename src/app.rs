//! Bucle principal de la interfaz. Redibuja solo al pulsar una tecla o con el reloj de la
//! pestaña (1 s en reposo, 200 ms si hay algo en curso): en reposo apenas consume CPU.

use crate::tabs::{self, Ctx};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Tabs};
use std::io;
use std::time::Duration;

pub fn run(ctx: Ctx) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let mut tabs = tabs::all();
    let mut cur = 0usize;
    let res = (|| -> io::Result<()> {
        loop {
            terminal.draw(|f| {
                let [bar, body, foot] = Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
                let titles: Vec<Line> = tabs.iter().enumerate().map(|(i, t)| Line::from(format!(" {} {} ", i + 1, t.title()))).collect();
                f.render_widget(
                    Tabs::new(titles).select(cur).highlight_style(Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)).divider("│"),
                    bar,
                );
                tabs[cur].draw(f, body, &ctx);
                f.render_widget(
                    Paragraph::new(format!(" {}  ·  Tab/1-{} cambiar pestaña · q salir", tabs[cur].help(), tabs.len())).style(Style::new().fg(Color::DarkGray)),
                    foot,
                );
            })?;
            let wait = if tabs[cur].busy() { Duration::from_millis(200) } else { Duration::from_secs(1) };
            if event::poll(wait)? {
                if let Event::Key(k) = event::read()? {
                    if k.kind != KeyEventKind::Press {
                        continue;
                    }
                    if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
                        return Ok(());
                    }
                    if !tabs[cur].capturing_input() {
                        match k.code {
                            KeyCode::Char('q') => return Ok(()),
                            KeyCode::Tab => {
                                cur = (cur + 1) % tabs.len();
                                continue;
                            }
                            KeyCode::BackTab => {
                                cur = (cur + tabs.len() - 1) % tabs.len();
                                continue;
                            }
                            KeyCode::Char(c) if c.is_ascii_digit() && (1..=tabs.len()).contains(&(c as usize - '0' as usize)) => {
                                cur = c as usize - '1' as usize;
                                continue;
                            }
                            _ => {}
                        }
                    }
                    tabs[cur].on_key(k, &ctx);
                }
            } else {
                tabs[cur].on_tick(&ctx);
            }
        }
    })();
    ratatui::restore();
    res
}
