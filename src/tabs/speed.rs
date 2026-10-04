//! Pestaña d) Test de velocidad: lista de servidores (manual) o selección automática del más cercano.

use super::{Ctx, Tab};
use crate::net::speed::{self, Phase, Progress, Server};
use crate::util;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Sparkline, Wrap};
use ratatui::Frame;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Servers {
    list: Vec<Server>,
    loading: bool,
    error: Option<String>,
}

pub struct SpeedTab {
    servers: Arc<Mutex<Servers>>,
    sel: ListState, // 0 = automático; i+1 = servers[i]
    progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    history: Vec<String>,
}

impl SpeedTab {
    pub fn new() -> Self {
        let mut t = SpeedTab {
            servers: Arc::new(Mutex::new(Servers::default())),
            sel: ListState::default(),
            progress: Arc::new(Mutex::new(Progress::default())),
            cancel: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
            history: Vec::new(),
        };
        t.sel.select(Some(0));
        t.load();
        t
    }

    fn load(&self) {
        {
            let mut s = self.servers.lock().unwrap();
            if s.loading {
                return;
            }
            s.loading = true;
            s.error = None;
        }
        let servers = self.servers.clone();
        std::thread::spawn(move || {
            let res = speed::list();
            match res {
                Ok(mut list) => {
                    speed::measure_all(&mut list);
                    list.sort_by(|a, b| a.latency_ms.unwrap_or(f64::MAX).total_cmp(&b.latency_ms.unwrap_or(f64::MAX)));
                    let mut s = servers.lock().unwrap();
                    s.list = list;
                    s.loading = false;
                }
                Err(e) => {
                    let mut s = servers.lock().unwrap();
                    s.error = Some(e);
                    s.loading = false;
                }
            }
        });
    }

    fn run(&mut self, auto: bool) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let chosen = {
            let s = self.servers.lock().unwrap();
            if s.list.is_empty() {
                self.running.store(false, Ordering::SeqCst);
                return;
            }
            let idx = if auto { speed::pick_auto(&s.list) } else { self.sel.selected().and_then(|i| i.checked_sub(1)) };
            match idx.or_else(|| if auto { Some(0) } else { None }) {
                Some(i) => s.list[i].clone(),
                None => match speed::pick_auto(&s.list) {
                    Some(i) => s.list[i].clone(),
                    None => s.list[0].clone(),
                },
            }
        };
        self.cancel.store(false, Ordering::SeqCst);
        let (p, c, r) = (self.progress.clone(), self.cancel.clone(), self.running.clone());
        std::thread::spawn(move || {
            speed::run_test(&chosen, p, c);
            r.store(false, Ordering::SeqCst);
        });
    }
}

impl Tab for SpeedTab {
    fn title(&self) -> String {
        "Velocidad".into()
    }

