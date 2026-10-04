//! Detección de sistema operativo **pasiva y heurística** (sin `nmap`): combina TTL, fabricante,
//! puertos abiertos y nombre. Es una estimación, no una huella exacta. Para mejorarla basta con
//! añadir reglas a `guess`, que recibe todas las señales disponibles.

#[derive(Default)]
pub struct Signals<'a> {
    pub ttl: Option<u8>,
    pub vendor: &'a str,
    pub ports: &'a [u16],
    pub hostname: Option<&'a str>,
    pub is_gateway: bool,
    /// Servicios mDNS anunciados (`_airplay._tcp`, `_googlecast._tcp`…).
    pub services: &'a [String],
    /// Modelo declarado por el propio equipo (mDNS `_device-info`: «MacBookAir10,1», «iPhone14,2»…).
    pub model: Option<&'a str>,
    /// Nombre NetBIOS (Windows/Samba).
    pub netbios: Option<&'a str>,
    /// Cabecera `SERVER` de UPnP/SSDP («Linux/4.9 UPnP/1.0 …»).
    pub ssdp: Option<&'a str>,
}

pub fn guess(s: &Signals) -> String {
    let v = s.vendor.to_lowercase();
    let name = s.hostname.unwrap_or("").to_lowercase();
    let has = |p: u16| s.ports.contains(&p);
    let any = |ps: &[u16]| ps.iter().any(|p| s.ports.contains(p));

    if s.is_gateway {
        return "Router / puerta de enlace".into();
    }
    // Lo que el equipo declara de sí mismo es más fiable que lo que se deduce de fuera.
    if let Some(m) = s.model {
        let label = if m.starts_with("iPhone") || m.starts_with("iPad") {
            Some(format!("iOS ({m})"))
        } else if m.starts_with("AppleTV") {
            Some("tvOS (Apple TV)".to_string())
        } else if m.starts_with("AudioAccessory") {
            Some("HomePod".to_string())
        } else if m.starts_with("Mac") || m.starts_with("iMac") {
            Some(format!("macOS ({m})"))
        } else {
            None
        };
        if let Some(l) = label {
            return l;
        }
    }
    // Por el nombre que el propio equipo anuncia («MacBook-Air-de-Ana», «iPhone-de-Luis», «android-…»).
    for (needle, label) in [
        ("macbook", "macOS (MacBook)"),
        ("imac", "macOS (iMac)"),
        ("macmini", "macOS (Mac mini)"),
        ("mac-mini", "macOS (Mac mini)"),
        ("iphone", "iOS (iPhone)"),
        ("ipad", "iOS (iPad)"),
        ("appletv", "tvOS (Apple TV)"),
        ("apple-tv", "tvOS (Apple TV)"),
        ("android", "Android"),
        ("galaxy", "Android (Samsung Galaxy)"),
        ("pixel", "Android (Google Pixel)"),
        ("chromecast", "Chromecast"),
    ] {
        if name.contains(needle) {
            return label.into();
        }
    }
    let svc = |p: &str| s.services.iter().any(|x| x.starts_with(p));
    if svc("_googlecast") {
        return "Chromecast / Android TV".into();
    }
    if svc("_airplay") || svc("_raop") {
        return if svc("_companion-link") || v.contains("apple") { "Apple (AirPlay)" } else { "Receptor AirPlay (TV/altavoz)" }.into();
    }
    if svc("_hap") {
        return "Accesorio HomeKit".into();
    }
    if svc("_ipp") || svc("_printer") || svc("_pdl-datastream") {
        return "Impresora".into();
    }
    for (prefix, label) in [("_sonos", "Sonos"), ("_hue", "Philips Hue"), ("_esphomelib", "ESPHome (IoT)")] {
        if svc(prefix) {
            return label.into();
        }
    }
    if let Some(server) = s.ssdp {
        let sv = server.to_lowercase();
        for (needle, label) in [
            ("windows", "Windows (UPnP)"),
            ("android", "Android (UPnP)"),
            ("synology", "Synology DSM"),
            ("roku", "Roku"),
            ("tizen", "Samsung (Tizen TV)"),
            ("darwin", "macOS / iOS (UPnP)"),
            ("mac os", "macOS (UPnP)"),
        ] {
            if sv.contains(needle) {
                return label.into();
            }
        }
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
    if s.netbios.is_some() {
        return "Windows / Samba (NetBIOS)".into();
    }
    if let Some(server) = s.ssdp {
        if server.to_lowercase().contains("linux") {
            return "Linux (UPnP)".into();
        }
    }
    if svc("_workstation") || svc("_ssh") {
        return "Linux / Unix (avahi)".into();
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
        guess(&Signals { ttl, vendor, ports, ..Default::default() })
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

    fn svc(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn por_el_nombre_del_equipo() {
        let n = |h: &'static str| guess(&Signals { hostname: Some(h), ..Default::default() });
        assert_eq!(n("MacBook-Air-de-Miguel"), "macOS (MacBook)");
        assert_eq!(n("iPhone-de-Ana"), "iOS (iPhone)");
        assert_eq!(n("Android"), "Android");
        assert_eq!(n("impresora-salon"), "");
    }

    #[test]
    fn el_modelo_declarado_manda_sobre_todo() {
        let sig = |m: &'static str| Signals { model: Some(m), vendor: "Apple", ports: &[3389], ..Default::default() };
        assert_eq!(guess(&sig("MacBookAir10,1")), "macOS (MacBookAir10,1)");
        assert_eq!(guess(&sig("iPhone14,2")), "iOS (iPhone14,2)");
        assert_eq!(guess(&sig("AppleTV6,2")), "tvOS (Apple TV)");
    }

    #[test]
    fn servicios_mdns() {
        let c = svc(&["_googlecast._tcp", "_airplay._tcp"]);
        assert_eq!(guess(&Signals { services: &c, ..Default::default() }), "Chromecast / Android TV");
        let a = svc(&["_airplay._tcp", "_companion-link._tcp"]);
        assert_eq!(guess(&Signals { services: &a, ..Default::default() }), "Apple (AirPlay)");
        let tv = svc(&["_airplay._tcp"]);
        assert_eq!(guess(&Signals { services: &tv, vendor: "LG Electronics", ..Default::default() }), "Receptor AirPlay (TV/altavoz)");
        let p = svc(&["_ipp._tcp", "_printer._tcp"]);
        assert_eq!(guess(&Signals { services: &p, ..Default::default() }), "Impresora");
    }

    #[test]
    fn ssdp_y_netbios() {
        assert_eq!(guess(&Signals { ssdp: Some("Microsoft-Windows/10.0 UPnP/1.0"), ..Default::default() }), "Windows (UPnP)");
        assert_eq!(guess(&Signals { ssdp: Some("Linux/4.9 UPnP/1.0 MiniUPnPd/2.1"), ..Default::default() }), "Linux (UPnP)");
        assert_eq!(guess(&Signals { netbios: Some("PC-SALON"), ..Default::default() }), "Windows / Samba (NetBIOS)");
        // los puertos de Windows siguen ganando al NetBIOS genérico
        assert_eq!(guess(&Signals { netbios: Some("PC"), ports: &[3389], ..Default::default() }), "Windows");
    }
}
