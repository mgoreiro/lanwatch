//! WiFi: interfaces inalámbricas, redes visibles y datos de la conexión actual.
//!
//! No se enlaza con ninguna biblioteca: se lee `/sys` y se usan, si existen, `nmcli` (NetworkManager,
//! escanea sin ser root) o `iw`. Los analizadores de texto son funciones puras para poder probarlos.

use std::process::{Command, Stdio};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Network {
    pub ssid: String,
    pub bssid: String,
    pub freq_mhz: u32,
    pub dbm: i32,
    pub security: String,
    pub in_use: bool,
    /// Solo con nmcli: modo (Infra/Mesh/Ad-Hoc), velocidad máxima anunciada, ancho de canal y cifrados.
    pub mode: String,
    pub rate: String,
    pub bandwidth: String,
    pub flags: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Link {
    pub iface: String,
    pub ssid: String,
    pub bssid: String,
    pub freq_mhz: u32,
    pub dbm: Option<i32>,
    pub rx_rate: Option<String>,
    pub tx_rate: Option<String>,
}

/// Interfaces inalámbricas (las que tienen `wireless` o `phy80211` en `/sys/class/net`).
pub fn interfaces() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir("/sys/class/net")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("wireless").exists() || e.path().join("phy80211").exists())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// Canal a partir de la frecuencia (2,4 GHz, 5 GHz y 6 GHz).
pub fn channel(freq: u32) -> Option<u32> {
    match freq {
        2484 => Some(14),
        2412..=2472 => Some((freq - 2407) / 5),
        5000..=5895 => Some((freq - 5000) / 5),
        5955..=7115 => Some((freq - 5950) / 5),
        _ => None,
    }
}

pub fn band(freq: u32) -> &'static str {
    match freq {
        2400..=2500 => "2,4 GHz",
        5000..=5900 => "5 GHz",
        5925..=7125 => "6 GHz",
        _ => "?",
    }
}

/// Calidad 0–100 a partir de la potencia recibida (−100 dBm = 0, −50 dBm o más = 100).
pub fn quality(dbm: i32) -> u32 {
    (2 * (dbm + 100)).clamp(0, 100) as u32
}

pub fn quality_label(dbm: i32) -> &'static str {
    match dbm {
        -55..=0 => "excelente",
        -65..=-56 => "buena",
        -75..=-66 => "regular",
        -85..=-76 => "débil",
        _ => "muy débil",
    }
}

fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd).args(args).stdin(Stdio::null()).output().map_err(|e| format!("{cmd}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        Err(if err.is_empty() { format!("{cmd} terminó con {}", out.status) } else { err.to_string() })
    }
}

fn have(cmd: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
}

// ───────────────────────────── nmcli ─────────────────────────────

