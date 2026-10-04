//! Pestaña b) Tráfico total de la puerta de enlace, con asistente para configurar SNMP sin salir de la app.
//!
//! Flujo del asistente (`s`): 1) comunidad y IP del router → 2) se consulta el router y se listan sus
//! interfaces → 3) se elige la WAN y se aplica al instante y se guarda en el fichero de configuración.

use super::{Ctx, Tab};
use crate::config::{self, SnmpCfg};
use crate::core::state;
use crate::net::snmp;
use crate::util;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Sparkline, Wrap};
use ratatui::Frame;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};

type Found = Arc<Mutex<Option<Result<Vec<(u32, String)>, String>>>>;

enum Mode {
    Idle,
    /// Paso 1: campos de texto. `field`: 0 = comunidad, 1 = IP del router.
    Form { community: String, host: String, field: usize, error: Option<String> },
    /// Consultando al router (hilo en marcha).
    Connecting { community: String, host: Ipv4Addr, found: Found },
    /// Paso 2: elegir interfaz.
    Pick { community: String, host: Ipv4Addr, ifaces: Vec<(u32, String)>, list: ListState },
}

pub struct Gateway {
    mode: Mode,
    msg: String,
    msg_at: Option<std::time::Instant>,
}

impl Gateway {
    pub fn new() -> Self {
        Gateway { mode: Mode::Idle, msg: String::new(), msg_at: None }
    }

    fn say(&mut self, m: impl Into<String>) {
        self.msg = m.into();
        self.msg_at = Some(std::time::Instant::now());
    }

    fn open_form(&mut self, ctx: &Ctx) {
        let st = state::lock(&ctx.shared);
        let cur = st.snmp.clone();
        let host = cur.as_ref().and_then(|c| c.host).or(st.iface.gateway).map(|h| h.to_string()).unwrap_or_default();
        self.mode = Mode::Form { community: cur.map(|c| c.community).unwrap_or("public".into()), host, field: 0, error: None };
    }

    fn apply(&mut self, ctx: &Ctx, cfg: Option<SnmpCfg>) {
        {
            let mut st = state::lock(&ctx.shared);
            st.snmp = cfg.clone();
            st.snmp_gen += 1;
        }
        let saved = match config::save_path() {
            Some(p) => config::save_snmp_to(&p, cfg.as_ref()).map(|_| format!(" (guardado en {p})")),
            None => Err("sin ruta de configuración".into()),
        };
        let what = match (&cfg, saved) {
            (Some(c), Ok(s)) => format!("SNMP activado: {}{s}", c.to_value()),
            (Some(c), Err(e)) => format!("SNMP activado ({}), pero no se pudo guardar: {e}", c.to_value()),
            (None, Ok(s)) => format!("SNMP desactivado{s}"),
            (None, Err(e)) => format!("SNMP desactivado, pero no se pudo guardar: {e}"),
        };
        self.say(what);
        self.mode = Mode::Idle;
    }
}

