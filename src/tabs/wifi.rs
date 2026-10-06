//! Pestaña WiFi: redes visibles con la calidad de su señal y, arriba, los datos de la conexión actual.
//! Sin interfaz inalámbrica muestra «WiFi no disponible»; si no está conectado, no hay recuadro superior.

use super::{Ctx, Tab};
use crate::net::{iface, wifi};
use crate::util;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Clear, Paragraph, Row, Table, TableState};
use ratatui::Frame;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Cada cuánto se repite el barrido mientras la pestaña está a la vista (un barrido interrumpe unos
/// instantes el tráfico del adaptador, así que no se hace en segundo plano ni más a menudo).
const RESCAN: Duration = Duration::from_secs(15);

#[derive(Default)]
struct Data {
    scanning: bool,
    nets: Vec<wifi::Network>,
    link: Option<wifi::Link>,
    addr: Option<(Ipv4Addr, u8)>,
    gateway: Option<Ipv4Addr>,
    mac: [u8; 6],
    error: Option<String>,
    last: Option<Instant>,
}

pub struct WifiTab {
    iface: Option<String>,
    data: Arc<Mutex<Data>>,
    sel: TableState,
    detail: bool,
}

impl WifiTab {
    pub fn new() -> Self {
        WifiTab { iface: wifi::interfaces().into_iter().next(), data: Arc::new(Mutex::new(Data::default())), sel: TableState::default(), detail: false }
    }

    fn scan(&self) {
        let Some(name) = self.iface.clone() else { return };
        {
            let mut d = self.data.lock().unwrap();
            if d.scanning {
                return;
            }
            d.scanning = true;
        }
        let data = self.data.clone();
        std::thread::spawn(move || {
            let link = wifi::link(&name);
            let (addr, gateway, mac) = iface::details(&name);
            let res = wifi::scan(&name);
            let mut d = data.lock().unwrap();
            d.link = link;
            (d.addr, d.gateway, d.mac) = (addr, gateway, mac);
            match res {
                Ok(n) => {
                    d.nets = n;
                    d.error = None;
                }
                Err(e) => d.error = Some(e),
            }
            d.last = Some(Instant::now());
            d.scanning = false;
        });
    }
}

fn color(dbm: i32) -> Color {
    match dbm {
        -55..=0 => Color::Green,
        -65..=-56 => Color::LightGreen,
        -75..=-66 => Color::Yellow,
        -85..=-76 => Color::LightRed,
        _ => Color::Red,
    }
}

fn bar(dbm: i32, width: usize) -> Span<'static> {
    let full = (wifi::quality(dbm) as usize * width).div_ceil(100).min(width);
    Span::styled(format!("{}{}", "█".repeat(full), "░".repeat(width - full)), Style::new().fg(color(dbm)))
}

fn chan(freq: u32) -> String {
    match wifi::channel(freq) {
        Some(c) => format!("{c:>3} · {}", wifi::band(freq)),
        None => "?".into(),
    }
}

impl Tab for WifiTab {
    fn title(&self) -> String {
        "WiFi".into()
    }

