//! Pestaña c) Test de DNS: resolvedores del sistema + IPs propias, con comparación de latencia y respuestas.

use super::{Ctx, Tab};
use crate::net::dns;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState};
use ratatui::Frame;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
struct Resolver {
    ip: Ipv4Addr,
    system: bool,
}

#[derive(Clone, Default)]
struct Result1 {
    cold: Vec<Option<f64>>, // primera consulta de cada dominio
    warm: Vec<Option<f64>>, // repetición inmediata (¿responde desde caché?)
    answers: Vec<Vec<Ipv4Addr>>,
}

#[derive(Default)]
struct Run {
    running: bool,
    done: bool,
    results: Vec<Result1>, // alineado con `resolvers` en el momento de lanzar
    resolvers: Vec<Resolver>,
}

pub struct DnsTab {
    resolvers: Vec<Resolver>,
    sel: TableState,
    input: Option<String>,
    msg: String,
    run: Arc<Mutex<Run>>,
}

impl DnsTab {
    pub fn new() -> Self {
        let mut resolvers: Vec<Resolver> = dns::system_resolvers().into_iter().map(|ip| Resolver { ip, system: true }).collect();
        if resolvers.is_empty() {
            resolvers.push(Resolver { ip: Ipv4Addr::new(1, 1, 1, 1), system: false });
        }
        let mut sel = TableState::default();
        sel.select(Some(0));
        DnsTab { resolvers, sel, input: None, msg: String::new(), run: Arc::new(Mutex::new(Run::default())) }
    }

    fn start(&mut self) {
        if self.run.lock().unwrap().running {
            return;
        }
        let resolvers = self.resolvers.clone();
        *self.run.lock().unwrap() = Run { running: true, resolvers: resolvers.clone(), results: vec![Result1::default(); resolvers.len()], done: false };
        let run = self.run.clone();
        std::thread::spawn(move || {
            std::thread::scope(|sc| {
                for (i, r) in resolvers.iter().enumerate() {
                    let run = run.clone();
                    sc.spawn(move || {
                        let mut res = Result1::default();
                        for d in dns::DOMAINS {
                            let a = dns::query(r.ip, d, Duration::from_secs(2));
                            res.cold.push(a.as_ref().ok().map(|x| x.ms));
                            res.answers.push(a.map(|x| x.addrs).unwrap_or_default());
                            res.warm.push(dns::query(r.ip, d, Duration::from_secs(2)).ok().map(|x| x.ms));
                        }
                        run.lock().unwrap().results[i] = res;
                    });
                }
            });
            let mut r = run.lock().unwrap();
            r.running = false;
            r.done = true;
        });
    }
}

fn avg(v: &[Option<f64>]) -> Option<f64> {
    let ok: Vec<f64> = v.iter().flatten().copied().collect();
    if ok.is_empty() { None } else { Some(ok.iter().sum::<f64>() / ok.len() as f64) }
}

fn ms(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.1} ms")).unwrap_or("–".into())
}

fn differs(a: &[Ipv4Addr], b: &[Ipv4Addr]) -> bool {
    !a.is_empty() && !b.is_empty() && !a.iter().any(|x| b.contains(x))
}

impl Tab for DnsTab {
    fn title(&self) -> String {
        "DNS".into()
    }