    fn help(&self) -> &'static str {
        "a modo automático · Enter probar el servidor elegido · Esc cancelar · r recargar lista"
    }

    fn busy(&self) -> bool {
        self.running.load(Ordering::Relaxed) || self.servers.lock().unwrap().loading
    }

    fn on_key(&mut self, key: KeyEvent, _ctx: &Ctx) {
        let n = self.servers.lock().unwrap().list.len() + 1;
        let s = self.sel.selected().unwrap_or(0);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.sel.select(Some((s + 1).min(n - 1))),
            KeyCode::Up | KeyCode::Char('k') => self.sel.select(Some(s.saturating_sub(1))),
            KeyCode::Char('a') => self.run(true),
            KeyCode::Enter => self.run(s == 0),
            KeyCode::Esc => self.cancel.store(true, Ordering::SeqCst),
            KeyCode::Char('r') => self.load(),
            _ => {}
        }
    }

    fn on_tick(&mut self, _ctx: &Ctx) {
        let p = self.progress.lock().unwrap();
        if p.phase == Phase::Done && !self.running.load(Ordering::Relaxed) {
            let line = format!(
                "{} — ↓ {} · ↑ {} · {} ms",
                p.server,
                util::mbit(p.dl_bps.unwrap_or(0.0)),
                util::mbit(p.ul_bps.unwrap_or(0.0)),
                p.latency_ms.map(|l| format!("{l:.0}")).unwrap_or("–".into())
            );
            if self.history.last() != Some(&line) {
                self.history.push(line);
                if self.history.len() > 6 {
                    self.history.remove(0);
                }
            }
        }
    }

    fn draw(&mut self, f: &mut Frame, area: Rect, _ctx: &Ctx) {
        let [left, right] = Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(area);

        let s = self.servers.lock().unwrap();
        let mut items = vec![ListItem::new(Line::styled("★ Automático (el de menor latencia)", Style::new().fg(Color::Green).bold()))];
        items.extend(s.list.iter().map(|sv| {
            let l = sv.latency_ms.map(|l| format!("{l:>6.0} ms")).unwrap_or("   sin resp.".into());
            ListItem::new(format!("{l}  {}", util::trunc(&sv.name, 44)))
        }));
        let title = if s.loading {
            " Servidores (midiendo latencia…) ".to_string()
        } else if let Some(e) = &s.error {
            format!(" Servidores — {} ", util::trunc(e, 60))
        } else {
            format!(" Servidores ({}) ", s.list.len())
        };
        f.render_stateful_widget(
            List::new(items).block(Block::bordered().title(title)).highlight_style(Style::new().add_modifier(Modifier::REVERSED)),
            left,
            &mut self.sel,
        );
        drop(s);

        let p = self.progress.lock().unwrap().clone();
        let [top, spark, hist] = Layout::vertical([Constraint::Length(9), Constraint::Min(4), Constraint::Length(8)]).areas(right);
        let phase = match p.phase {
            Phase::Idle => "En reposo",
            Phase::Latency => "Midiendo latencia…",
            Phase::Download => "Descargando…",
            Phase::Upload => "Subiendo…",
            Phase::Done => "Completado",
            Phase::Failed => "Error",
        };
        let mut lines = vec![
            Line::styled(phase, Style::new().bold().fg(Color::Yellow)),
            Line::from(format!("Servidor: {}", if p.server.is_empty() { "–" } else { &p.server })),
            Line::from(format!("Latencia: {} · Jitter: {}", p.latency_ms.map(|l| format!("{l:.1} ms")).unwrap_or("–".into()), p.jitter_ms.map(|l| format!("{l:.1} ms")).unwrap_or("–".into()))),
            Line::from(""),
            Line::styled(
                format!("↓ Descarga: {}", p.dl_bps.map(util::mbit).unwrap_or_else(|| if p.phase == Phase::Download { util::mbit(p.cur_bps) } else { "–".into() })),
                Style::new().fg(Color::Cyan).bold(),
            ),
            Line::styled(
                format!("↑ Subida:   {}", p.ul_bps.map(util::mbit).unwrap_or_else(|| if p.phase == Phase::Upload { util::mbit(p.cur_bps) } else { "–".into() })),
                Style::new().fg(Color::Magenta).bold(),
            ),
        ];
        if let Some(e) = &p.error {
            lines.push(Line::styled(e.clone(), Style::new().fg(Color::Red)));
        }
        f.render_widget(Paragraph::new(lines).block(Block::bordered().title(" Test ")).wrap(Wrap { trim: true }), top);

        let w = (spark.width as usize).saturating_sub(2);
        let data: Vec<u64> = p.hist.iter().skip(p.hist.len().saturating_sub(w)).copied().collect();
        f.render_widget(
            Sparkline::default().block(Block::bordered().title(" Velocidad instantánea ")).data(&data).style(Style::new().fg(if p.phase == Phase::Upload { Color::Magenta } else { Color::Cyan })),
            spark,
        );
        f.render_widget(
            Paragraph::new(self.history.iter().rev().map(|h| Line::from(h.clone())).collect::<Vec<_>>()).block(Block::bordered().title(" Historial de esta sesión ")).wrap(Wrap { trim: true }),
            hist,
        );
    }
}