    fn help(&self) -> &'static str {
        "Enter detalle de la red · r buscar ahora · ↑↓ elegir · se actualiza cada 15 s"
    }

    fn busy(&self) -> bool {
        self.data.lock().unwrap().scanning
    }

    fn on_key(&mut self, key: KeyEvent, _ctx: &Ctx) {
        let n = self.data.lock().unwrap().nets.len();
        let s = self.sel.selected().unwrap_or(0);
        match key.code {
            KeyCode::Char('r') => self.scan(),
            KeyCode::Enter if n > 0 => self.detail = !self.detail,
            KeyCode::Esc => self.detail = false,
            KeyCode::Down | KeyCode::Char('j') if n > 0 => self.sel.select(Some((s + 1).min(n - 1))),
            KeyCode::Up | KeyCode::Char('k') => self.sel.select(Some(s.saturating_sub(1))),
            _ => {}
        }
    }

    fn on_tick(&mut self, _ctx: &Ctx) {
        let due = {
            let d = self.data.lock().unwrap();
            !d.scanning && d.last.is_none_or(|t| t.elapsed() >= RESCAN)
        };
        if due {
            self.scan();
        }
    }

    fn draw(&mut self, f: &mut Frame, area: Rect, _ctx: &Ctx) {
        let Some(name) = &self.iface else {
            let [_, mid, _] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1), Constraint::Fill(1)]).areas(area);
            f.render_widget(Paragraph::new("WiFi no disponible").alignment(Alignment::Center).style(Style::new().fg(Color::DarkGray).bold()), mid);
            return;
        };
        if self.data.lock().unwrap().last.is_none() {
            self.scan(); // primera vez que se dibuja: no esperar al reloj
        }
        let d = self.data.lock().unwrap();

        let (top, list) = if d.link.is_some() {
            let [a, b] = Layout::vertical([Constraint::Length(7), Constraint::Min(4)]).areas(area);
            (Some(a), b)
        } else {
            (None, area)
        };

        if let (Some(top), Some(l)) = (top, &d.link) {
            let k = Style::new().fg(Color::DarkGray);
            let row = |a: &str, av: Vec<Span<'static>>, b: &str, bv: String| {
                let mut spans = vec![Span::styled(format!("{a:<11}"), k)];
                spans.extend(av);
                spans.push(Span::styled(format!("{b:<13}"), k));
                spans.push(Span::raw(bv));
                Line::from(spans)
            };
            let pad = |s: String| vec![Span::raw(format!("{s:<30}"))];
            let sig = l.dbm.map(|x| format!("{x} dBm · {} ({} %)", wifi::quality_label(x), wifi::quality(x))).unwrap_or("–".into());
            let lines = vec![
                row("Red", vec![Span::styled(format!("{:<30}", util::trunc(&l.ssid, 30)), Style::new().bold().fg(Color::Green))], "BSSID", l.bssid.clone()),
                row("Interfaz", pad(name.clone()), "MAC", util::mac_str(&d.mac)),
                row("Canal", pad(chan(l.freq_mhz)), "IP", d.addr.map(|(i, p)| format!("{i}/{p}")).unwrap_or("–".into())),
                row("Señal", pad(sig), "Puerta enl.", d.gateway.map(|g| g.to_string()).unwrap_or("–".into())),
                row("Recibe", pad(l.rx_rate.clone().unwrap_or("–".into())), "Envía", l.tx_rate.clone().unwrap_or("–".into())),
            ];
            f.render_widget(Paragraph::new(lines).block(Block::bordered().title(" Conectado a ").border_style(Style::new().fg(Color::Green))), top);
        }

        let title = if d.scanning {
            format!(" Redes disponibles ({}) — buscando… ", d.nets.len())
        } else if let Some(e) = &d.error {
            format!(" Redes disponibles — {} ", util::trunc(e, 70))
        } else {
            format!(" Redes disponibles ({}) · {} ", d.nets.len(), name)
        };
        let rows: Vec<Row> = d
            .nets
            .iter()
            .map(|n| {
                let ssid = if n.ssid.is_empty() { "(oculta)".to_string() } else { n.ssid.clone() };
                let style = if n.in_use { Style::new().bold().fg(Color::Green) } else { Style::new() };
                Row::new(vec![
                    Cell::from(if n.in_use { "★" } else { " " }),
                    Cell::from(util::trunc(&ssid, 32)).style(style),
                    Cell::from(chan(n.freq_mhz)),
                    Cell::from(Line::from(bar(n.dbm, 10))),
                    Cell::from(format!("{} dBm", n.dbm)),
                    Cell::from(Span::styled(wifi::quality_label(n.dbm), Style::new().fg(color(n.dbm)))),
                    Cell::from(n.security.clone()),
                    Cell::from(n.bssid.clone()).style(Style::new().fg(Color::DarkGray)),
                ])
            })
            .collect();
        let widths = [Constraint::Length(1), Constraint::Length(32), Constraint::Length(14), Constraint::Length(10), Constraint::Length(8), Constraint::Length(10), Constraint::Length(10), Constraint::Min(17)];
        let t = Table::new(rows, widths)
            .header(Row::new(vec!["", "Red", "Canal", "Señal", "Potencia", "Calidad", "Seguridad", "BSSID"]).style(Style::new().bold().fg(Color::Cyan)))
            .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
            .column_spacing(1)
            .block(Block::bordered().title(title));
        f.render_stateful_widget(t, list, &mut self.sel);
        if self.detail {
            let sel = self.sel.selected().unwrap_or(0).min(d.nets.len().saturating_sub(1));
            if let Some(n) = d.nets.get(sel) {
                draw_detail(f, area, n, &d.nets);
            }
        }
    }
}

