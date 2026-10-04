//! Utilidades de formato compartidas por todas las pestañas.

pub fn mac_str(m: &[u8; 6]) -> String {
    format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", m[0], m[1], m[2], m[3], m[4], m[5])
}

/// Bytes con unidades binarias: 1.5 MB.
pub fn bytes(n: u64) -> String {
    let mut v = n as f64;
    for u in ["B", "KB", "MB", "GB", "TB"] {
        if v < 1024.0 || u == "TB" {
            return if u == "B" { format!("{} B", n) } else { format!("{:.1} {}", v, u) };
        }
        v /= 1024.0;
    }
    unreachable!()
}

/// Velocidad en bytes/s: 1.5 MB/s.
pub fn rate(bps: f64) -> String {
    if bps < 1.0 {
        return "0 B/s".into();
    }
    format!("{}/s", bytes(bps as u64))
}

/// Velocidad en bits/s (Mbit/s), la unidad habitual de los tests de velocidad.
pub fn mbit(bytes_per_s: f64) -> String {
    format!("{:.1} Mbit/s", bytes_per_s * 8.0 / 1e6)
}

pub fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
    }
}

pub fn ago(secs: u64) -> String {
    match secs {
        0..=59 => format!("{}s", secs),
        60..=3599 => format!("{}m", secs / 60),
        _ => format!("{}h", secs / 3600),
    }
}
