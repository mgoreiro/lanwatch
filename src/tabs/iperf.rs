//! Pestaña iPerf3: servidores públicos para lanzar una prueba, o modo servidor para que otro equipo
//! de la red mida contra este. Por defecto, servidor en el puerto 5201.

use super::{Ctx, Tab};
use crate::core::state;
use crate::net::iperf::{self, Handle, Mode, Params, Run, DEFAULT_PORT, SERVERS};
use crate::util;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Sparkline, Wrap};
use ratatui::Frame;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const RETRY_AFTER: Duration = Duration::from_secs(60);

/// Qué hacer cuando el servidor o el puerto están ocupados.
#[derive(Clone, Copy, PartialEq)]
enum Retry {
    None,
    Asking,
    Waiting(Instant),
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Port,
    Host,
}

pub struct IperfTab {
    mode: Mode,
    port: u16,
    host: String,
    reverse: bool,
    sel: ListState,
    input: Option<(Field, String)>,
    msg: String,
    run: Arc<Mutex<Run>>,
    child: Handle,
    retry: Retry,
    last: Option<Params>,
}

impl IperfTab {
    pub fn new() -> Self {
        let mut sel = ListState::default();
        sel.select(Some(0));
        IperfTab {
            mode: Mode::Server,
            port: DEFAULT_PORT,
            host: String::new(),
            reverse: false,
            sel,
            input: None,
            msg: String::new(),
            run: Arc::new(Mutex::new(Run::default())),
            child: Arc::new(Mutex::new(None)),
            retry: Retry::None,
            last: None,
        }
    }

    fn running(&self) -> bool {
        self.run.lock().unwrap().running
    }

    fn launch(&mut self) {
        if self.running() {
            self.msg = "ya hay una prueba en curso (Esc para pararla)".into();
            return;
        }
        if self.mode == Mode::Client && self.host.trim().is_empty() {
            self.msg = "indica el destino (d) o elige un servidor de la lista y pulsa Enter".into();
            return;
        }
        if !iperf::available() {
            self.msg = "iperf3 no está instalado: sudo apt install iperf3".into();
            return;
        }
        self.msg.clear();
        let p = Params { mode: self.mode, port: self.port, host: self.host.trim().to_string(), reverse: self.reverse, secs: 10 };
        self.last = Some(p.clone());
        self.retry = Retry::None;
        iperf::start(p, self.run.clone(), self.child.clone());
    }

    /// Enter sobre la lista: rellena destino y puerto, pasa a modo cliente y lanza.
    fn use_selected(&mut self) {
        let Some(s) = self.sel.selected().and_then(|i| SERVERS.get(i)) else { return };
        if self.running() {
            self.msg = "ya hay una prueba en curso (Esc para pararla)".into();
            return;
        }
        self.mode = Mode::Client;
        self.host = s.host.to_string();
        self.port = s.port;
        self.launch();
    }

    fn accept(&mut self, field: Field, text: &str) {
        match field {
            Field::Port if text.is_empty() => self.port = DEFAULT_PORT,
            Field::Port => match text.parse::<u16>() {
                Ok(p) if p > 0 => self.port = p,
                _ => self.msg = "puerto no válido (1-65535)".into(),
            },
            Field::Host => self.host = text.trim().to_string(),
        }
    }
}

impl Tab for IperfTab {
    fn title(&self) -> String {
        "iPerf3".into()
    }

