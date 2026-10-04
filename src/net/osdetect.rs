//! Detección de sistema operativo **pasiva y heurística** (sin `nmap`): combina TTL, fabricante,
//! puertos abiertos y nombre. Es una estimación, no una huella exacta. Para mejorarla basta con
//! añadir reglas a `guess`, que recibe todas las señales disponibles.

pub struct Signals<'a> {
    pub ttl: Option<u8>,
    pub vendor: &'a str,
    pub ports: &'a [u16],
    pub hostname: Option<&'a str>,
    pub is_gateway: bool,
}

pub fn guess(s: &Signals) -> String {
    let v = s.vendor.to_lowercase();
    let name = s.hostname.unwrap_or("").to_lowercase();
    let has = |p: u16| s.ports.contains(&p);
    let any = |ps: &[u16]| ps.iter().any(|p| s.ports.contains(p));

    if s.is_gateway {
        return "Router / puerta de enlace".into();
    }
    // Por servicios muy característicos
    if has(62078) {
        return "iOS (iPhone/iPad)".into();
    }
    if any(&[9100, 515, 631]) && !any(&[3389, 445]) {
        return "Impresora".into();
    }
    if has(554) || has(8554) {
        return "Cámara IP / NVR".into();
    }
    if has(32400) {
        return "Servidor Plex".into();
    }
    if has(8008) || has(8009) {
        return "Chromecast / Android TV".into();
    }
    if has(3389) || (has(445) && has(135)) {
        return "Windows".into();
    }
    // Por fabricante
    if v.contains("apple") {
        return if any(&[22, 548, 5009]) || name.contains("mac") { "macOS" } else { "Apple (iOS/macOS)" }.into();
    }
    if v.contains("raspberry") {
        return "Linux (Raspberry Pi)".into();
    }
    for (needle, label) in [
        ("microsoft", "Windows / Xbox"),
        ("sony interactive", "PlayStation"),
        ("nintendo", "Nintendo"),
        ("espressif", "IoT (ESP32/ESP8266)"),
        ("tuya", "IoT (Tuya)"),
        ("amazon", "Amazon (Echo/Fire TV)"),
        ("google", "Google (Nest/Chromecast)"),
        ("synology", "Synology DSM"),
        ("qnap", "QNAP"),
        ("ubiquiti", "Ubiquiti"),
        ("tp-link", "Equipo de red TP-Link"),
        ("sonos", "Sonos"),
        ("samsung", "Samsung (Android/TV)"),
        ("xiaomi", "Xiaomi (Android/IoT)"),
        ("huawei", "Huawei (Android/red)"),
        ("orange pi", "Linux (Orange Pi)"),
        ("eero", "Router mesh eero"),
        ("ring", "Ring (cámara/timbre)"),
        ("tcl", "TCL (Android TV)"),
        ("lg electronics", "LG (webOS/TV)"),
        ("philips", "Philips (Hue/TV)"),
        ("signify", "Philips Hue"),
        ("hikvision", "Cámara Hikvision"),
        ("dahua", "Cámara Dahua"),
        ("hewlett", "HP (impresora/PC)"),
        ("brother", "Impresora Brother"),
        ("canon", "Canon (impresora)"),
        ("epson", "Epson (impresora)"),
        ("shenzhen", "IoT / Android"),
    ] {
        if v.contains(needle) {
            return label.into();
        }
    }
    // Por TTL
    match s.ttl {
        Some(t) if t > 128 => "Equipo de red (TTL 255)".into(),
        Some(t) if t > 64 => "Windows (TTL 128)".into(),
        Some(_) if has(22) => "Linux / Unix".into(),
        Some(_) => "Linux / Android / iOS (TTL 64)".into(),
        None if has(22) => "Linux / Unix".into(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(ttl: Option<u8>, vendor: &str, ports: &[u16]) -> String {
        guess(&Signals { ttl, vendor, ports, hostname: None, is_gateway: false })
    }

    #[test]
    fn por_puertos_y_fabricante() {
        assert_eq!(g(None, "", &[62078]), "iOS (iPhone/iPad)");
        assert_eq!(g(None, "HP", &[9100, 80]), "Impresora");
        assert_eq!(g(Some(128), "", &[445, 135]), "Windows");
        assert_eq!(g(None, "Espressif", &[]), "IoT (ESP32/ESP8266)");
    }

    #[test]
    fn por_ttl() {
        assert_eq!(g(Some(255), "", &[]), "Equipo de red (TTL 255)");
        assert_eq!(g(Some(64), "", &[22]), "Linux / Unix");
        assert_eq!(g(None, "", &[]), "");
    }
}
