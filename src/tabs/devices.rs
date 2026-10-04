//! Pestaña a) Dispositivos de la red local.

use super::{Action, Ctx, Tab};
use crate::core::state::{self, Device};
use crate::util;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Frame;

#[derive(Clone, Copy, PartialEq)]
enum Sort {
    Ip,
    Down,
    Up,
}

pub struct Devices {
    table: TableState,
    sort: Sort,
    detail: bool,
    confirm_cap: bool, // esperando «s/n» para dar CAP_NET_RAW
    msg: String,
    msg_at: Option<std::time::Instant>, // los avisos desaparecen solos
}

impl Devices {
    pub fn new() -> Self {
        let mut table = TableState::default();
        table.select(Some(0));
        Devices { table, sort: Sort::Ip, detail: false, confirm_cap: false, msg: String::new(), msg_at: None }
    }
}

fn ports_str(d: &Device, max: usize) -> String {
    if d.scanned.is_none() {
        return "…".into();
    }
    if d.ports.is_empty() {
        return "–".into();
    }
    util::trunc(&d.ports.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(","), max)
}

impl Tab for Devices {
    fn title(&self) -> String {
        "Dispositivos".into()
    }

    fn help(&self) -> &'static str {
        "↑↓ mover · Enter detalle · s ordenar · r reescanear · c activar TTL (sudo setcap)"
    }

    fn capturing_input(&self) -> bool {
        self.confirm_cap
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &Ctx) {
        if self.confirm_cap {
            self.confirm_cap = false;
            if matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S') | KeyCode::Char('y')) {
                ctx.request(Action::GrantRawCap);
            }
            return;
        }
        let n = state::lock(&ctx.shared).devices.len();
        let sel = self.table.selected().unwrap_or(0);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.table.select(Some((sel + 1).min(n.saturating_sub(1)))),
            KeyCode::Up | KeyCode::Char('k') => self.table.select(Some(sel.saturating_sub(1))),
            KeyCode::PageDown => self.table.select(Some((sel + 10).min(n.saturating_sub(1)))),
            KeyCode::PageUp => self.table.select(Some(sel.saturating_sub(10))),
            KeyCode::Home => self.table.select(Some(0)),
            KeyCode::Enter => self.detail = !self.detail,
            KeyCode::Char('s') => {
                self.sort = match self.sort {
                    Sort::Ip => Sort::Down,
                    Sort::Down => Sort::Up,
                    Sort::Up => Sort::Ip,
                }
            }
            KeyCode::Char('r') => state::lock(&ctx.shared).force_scan = true,
            KeyCode::Char('c') => {
                if state::lock(&ctx.shared).raw_icmp {
                    self.msg = "CAP_NET_RAW ya está activo".into();
                    self.msg_at = Some(std::time::Instant::now());
                } else {
                    self.confirm_cap = true;
                }
            }
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame, area: Rect, ctx: &Ctx) {
        let st = state::lock(&ctx.shared);
        let mut devs: Vec<&Device> = st.devices.values().collect();
        match self.sort {
            Sort::Ip => {}
            Sort::Down => devs.sort_by(|a, b| b.flow.in_bps.total_cmp(&a.flow.in_bps)),
            Sort::Up => devs.sort_by(|a, b| b.flow.out_bps.total_cmp(&a.flow.out_bps)),
        }
        let online = devs.iter().filter(|d| d.online).count();
        let flow = &st.flow;
        let flow_txt = match (flow.listening, &flow.error) {
            (_, Some(e)) => format!("NetFlow: {e}"),
            (None, _) => "NetFlow: desactivado (tráfico por equipo no disponible)".into(),
            (Some(p), _) if flow.flows == 0 => format!("NetFlow: esperando datos del router en UDP {p}…"),
            (Some(p), _) => format!(
                "NetFlow UDP {p}: {} flujos · último hace {}",
                flow.flows,
                flow.last.map(|t| util::ago(t.elapsed().as_secs())).unwrap_or_default()
            ),
        };
        let scan = if st.scanning {
            "escaneando…".to_string()
        } else {
            st.last_sweep.map(|t| format!("último barrido hace {}", util::ago(t.elapsed().as_secs()))).unwrap_or_default()
        };
        let icmp = if st.raw_icmp { "" } else { "sin CAP_NET_RAW: sin TTL (ver README)" };
        let info = Paragraph::new(vec![
            Line::from(vec![
                Span::styled(format!(" {} ", st.iface.name), Style::new().bold()),
                Span::raw(
                    [
                        format!("{}/{}", st.iface.ip, st.iface.prefix),
                        format!("puerta de enlace {}", st.iface.gateway.map(|g| g.to_string()).unwrap_or("?".into())),
                        format!("{} dispositivos ({} en línea)", devs.len(), online),
                        scan,
                        icmp.to_string(),
                    ]
                    .into_iter()
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · "),
                ),
            ]),
            if self.confirm_cap {
                Line::styled(
                    format!(" ¿Ejecutar «{}»? Pedirá tu contraseña de sudo y reiniciará la app.   s = sí · otra tecla = no", crate::elevate::command_line()),
                    Style::new().fg(Color::Yellow).bold(),
                )
            } else if !self.msg.is_empty() && self.msg_at.is_some_and(|t| t.elapsed().as_secs() < 4) {
                Line::styled(format!(" {}", self.msg), Style::new().fg(Color::Green))
            } else {
                Line::styled(format!(" {flow_txt}"), Style::new().fg(Color::DarkGray))
            },
        ]);

        let detail_h = if self.detail { 8 } else { 0 };
        let [info_a, table_a, detail_a] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(3), Constraint::Length(detail_h)]).areas(area);
        f.render_widget(info, info_a);

        let netflow_on = flow.listening.is_some();
        let rate_cell = |d: &Device, v: f64| -> Cell {
            if !netflow_on {
                Cell::from("n/d").style(Style::new().fg(Color::DarkGray))
            } else if !d.flow.seen {
                Cell::from("–").style(Style::new().fg(Color::DarkGray))
            } else {
                Cell::from(util::rate(v))
            }
        };
        let rows: Vec<Row> = devs
            .iter()
            .map(|d| {
                let dim = if d.online { Style::new() } else { Style::new().fg(Color::DarkGray) };
                let dot = if d.online { Span::styled("●", Style::new().fg(Color::Green)) } else { Span::styled("○", Style::new().fg(Color::DarkGray)) };
                let tag = if d.is_gateway { " (gw)" } else if d.is_self { " (yo)" } else { "" };
                Row::new(vec![
                    Cell::from(Line::from(vec![dot, Span::raw(format!(" {}{}", d.ip, tag))])),
                    Cell::from(util::mac_str(&d.mac)),
                    Cell::from(util::trunc(&d.vendor, 20)),
                    Cell::from(util::trunc(d.hostname.as_deref().unwrap_or(""), 15)),
                    Cell::from(util::trunc(&d.os, 22)),
                    Cell::from(ports_str(d, 28)),
                    rate_cell(d, d.flow.in_bps),
                    rate_cell(d, d.flow.out_bps),
                ])
                .style(dim)
            })
            .collect();
        let sort_mark = |s: Sort, label: &str| if self.sort == s { format!("{label}▼") } else { label.to_string() };
        let header = Row::new(vec![
            "IP".to_string(),
            "MAC".into(),
            "Fabricante".into(),
            "Nombre".into(),
            "Sistema".into(),
            "Puertos abiertos".into(),
            sort_mark(Sort::Down, "↓ Entrante"),
            sort_mark(Sort::Up, "↑ Saliente"),
        ])
        .style(Style::new().add_modifier(Modifier::BOLD).fg(Color::Cyan));
        let table = Table::new(
            rows,
            [
                Constraint::Length(19),
                Constraint::Length(17),
                Constraint::Length(20),
                Constraint::Length(15),
                Constraint::Length(22),
                Constraint::Min(16),
                Constraint::Length(11),
                Constraint::Length(11),
            ],
        )
        .header(header)
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .column_spacing(1);
        if self.table.selected().is_some_and(|s| s >= devs.len()) {
            self.table.select(Some(devs.len().saturating_sub(1)));
        }
        f.render_stateful_widget(table, table_a, &mut self.table);

        if self.detail {
            if let Some(d) = self.table.selected().and_then(|i| devs.get(i)) {
                let ports = if d.ports.is_empty() { "ninguno detectado".to_string() } else { d.ports.iter().map(|p| format!("{p} ({})", service(*p))).collect::<Vec<_>>().join(", ") };
                let lines = vec![
                    Line::from(format!("IP {} · MAC {} · {}", d.ip, util::mac_str(&d.mac), if d.vendor.is_empty() { "fabricante desconocido" } else { &d.vendor })),
                    Line::from(format!("Nombre: {} · Sistema: {} · TTL: {}", d.hostname.as_deref().unwrap_or("–"), if d.os.is_empty() { "desconocido" } else { &d.os }, d.ttl.map(|t| t.to_string()).unwrap_or("–".into()))),
                    Line::from(format!("Puertos abiertos: {ports}")),
                    Line::from(format!(
                        "Tráfico desde el arranque (NetFlow): ↓ {} · ↑ {}",
                        util::bytes(d.flow.in_bytes),
                        util::bytes(d.flow.out_bytes)
                    )),
                    Line::from(format!("Visto hace {} · en la lista desde hace {}", util::ago(d.last_seen.elapsed().as_secs()), util::ago(d.first_seen.elapsed().as_secs()))),
                ];
                f.render_widget(
                    Paragraph::new(lines).wrap(Wrap { trim: true }).block(Block::new().borders(Borders::TOP | Borders::BOTTOM).title(" Detalle ")),
                    detail_a,
                );
            }
        }
    }
}

fn service(p: u16) -> &'static str {
    match p {
        21 => "ftp", 22 => "ssh", 23 => "telnet", 25 => "smtp", 53 => "dns", 80 => "http", 110 => "pop3", 111 => "rpc",
        135 => "msrpc", 139 => "netbios", 143 => "imap", 161 => "snmp", 443 => "https", 445 => "smb", 515 => "lpd",
        548 => "afp", 554 => "rtsp", 631 => "ipp", 993 => "imaps", 995 => "pop3s", 1723 => "pptp", 1883 => "mqtt",
        1900 => "upnp", 2049 => "nfs", 3306 => "mysql", 3389 => "rdp", 3689 => "daap", 5000 => "upnp/synology",
        5060 => "sip", 5432 => "postgres", 5555 => "adb", 5900 => "vnc", 6379 => "redis", 7659 => "iptv-tuner",
        7660 => "iptv-control", 8008 => "chromecast", 8009 => "chromecast", 8080 => "http-alt", 8096 => "jellyfin",
        8123 => "home-assistant", 8443 => "https-alt", 8554 => "rtsp-alt", 9100 => "jetdirect", 32400 => "plex",
        62078 => "iphone-sync", _ => "tcp",
    }
}