    fn help(&self) -> &'static str {
        "Enter probar el servidor elegido · r lanzar · m modo · p puerto · d destino · v sentido · Esc parar"
    }

    fn capturing_input(&self) -> bool {
        self.input.is_some() || self.retry == Retry::Asking
    }

    fn busy(&self) -> bool {
        self.running() || self.retry != Retry::None || self.run.lock().unwrap().retryable
    }

    fn on_tick(&mut self, _ctx: &Ctx) {
        match self.retry {
            Retry::None => {
                let mut r = self.run.lock().unwrap();
                if !r.running && r.retryable {
                    r.retryable = false;
                    self.retry = Retry::Asking;
                }
            }
            Retry::Waiting(t) if t.elapsed() >= RETRY_AFTER => {
                if let Some(p) = self.last.clone() {
                    self.retry = Retry::None;
                    iperf::start(p, self.run.clone(), self.child.clone());
                }
            }
            _ => {}
        }
    }

    fn on_key(&mut self, key: KeyEvent, _ctx: &Ctx) {
        match (self.retry, key.code) {
            (Retry::Asking, KeyCode::Char('s' | 'S' | 'y' | 'Y') | KeyCode::Enter) => {
                self.retry = Retry::Waiting(Instant::now());
                return;
            }
            (Retry::Asking, _) => {
                self.retry = Retry::None;
                return;
            }
            (Retry::Waiting(_), KeyCode::Esc) => {
                self.retry = Retry::None;
                self.msg = "reintento cancelado".into();
                return;
            }
            _ => {}
        }
        if let Some((field, buf)) = self.input.as_mut() {
            let field = *field;
            match key.code {
                KeyCode::Esc => self.input = None,
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Char(c) if field == Field::Port && c.is_ascii_digit() && buf.len() < 5 => buf.push(c),
                KeyCode::Char(c) if field == Field::Host && (c.is_ascii_alphanumeric() || ".-_:[]".contains(c)) && buf.len() < 253 => buf.push(c),
                KeyCode::Enter => {
                    let text = std::mem::take(buf);
                    self.input = None;
                    self.accept(field, &text);
                }
                _ => {}
            }
            return;
        }
        let n = SERVERS.len();
        let s = self.sel.selected().unwrap_or(0);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.sel.select(Some((s + 1).min(n - 1))),
            KeyCode::Up | KeyCode::Char('k') => self.sel.select(Some(s.saturating_sub(1))),
            KeyCode::Enter => self.use_selected(),
            KeyCode::Char('r') => self.launch(),
            KeyCode::Char('m') => {
                if self.running() {
                    self.msg = "para la prueba (Esc) antes de cambiar de modo".into();
                } else {
                    self.mode = if self.mode == Mode::Server { Mode::Client } else { Mode::Server };
                }
            }
            KeyCode::Char('p') => self.input = Some((Field::Port, String::new())),
            KeyCode::Char('d') => {
                self.mode = Mode::Client;
                self.input = Some((Field::Host, self.host.clone()));
            }
            KeyCode::Char('v') => self.reverse = !self.reverse,
            KeyCode::Esc => {
                if self.running() {
                    iperf::stop(&self.child);
                }
            }
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame, area: Rect, ctx: &Ctx) {
        let [left, right] = Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(area);

        let items: Vec<ListItem> = SERVERS
            .iter()
            .map(|s| ListItem::new(vec![Line::from(format!("{}:{}", s.host, s.port)), Line::styled(format!("   {}", s.place), Style::new().fg(Color::DarkGray))]))
            .collect();
        f.render_stateful_widget(
            List::new(items).block(Block::bordered().title(format!(" Servidores públicos ({}) ", SERVERS.len()))).highlight_style(Style::new().add_modifier(Modifier::REVERSED)),
            left,
            &mut self.sel,
        );

        let [form, status, spark] = Layout::vertical([Constraint::Length(8), Constraint::Min(4), Constraint::Length(5)]).areas(right);
        let k = Style::new().fg(Color::DarkGray);
        let edit = |field: Field, shown: String| -> Span<'static> {
            match &self.input {
                Some((f, b)) if *f == field => Span::styled(format!("{b}█"), Style::new().fg(Color::Yellow)),
                _ => Span::raw(shown),
            }
        };
        let client = self.mode == Mode::Client;
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Modo      ", k),
                Span::styled(if client { "Cliente" } else { "Servidor" }, Style::new().bold().fg(Color::Cyan)),
                Span::styled("   (m para cambiar)", k),
            ]),
            Line::from(vec![Span::styled("Puerto    ", k), edit(Field::Port, self.port.to_string()), Span::styled(if self.port == DEFAULT_PORT { "   (el de iperf3)" } else { "" }, k)]),
        ];
        if client {
            lines.push(Line::from(vec![Span::styled("Destino   ", k), edit(Field::Host, if self.host.is_empty() { "– (d para escribirlo)".into() } else { self.host.clone() })]));
            lines.push(Line::from(vec![
                Span::styled("Sentido   ", k),
                Span::raw(if self.reverse { "↓ descarga (el servidor envía)" } else { "↑ subida (este equipo envía)" }),
                Span::styled("   (v)", k),
            ]));
            lines.push(Line::styled("Cada prueba dura 10 s y usa toda la línea.", k));
        } else {
            let ip = state::lock(&ctx.shared).iface.ip;
            lines.push(Line::from(vec![Span::styled("Escucha   ", k), Span::raw(format!("{ip}:{}", self.port))]));
            lines.push(Line::styled("Otro equipo: iperf3 -c <esta IP> -p <puerto>", k));
            lines.push(Line::styled("r arranca el servidor; Enter vacío en el puerto = 5201.", k));
        }
        if !self.msg.is_empty() {
            lines.push(Line::styled(self.msg.clone(), Style::new().fg(Color::Yellow)));
        }
        f.render_widget(Paragraph::new(lines).block(Block::bordered().title(" Conexión ")), form);

        let r = self.run.lock().unwrap();
        let head = if self.retry != Retry::None {
            "Ocupado"
        } else if r.running {
            if client { "Ejecutando prueba…" } else { "Servidor en marcha, esperando clientes…" }
        } else if r.finished {
            "Terminado"
        } else {
            "En reposo · pulsa r"
        };
        let mut out = vec![Line::styled(head, Style::new().bold().fg(Color::Yellow))];
        if let Some(e) = &r.error {
            out.push(Line::styled(iperf::explain(e), Style::new().fg(Color::Red)));
        }
        match self.retry {
            Retry::Asking => out.push(Line::styled("¿Esperar 1 minuto y reintentar? (s/n)", Style::new().fg(Color::Yellow).bold())),
            Retry::Waiting(t) => out.push(Line::styled(
                format!("Reintentando en {} s… (Esc cancela)", RETRY_AFTER.saturating_sub(t.elapsed()).as_secs() + 1),
                Style::new().fg(Color::Yellow).bold(),
            )),
            Retry::None => {}
        }
        let room = (status.height as usize).saturating_sub(2 + out.len());
        out.extend(r.lines.iter().rev().take(room).rev().map(|l| Line::from(util::trunc(l, status.width as usize))));
        f.render_widget(Paragraph::new(out).block(Block::bordered().title(" Salida de iperf3 ")).wrap(Wrap { trim: false }), status);

        let w = (spark.width as usize).saturating_sub(2);
        let data: Vec<u64> = r.hist.iter().skip(r.hist.len().saturating_sub(w)).copied().collect();
        let last = r.hist.last().map(|b| util::mbit(*b as f64)).unwrap_or_default();
        f.render_widget(
            Sparkline::default().block(Block::bordered().title(format!(" Velocidad instantánea {last} "))).data(&data).style(Style::new().fg(Color::Cyan)),
            spark,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::net::iface::IfaceInfo;
    use ratatui::crossterm::event::KeyModifiers;
    use ratatui::{backend::TestBackend, Terminal};
    use std::net::Ipv4Addr;

    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    fn render(t: &mut IperfTab, ctx: &Ctx) -> String {
        let mut term = Terminal::new(TestBackend::new(120, 24)).unwrap();
        term.draw(|f| t.draw(f, f.area(), ctx)).unwrap();
        term.backend().buffer().content().chunks(120).map(|r| r.iter().map(|c| c.symbol()).collect::<String>().trim_end().to_string() + "\n").collect()
    }

    #[test]
    fn por_defecto_servidor_5201_y_edicion_de_cliente() {
        let iface = IfaceInfo { name: "end0".into(), ip: Ipv4Addr::new(192, 168, 1, 5), prefix: 24, gateway: None, mac: [0; 6] };
        let ctx = Ctx::new(state::new(iface, None), Arc::new(Config::default()));
        let mut t = IperfTab::new();
        let s = render(&mut t, &ctx);
        println!("{s}");
        assert!(s.contains("Servidor") && s.contains("192.168.1.5:5201") && s.contains("iperf.he.net:5201"));

        t.on_key(key(KeyCode::Char('p')), &ctx);
        assert!(t.capturing_input());
        for c in "5002".chars() {
            t.on_key(key(KeyCode::Char(c)), &ctx);
        }
        t.on_key(key(KeyCode::Enter), &ctx);
        assert_eq!(t.port, 5002);

        t.on_key(key(KeyCode::Char('d')), &ctx);
        assert!(t.mode == Mode::Client);
        for c in "iperf.example.org".chars() {
            t.on_key(key(KeyCode::Char(c)), &ctx);
        }
        t.on_key(key(KeyCode::Enter), &ctx);
        assert_eq!(t.host, "iperf.example.org");
        assert!(render(&mut t, &ctx).contains("iperf.example.org"));
    }
}