/// Divide una línea del modo `-t` de nmcli: separador `:`, con `\:` y `\\` escapados.
fn split_terse(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut esc) = (Vec::new(), String::new(), false);
    for c in line.chars() {
        match (esc, c) {
            (true, c) => {
                cur.push(c);
                esc = false;
            }
            (false, '\\') => esc = true,
            (false, ':') => out.push(std::mem::take(&mut cur)),
            (false, c) => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Salida de `nmcli -t -f IN-USE,SSID,BSSID,FREQ,SIGNAL,SECURITY[,MODE,CHAN,RATE,BANDWIDTH,WPA-FLAGS,RSN-FLAGS] dev wifi list`.
pub fn parse_nmcli(text: &str) -> Vec<Network> {
    text.lines()
        .filter_map(|l| {
            let f = split_terse(l);
            if f.len() < 6 {
                return None;
            }
            let pct: i32 = f[4].trim().parse().ok()?;
            Some(Network {
                in_use: f[0].trim() == "*",
                ssid: f[1].clone(),
                bssid: f[2].to_lowercase(),
                freq_mhz: f[3].split_whitespace().next().and_then(|x| x.parse().ok()).unwrap_or(0),
                dbm: pct / 2 - 100, // nmcli da el porcentaje que calcula con esta misma fórmula
                security: if f[5].trim().is_empty() { "Abierta".into() } else { f[5].trim().to_string() },
                mode: f.get(6).cloned().unwrap_or_default(),
                rate: f.get(8).cloned().unwrap_or_default(),
                bandwidth: f.get(9).cloned().unwrap_or_default(),
                flags: [f.get(10), f.get(11)].into_iter().flatten().map(|x| x.trim()).filter(|x| !x.is_empty() && *x != "(none)").collect::<Vec<_>>().join(" · "),
            })
        })
        .collect()
}

// ─────────────────────────────── iw ──────────────────────────────

/// Salida de `iw dev X scan` / `scan dump`.
pub fn parse_iw_scan(text: &str) -> Vec<Network> {
    let mut v: Vec<Network> = Vec::new();
    let mut privacy = false;
    let mut wpa = (false, false); // (WPA, RSN)
    fn close(v: &mut [Network], privacy: bool, wpa: (bool, bool)) {
        if let Some(n) = v.last_mut() {
            n.security = match (wpa, privacy) {
                ((_, true), _) => "WPA2/3",
                ((true, _), _) => "WPA",
                (_, _) if privacy => "WEP",
                _ => "Abierta",
            }
            .into();
        }
    }
    for l in text.lines() {
        if let Some(rest) = l.strip_prefix("BSS ") {
            close(&mut v, privacy, wpa);
            privacy = false;
            wpa = (false, false);
            v.push(Network {
                bssid: rest.split(|c: char| c == '(' || c.is_whitespace()).next().unwrap_or("").to_lowercase(),
                in_use: rest.contains("associated"),
                ..Default::default()
            });
            continue;
        }
        let t = l.trim();
        let Some(n) = v.last_mut() else { continue };
        if let Some(x) = t.strip_prefix("freq:") {
            n.freq_mhz = x.trim().split('.').next().and_then(|x| x.parse().ok()).unwrap_or(0);
        } else if let Some(x) = t.strip_prefix("signal:") {
            n.dbm = x.trim().split(|c: char| c == '.' || c.is_whitespace()).next().and_then(|x| x.parse().ok()).unwrap_or(-100);
        } else if let Some(x) = t.strip_prefix("SSID:") {
            n.ssid = unescape_ssid(x.trim());
        } else if t.starts_with("capability:") && t.contains("Privacy") {
            privacy = true;
        } else if t.starts_with("RSN:") {
            wpa.1 = true;
        } else if t.starts_with("WPA:") {
            wpa.0 = true;
        }
    }
    close(&mut v, privacy, wpa);
    v
}

/// `iw` escapa los bytes no imprimibles del SSID como `\xNN`.
fn unescape_ssid(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1] == b'x' {
            if let Ok(x) = u8::from_str_radix(&s[i + 2..i + 4], 16) {
                out.push(x);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Salida de `iw dev X link`; `None` si no hay conexión.
pub fn parse_iw_link(iface: &str, text: &str) -> Option<Link> {
    let first = text.lines().next()?;
    let bssid = first.strip_prefix("Connected to ")?.split_whitespace().next()?.to_lowercase();
    let mut l = Link { iface: iface.into(), bssid, ..Default::default() };
    for t in text.lines().map(str::trim) {
        if let Some(x) = t.strip_prefix("SSID:") {
            l.ssid = unescape_ssid(x.trim());
        } else if let Some(x) = t.strip_prefix("freq:") {
            l.freq_mhz = x.trim().split('.').next().and_then(|x| x.parse().ok()).unwrap_or(0);
        } else if let Some(x) = t.strip_prefix("signal:") {
            l.dbm = x.trim().split_whitespace().next().and_then(|x| x.parse().ok());
        } else if let Some(x) = t.strip_prefix("rx bitrate:") {
            l.rx_rate = Some(x.trim().to_string());
        } else if let Some(x) = t.strip_prefix("tx bitrate:") {
            l.tx_rate = Some(x.trim().to_string());
        }
    }
    Some(l)
}

// ───────────────────────────── API ───────────────────────────────

/// Redes visibles, ordenadas de mayor a menor señal. Prueba `nmcli`, luego `iw scan` y por último la
/// caché del kernel (`iw scan dump`, que no necesita permisos).
pub fn scan(iface: &str) -> Result<Vec<Network>, String> {
    let mut errs = Vec::new();
    let mut nets = None;
    if have("nmcli") {
        match run("nmcli", &["-t", "-f", "IN-USE,SSID,BSSID,FREQ,SIGNAL,SECURITY,MODE,CHAN,RATE,BANDWIDTH,WPA-FLAGS,RSN-FLAGS", "dev", "wifi", "list", "ifname", iface, "--rescan", "yes"]) {
            Ok(t) => nets = Some(parse_nmcli(&t)),
            Err(e) => errs.push(e),
        }
    }
    if nets.is_none() && have("iw") {
        match run("iw", &["dev", iface, "scan"]).or_else(|_| run("iw", &["dev", iface, "scan", "dump"])) {
            Ok(t) => nets = Some(parse_iw_scan(&t)),
            Err(e) => errs.push(e),
        }
    }
    let mut nets = nets.ok_or_else(|| if errs.is_empty() { "hace falta nmcli o iw (apt install iw)".to_string() } else { errs.join(" · ") })?;
    nets.sort_by(|a, b| b.dbm.cmp(&a.dbm).then_with(|| a.ssid.cmp(&b.ssid)));
    Ok(nets)
}

/// Conexión actual de la interfaz, o `None` si no está asociada a ninguna red.
pub fn link(iface: &str) -> Option<Link> {
    if have("iw") {
        return run("iw", &["dev", iface, "link"]).ok().and_then(|t| parse_iw_link(iface, &t));
    }
    // Sin `iw`: la red marcada «en uso» por NetworkManager (sin velocidades de enlace).
    let net = scan_cached_nmcli(iface)?;
    Some(Link { iface: iface.into(), ssid: net.ssid, bssid: net.bssid, freq_mhz: net.freq_mhz, dbm: Some(net.dbm), ..Default::default() })
}

fn scan_cached_nmcli(iface: &str) -> Option<Network> {
    let t = run("nmcli", &["-t", "-f", "IN-USE,SSID,BSSID,FREQ,SIGNAL,SECURITY,MODE,CHAN,RATE,BANDWIDTH,WPA-FLAGS,RSN-FLAGS", "dev", "wifi", "list", "ifname", iface, "--rescan", "no"]).ok()?;
    parse_nmcli(&t).into_iter().find(|n| n.in_use)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nmcli_con_dos_puntos_escapados() {
        let t = "*:Casa:AA\\:BB\\:CC\\:DD\\:EE\\:FF:5180 MHz:78:WPA2\n:Vecino 2.4:11\\:22\\:33\\:44\\:55\\:66:2412 MHz:40:\n";
        let v = parse_nmcli(t);
        assert_eq!(v.len(), 2);
        assert!(v[0].in_use);
        assert_eq!((v[0].ssid.as_str(), v[0].bssid.as_str(), v[0].freq_mhz, v[0].dbm), ("Casa", "aa:bb:cc:dd:ee:ff", 5180, -61));
        assert_eq!(v[1].security, "Abierta");
        let c = parse_nmcli("*:Casa:AA\\:BB\\:CC\\:DD\\:EE\\:FF:5180 MHz:78:WPA2:Infra:36:540 Mbit/s:80 MHz:(none):pair_ccmp group_ccmp psk\n");
        assert_eq!((c[0].mode.as_str(), c[0].rate.as_str(), c[0].bandwidth.as_str(), c[0].flags.as_str()), ("Infra", "540 Mbit/s", "80 MHz", "pair_ccmp group_ccmp psk"));
        assert!(!v[1].in_use);
    }

    #[test]
    fn iw_scan_con_seguridad_y_asociada() {
        let t = "BSS aa:bb:cc:dd:ee:ff(on wlan0) -- associated\n\tfreq: 5180\n\tcapability: ESS Privacy (0x1431)\n\tsignal: -52.00 dBm\n\tSSID: Casa\n\tRSN:\t * Version: 1\n\
                 BSS 11:22:33:44:55:66(on wlan0)\n\tfreq: 2437\n\tcapability: ESS ShortPreamble (0x0421)\n\tsignal: -80.00 dBm\n\tSSID: Abierta\\x20X\n";
        let v = parse_iw_scan(t);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].ssid.as_str(), v[0].freq_mhz, v[0].dbm, v[0].security.as_str(), v[0].in_use), ("Casa", 5180, -52, "WPA2/3", true));
        assert_eq!((v[1].ssid.as_str(), v[1].security.as_str()), ("Abierta X", "Abierta"));
    }

    #[test]
    fn iw_link_conectado_y_no() {
        let t = "Connected to aa:bb:cc:dd:ee:ff (on wlan0)\n\tSSID: Casa\n\tfreq: 5180\n\tsignal: -52 dBm\n\trx bitrate: 433.3 MBit/s VHT-MCS 9\n\ttx bitrate: 390.0 MBit/s\n";
        let l = parse_iw_link("wlan0", t).unwrap();
        assert_eq!((l.ssid.as_str(), l.freq_mhz, l.dbm), ("Casa", 5180, Some(-52)));
        assert_eq!(l.rx_rate.as_deref(), Some("433.3 MBit/s VHT-MCS 9"));
        assert!(parse_iw_link("wlan0", "Not connected.\n").is_none());
    }

    #[test]
    fn canales_y_calidad() {
        assert_eq!((channel(2412), channel(2472), channel(2484), channel(5180), channel(5955)), (Some(1), Some(13), Some(14), Some(36), Some(1)));
        assert_eq!((quality(-30), quality(-75), quality(-100), quality(-120)), (100, 50, 0, 0));
        assert_eq!(quality_label(-52), "excelente");
        assert_eq!(quality_label(-90), "muy débil");
    }
}