fn tail(h: &std::collections::VecDeque<u64>, width: usize) -> Vec<u64> {
    h.iter().skip(h.len().saturating_sub(width)).copied().collect()
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

impl Tab for Gateway {
    fn title(&self) -> String {
        "Puerta de enlace".into()
    }

    fn help(&self) -> &'static str {
        "s configurar SNMP del router · d desactivarlo"
    }

    fn capturing_input(&self) -> bool {
        !matches!(self.mode, Mode::Idle)
    }

    fn busy(&self) -> bool {
        matches!(self.mode, Mode::Connecting { .. })
    }

    fn on_tick(&mut self, _ctx: &Ctx) {
        if let Mode::Connecting { community, host, found } = &self.mode {
            let done = found.lock().unwrap().take();
            match done {
                Some(Ok(ifaces)) => {
                    let mut list = ListState::default();
                    list.select(Some(ifaces.iter().position(|(_, n)| n == "eth0").unwrap_or(0)));
                    self.mode = Mode::Pick { community: community.clone(), host: *host, ifaces, list };
                }
                Some(Err(e)) => {
                    self.mode = Mode::Form { community: community.clone(), host: host.to_string(), field: 0, error: Some(e) };
                }
                None => {}
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &Ctx) {
        match &mut self.mode {
            Mode::Idle => match key.code {
                KeyCode::Char('s') => self.open_form(ctx),
                KeyCode::Char('d') => {
                    if state::lock(&ctx.shared).snmp.is_some() {
                        self.apply(ctx, None);
                    } else {
                        self.say("SNMP no está activado");
                    }
                }
                _ => {}
            },
            Mode::Form { community, host, field, error } => match key.code {
                KeyCode::Esc => self.mode = Mode::Idle,
                KeyCode::Tab | KeyCode::Down | KeyCode::Up => *field = 1 - *field,
                KeyCode::Backspace => {
                    if *field == 0 { community.pop() } else { host.pop() };
                }
                KeyCode::Char(c) if !c.is_control() && !c.is_whitespace() => {
                    if *field == 0 {
                        community.push(c)
                    } else if c.is_ascii_digit() || c == '.' {
                        host.push(c)
                    }
                }
                KeyCode::Enter => {
                    let Ok(ip) = host.parse::<Ipv4Addr>() else {
                        *error = Some("la IP del router no es válida".into());
                        return;
                    };
                    if community.is_empty() {
                        *error = Some("falta la comunidad".into());
                        return;
                    }
                    let (comm, found): (String, Found) = (community.clone(), Arc::new(Mutex::new(None)));
                    let (c2, f2) = (comm.clone(), found.clone());
                    std::thread::spawn(move || {
                        let r = snmp::Client::new(ip, &c2).and_then(|mut c| c.list_interfaces()).map_err(|e| {
                            if e.starts_with("sin respuesta") {
                                format!("{e} Revisa en la Ayuda cómo activarlo en el router.")
                            } else {
                                e
                            }
                        });
                        *f2.lock().unwrap() = Some(r);
                    });
                    self.mode = Mode::Connecting { community: comm, host: ip, found };
                }
                _ => {}
            },
            Mode::Connecting { .. } => {
                if key.code == KeyCode::Esc {
                    self.mode = Mode::Idle;
                }
            }
            Mode::Pick { community, host, ifaces, list } => {
                let sel = list.selected().unwrap_or(0);
                match key.code {
                    KeyCode::Esc => self.mode = Mode::Idle,
                    KeyCode::Down | KeyCode::Char('j') => list.select(Some((sel + 1).min(ifaces.len() - 1))),
                    KeyCode::Up | KeyCode::Char('k') => list.select(Some(sel.saturating_sub(1))),
                    KeyCode::Enter => {
                        let cfg = SnmpCfg { host: Some(*host), community: community.clone(), ifname: Some(ifaces[sel].1.clone()) };
                        self.apply(ctx, Some(cfg));
                    }
                    _ => {}
                }
            }
        }
    }

    fn draw(&mut self, f: &mut Frame, area: Rect, ctx: &Ctx) {
        let st = state::lock(&ctx.shared);
        let g = &st.gateway;
        let gw = st.iface.gateway.map(|x| x.to_string()).unwrap_or("?".into());
        let [top, a, b] = Layout::vertical([Constraint::Length(7), Constraint::Min(4), Constraint::Min(4)]).areas(area);

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
        if !self.msg.is_empty() && self.msg_at.is_some_and(|t| t.elapsed().as_secs() < 8) {
            lines.push(Line::styled(self.msg.clone(), Style::new().fg(Color::Green)));
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
        drop(st);

        // Asistente de SNMP (ventana emergente)
        let hint = Style::new().fg(Color::DarkGray);
        match &mut self.mode {
            Mode::Idle => {}
            Mode::Form { community, host, field, error } => {
                let r = centered(area, 66, 11);
                f.render_widget(Clear, r);
                let cur = |i: usize, v: &str| if *field == i { format!("{v}█") } else { v.to_string() };
                let mark = |i: usize| if *field == i { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new() };
                let mut l = vec![
                    Line::styled("Paso 1 de 2 · Datos del router (SNMP v2c)", Style::new().bold()),
                    Line::raw(""),
                    Line::styled(format!(" Comunidad : {:<30}", cur(0, community)), mark(0)),
                    Line::styled(format!(" IP router : {:<30}", cur(1, host)), mark(1)),
                    Line::raw(""),
                    Line::styled("El router debe tener SNMP activado (ver Ayuda → NetFlow y SNMP).", hint),
                ];
                if let Some(e) = error {
                    l.push(Line::styled(e.clone(), Style::new().fg(Color::Red)));
                }
                l.push(Line::raw(""));
                l.push(Line::styled("Tab cambiar de campo · Enter consultar el router · Esc cancelar", hint));
                f.render_widget(Paragraph::new(l).wrap(Wrap { trim: true }).block(Block::bordered().title(" Configurar SNMP ")), r);
            }
            Mode::Connecting { host, .. } => {
                let r = centered(area, 50, 5);
                f.render_widget(Clear, r);
                f.render_widget(
                    Paragraph::new(vec![Line::styled(format!("Consultando a {host}…"), Style::new().fg(Color::Yellow)), Line::styled("Esc para cancelar", hint)])
                        .block(Block::bordered().title(" Configurar SNMP ")),
                    r,
                );
            }
            Mode::Pick { ifaces, list, .. } => {
                let r = centered(area, 50, (ifaces.len() as u16 + 6).min(area.height));
                f.render_widget(Clear, r);
                let items: Vec<ListItem> = ifaces.iter().map(|(i, n)| ListItem::new(format!("{n:<14} (índice {i})"))).collect();
                f.render_stateful_widget(
                    List::new(items)
                        .block(Block::bordered().title(" Paso 2 de 2 · ¿Qué interfaz? ").title_bottom(" ↑↓ elegir · Enter aplicar · Esc cancelar "))
                        .highlight_style(Style::new().add_modifier(Modifier::REVERSED)),
                    r,
                    list,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::net::iface::IfaceInfo;
    use ratatui::crossterm::event::KeyModifiers;
    use ratatui::{backend::TestBackend, Terminal};

    fn ctx() -> Ctx {
        let iface = IfaceInfo { name: "end0".into(), ip: Ipv4Addr::new(192, 168, 1, 232), prefix: 24, gateway: Some(Ipv4Addr::new(192, 168, 1, 1)), mac: [0; 6] };
        Ctx::new(state::new(iface, None), Arc::new(Config::default()))
    }
    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }
    fn screen(t: &mut Gateway, ctx: &Ctx) -> String {
        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        term.draw(|f| t.draw(f, f.area(), ctx)).unwrap();
        term.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn el_asistente_valida_y_se_puede_cancelar() {
        let ctx = ctx();
        let mut t = Gateway::new();
        assert!(!t.capturing_input());
        t.on_key(key(KeyCode::Char('s')), &ctx);
        assert!(t.capturing_input());
        let s = screen(&mut t, &ctx);
        assert!(s.contains("Paso 1 de 2") && s.contains("public") && s.contains("192.168.1.1"), "{s}");
        // IP inválida → error y se queda en el formulario
        t.on_key(key(KeyCode::Tab), &ctx);
        for _ in 0..12 {
            t.on_key(key(KeyCode::Backspace), &ctx);
        }
        t.on_key(key(KeyCode::Char('1')), &ctx);
        t.on_key(key(KeyCode::Enter), &ctx);
        assert!(screen(&mut t, &ctx).contains("la IP del router no es válida"));
        t.on_key(key(KeyCode::Esc), &ctx);
        assert!(!t.capturing_input());
    }

    #[test]
    fn elegir_interfaz_aplica_guarda_y_avisa_al_hilo_de_fondo() {
        let ctx = ctx();
        let path = std::env::temp_dir().join(format!("lanwatch-gw-{}.conf", std::process::id()));
        std::env::set_var("LANWATCH_CONF", &path);
        let mut t = Gateway::new();
        let mut list = ListState::default();
        list.select(Some(1));
        t.mode = Mode::Pick { community: "public".into(), host: Ipv4Addr::new(192, 168, 1, 1), ifaces: vec![(1, "lo".into()), (2, "eth0".into())], list };
        let s = screen(&mut t, &ctx);
        assert!(s.contains("Paso 2 de 2") && s.contains("eth0"), "{s}");
        t.on_key(key(KeyCode::Enter), &ctx);
        let st = state::lock(&ctx.shared);
        assert_eq!(st.snmp.as_ref().unwrap().to_value(), "public@192.168.1.1/eth0");
        assert_eq!(st.snmp_gen, 1, "el hilo de la puerta de enlace debe reconectarse");
        drop(st);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "snmp=public@192.168.1.1/eth0\n");
        // desactivar
        t.on_key(key(KeyCode::Char('d')), &ctx);
        assert!(state::lock(&ctx.shared).snmp.is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        let _ = std::fs::remove_file(&path);
        std::env::remove_var("LANWATCH_CONF");
    }
}