    fn help(&self) -> &'static str {
        "r ejecutar test · i añadir IP · d borrar IP añadida · ↑↓ elegir"
    }

    fn capturing_input(&self) -> bool {
        self.input.is_some()
    }

    fn busy(&self) -> bool {
        self.run.lock().unwrap().running
    }

    fn on_key(&mut self, key: KeyEvent, _ctx: &Ctx) {
        if let Some(buf) = self.input.as_mut() {
            match key.code {
                KeyCode::Esc => self.input = None,
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Char(c) if c.is_ascii_digit() || c == '.' => buf.push(c),
                KeyCode::Enter => {
                    match buf.parse::<Ipv4Addr>() {
                        Ok(ip) if self.resolvers.iter().any(|r| r.ip == ip) => self.msg = format!("{ip} ya está en la lista"),
                        Ok(ip) => {
                            self.resolvers.push(Resolver { ip, system: false });
                            self.msg = format!("añadido {ip}; pulsa r para probar");
                        }
                        Err(_) => self.msg = "IP no válida".into(),
                    }
                    self.input = None;
                }
                _ => {}
            }
            return;
        }
        let n = self.resolvers.len();
        let s = self.sel.selected().unwrap_or(0);
        match key.code {
            KeyCode::Char('r') => self.start(),
            KeyCode::Char('i') => self.input = Some(String::new()),
            KeyCode::Down | KeyCode::Char('j') => self.sel.select(Some((s + 1).min(n - 1))),
            KeyCode::Up | KeyCode::Char('k') => self.sel.select(Some(s.saturating_sub(1))),
            KeyCode::Char('d') => {
                if self.resolvers.get(s).is_some_and(|r| !r.system) {
                    self.resolvers.remove(s);
                    self.sel.select(Some(s.saturating_sub(1)));
                } else {
                    self.msg = "solo se pueden borrar las IP añadidas a mano".into();
                }
            }
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame, area: Rect, _ctx: &Ctx) {
        let run = self.run.lock().unwrap();
        let [top, mid, bottom] = Layout::vertical([Constraint::Length(2), Constraint::Length(self.resolvers.len() as u16 + 3), Constraint::Min(4)]).areas(area);

        let status = if let Some(b) = &self.input {
            Line::styled(format!(" IP del DNS a probar: {b}█   (Enter aceptar · Esc cancelar)"), Style::new().fg(Color::Yellow))
        } else if run.running {
            Line::styled(" Probando… (12 dominios × 2 consultas por resolvedor)", Style::new().fg(Color::Yellow))
        } else if run.done {
            Line::from(" Resultado del último test. La 1ª consulta mide al resolvedor; la repetición, su caché.")
        } else {
            Line::from(" Pulsa r para lanzar el test con los resolvedores de la lista.")
        };
        f.render_widget(Paragraph::new(vec![status, Line::styled(format!(" {}", self.msg), Style::new().fg(Color::DarkGray))]), top);

        // Resumen por resolvedor. Los resultados corresponden a la lista del momento de lanzar.
        let rows: Vec<Row> = self
            .resolvers
            .iter()
            .map(|r| {
                let res = run.resolvers.iter().position(|x| x.ip == r.ip).and_then(|i| run.results.get(i)).filter(|x| !x.cold.is_empty());
                let label = format!("{}{}", r.ip, if r.system { "  (sistema)" } else { "  (añadido)" });
                match res {
                    Some(x) => {
                        let fails = x.cold.iter().filter(|v| v.is_none()).count();
                        let ok: Vec<f64> = x.cold.iter().flatten().copied().collect();
                        let (mn, mx) = (ok.iter().cloned().fold(f64::MAX, f64::min), ok.iter().cloned().fold(0.0, f64::max));
                        let ref_i = run.resolvers.iter().position(|y| y.ip == self.resolvers[0].ip).unwrap_or(0);
                        let diff = match run.results.get(ref_i) {
                            Some(base) if ref_i != run.resolvers.iter().position(|y| y.ip == r.ip).unwrap_or(0) => {
                                x.answers.iter().zip(&base.answers).filter(|(a, b)| differs(a, b)).count()
                            }
                            _ => 0,
                        };
                        Row::new(vec![
                            label,
                            ms(avg(&x.cold)),
                            ms(avg(&x.warm)),
                            if ok.is_empty() { "–".into() } else { format!("{mn:.1} / {mx:.1} ms") },
                            format!("{fails}/{}", x.cold.len()),
                            if diff == 0 { "=".into() } else { format!("{diff} dominios ≠") },
                        ])
                    }
                    None => Row::new(vec![label, "–".into(), "–".into(), "–".into(), "–".into(), "–".into()]),
                }
            })
            .collect();
        let t = Table::new(rows, [Constraint::Length(28), Constraint::Length(12), Constraint::Length(12), Constraint::Length(18), Constraint::Length(8), Constraint::Min(10)])
            .header(Row::new(vec!["Resolvedor", "1ª consulta", "Repetición", "mín / máx", "Fallos", "Respuestas vs 1º"]).style(Style::new().bold().fg(Color::Cyan)))
            .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
            .block(Block::new());
        f.render_stateful_widget(t, mid, &mut self.sel);

        // Matriz dominio × resolvedor (1ª consulta)
        if run.done || run.running {
            let mut header = vec!["Dominio".to_string()];
            header.extend(run.resolvers.iter().map(|r| r.ip.to_string()));
            let rows: Vec<Row> = dns::DOMAINS
                .iter()
                .enumerate()
                .map(|(d, name)| {
                    let mut cells = vec![name.to_string()];
                    for (i, _) in run.resolvers.iter().enumerate() {
                        let r = &run.results[i];
                        cells.push(match r.cold.get(d) {
                            Some(Some(v)) => {
                                let base = run.results.first().and_then(|b| b.answers.get(d));
                                let mark = if i > 0 && base.is_some_and(|b| differs(b, &r.answers[d])) { " ≠" } else { "" };
                                format!("{v:.1} ms{mark}")
                            }
                            Some(None) => "fallo".into(),
                            None => "…".into(),
                        });
                    }
                    Row::new(cells)
                })
                .collect();
            let widths: Vec<Constraint> = std::iter::once(Constraint::Length(20)).chain(run.resolvers.iter().map(|_| Constraint::Length(16))).collect();
            f.render_widget(
                Table::new(rows, widths)
                    .header(Row::new(header).style(Style::new().bold().fg(Color::Cyan)))
                    .block(Block::bordered().title(" Por dominio (1ª consulta; ≠ = ninguna IP en común con el 1.er resolvedor) ")),
                bottom,
            );
        }
    }
}