fn draw_detail(f: &mut Frame, area: Rect, n: &wifi::Network, all: &[wifi::Network]) {
    let k = Style::new().fg(Color::DarkGray);
    let row = |a: &str, v: String| Line::from(vec![Span::styled(format!("{a:<25}"), k), Span::raw(v)]);
    let ch = wifi::channel(n.freq_mhz);
    let same_ssid: Vec<&wifi::Network> = all.iter().filter(|x| x.ssid == n.ssid && !n.ssid.is_empty()).collect();
    let same_chan = all.iter().filter(|x| x.bssid != n.bssid && x.freq_mhz == n.freq_mhz).count();
    let adjacent = all.iter().filter(|x| x.bssid != n.bssid && x.freq_mhz != n.freq_mhz && x.freq_mhz.abs_diff(n.freq_mhz) < 25 && n.freq_mhz < 3000).count();
    let mac: Option<[u8; 6]> = {
        let p: Vec<u8> = n.bssid.split(':').filter_map(|x| u8::from_str_radix(x, 16).ok()).collect();
        <[u8; 6]>::try_from(p).ok()
    };
    let vendor = mac.map(|m| crate::net::oui::vendor(&m)).filter(|v| !v.is_empty()).unwrap_or("desconocido".into());
    let mode = match n.mode.as_str() {
        "Infra" => "Infraestructura (punto de acceso)".to_string(),
        "Mesh" => "Malla (mesh)".to_string(),
        "Ad-Hoc" => "Ad-hoc (equipo a equipo)".to_string(),
        "" => "–".to_string(),
        x => x.to_string(),
    };
    let mut l = vec![
        row("Nombre (SSID)", if n.ssid.is_empty() { "(oculta)".into() } else { n.ssid.clone() }),
        row("Estado", if n.in_use { "★ conectado a esta red".into() } else { "disponible".into() }),
        row("BSSID (MAC del AP)", n.bssid.clone()),
        row("Fabricante del AP", vendor),
        row("Tipo", mode),
        row("Banda y canal", format!("{} · canal {} · {} MHz", wifi::band(n.freq_mhz), ch.map(|c| c.to_string()).unwrap_or("?".into()), n.freq_mhz)),
        row("Ancho de canal", if n.bandwidth.is_empty() { "–".into() } else { n.bandwidth.clone() }),
        row("Velocidad máx. anunciada", if n.rate.is_empty() { "–".into() } else { n.rate.clone() }),
        Line::from(vec![Span::styled(format!("{:<25}", "Señal"), k), bar(n.dbm, 10), Span::raw(format!("  {} dBm · {} ({} %)", n.dbm, wifi::quality_label(n.dbm), wifi::quality(n.dbm)))]),
        row("Seguridad", n.security.clone()),
        row("Cifrado / claves", if n.flags.is_empty() { "–".into() } else { n.flags.clone() }),
    ];
    if !same_ssid.is_empty() {
        let best = same_ssid.iter().map(|x| x.dbm).max().unwrap_or(n.dbm);
        l.push(row("Puntos de acceso", format!("{} con este nombre (el mejor, {} dBm)", same_ssid.len(), best)));
    }
    l.push(row("Congestión", format!("{same_chan} más en el mismo canal{}", if n.freq_mhz < 3000 { format!(" · {adjacent} en canales que se solapan") } else { String::new() })));
    let (w, h) = (area.width.min(84), (l.len() as u16 + 2).min(area.height));
    let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(l).block(Block::bordered().title(" Detalle de la red · Enter/Esc cierra ").border_style(Style::new().fg(Color::Cyan))), r);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::core::state;
    use crate::net::iface::IfaceInfo;
    use ratatui::{backend::TestBackend, Terminal};

    fn text(tab: &mut WifiTab) -> String {
        let iface = IfaceInfo { name: "wlan0".into(), ip: Ipv4Addr::new(192, 168, 1, 5), prefix: 24, gateway: None, mac: [0; 6] };
        let ctx = Ctx::new(state::new(iface, None), Arc::new(Config::default()));
        let mut term = Terminal::new(TestBackend::new(120, 22)).unwrap();
        term.draw(|f| tab.draw(f, f.area(), &ctx)).unwrap();
        term.backend().buffer().content().chunks(120).map(|r| r.iter().map(|c| c.symbol()).collect::<String>().trim_end().to_string() + "\n").collect()
    }

    #[test]
    fn sin_wifi_y_con_conexion() {
        let mut t = WifiTab { iface: None, data: Arc::new(Mutex::new(Data::default())), sel: TableState::default(), detail: false };
        assert!(text(&mut t).contains("WiFi no disponible"));

        let net = |s: &str, f, d, sec: &str, u| wifi::Network { ssid: s.into(), bssid: "aa:bb:cc:dd:ee:ff".into(), freq_mhz: f, dbm: d, security: sec.into(), in_use: u, mode: "Infra".into(), rate: "540 Mbit/s".into(), bandwidth: "80 MHz".into(), flags: "pair_ccmp group_ccmp psk".into() };
        let data = Data {
            nets: vec![net("Casa", 5180, -52, "WPA2", true), net("Vecino", 2437, -78, "WPA2", false), net("", 2412, -88, "Abierta", false)],
            link: Some(wifi::Link { iface: "wlan0".into(), ssid: "Casa".into(), bssid: "aa:bb:cc:dd:ee:ff".into(), freq_mhz: 5180, dbm: Some(-52), rx_rate: Some("433 MBit/s".into()), tx_rate: Some("390 MBit/s".into()) }),
            addr: Some((Ipv4Addr::new(192, 168, 1, 5), 24)),
            last: Some(Instant::now()),
            ..Default::default()
        };
        let mut t = WifiTab { iface: Some("wlan0".into()), data: Arc::new(Mutex::new(data)), sel: TableState::default(), detail: false };
        let con = text(&mut t);
        println!("{con}");
        assert!(con.contains("Conectado a") && con.contains("(oculta)") && con.contains("excelente"));

        t.detail = true;
        t.sel.select(Some(0));
        let det = text(&mut t);
        println!("{det}");
        assert!(det.contains("Detalle de la red") && det.contains("canal 36") && det.contains("más en el mismo canal"));
        t.detail = false;

        t.data.lock().unwrap().link = None;
        assert!(!text(&mut t).contains("Conectado a"));
    }
}
