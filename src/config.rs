//! Configuración: fichero `key=value` (opcional) y argumentos de línea de órdenes.
//! Los argumentos pisan al fichero. Añadir una opción = una entrada en `apply`.

use std::net::Ipv4Addr;

#[derive(Clone, Debug)]
pub struct SnmpCfg {
    pub host: Option<Ipv4Addr>, // None = la puerta de enlace
    pub community: String,
    pub ifname: Option<String>, // None = la primera interfaz que no sea "lo"
}

#[derive(Clone, Debug)]
pub struct Config {
    pub iface: Option<String>,
    pub netflow_port: Option<u16>, // None = desactivado
    pub snmp: Option<SnmpCfg>,
    pub portscan: bool,
    pub fingerprint: bool, // sondas mDNS/NetBIOS/SSDP a cada dispositivo
    pub scan_secs: u64,
    pub setcap: bool, // da CAP_NET_RAW al binario (pide sudo) y sale
    pub once: bool,
    pub source: Option<String>, // fichero de configuración que se leyó, si hubo // modo sin interfaz: escanea una vez, imprime la tabla y sale
}

impl Default for Config {
    fn default() -> Self {
        Config { iface: None, netflow_port: Some(2055), snmp: None, portscan: true, fingerprint: true, scan_secs: 60, setcap: false, once: false, source: None }
    }
}

pub const USAGE: &str = "\
lanwatch — monitor de red en terminal

USO: lanwatch [opciones]

  --iface <nombre>            interfaz a vigilar (por defecto, la de la ruta por defecto)
  --netflow <puerto|off>      colector NetFlow v5/v9/IPFIX para el tráfico por dispositivo (2055)
  --snmp <comunidad>[@host][/interfaz]
                              contadores del router por SNMP v2c (host = puerta de enlace)
  --portscan <on|off>         escaneo de puertos de los dispositivos (on)
  --fingerprint <on|off>      sondas mDNS/NetBIOS/SSDP para identificar mejor los equipos (on)
  --scan-interval <segundos>  cada cuánto se barre la red (60)
  --setcap                    da CAP_NET_RAW al binario (sudo) para medir el TTL; también en la app (tecla c)
  --once                      sin interfaz: escanea una vez, imprime los dispositivos y sale
  --help, --version

También se leen de ~/.config/lanwatch.conf o /etc/lanwatch.conf (una opción por línea,
`clave=valor`, sin los guiones). Los argumentos tienen prioridad.
";

impl Config {
    pub fn load() -> Result<Config, String> {
        Self::parse(conf_paths(), std::env::args().skip(1).collect())
    }

    /// Fichero (el primero que exista de `paths`) y después los argumentos, que tienen prioridad.
    pub fn parse(paths: Vec<String>, argv: Vec<String>) -> Result<Config, String> {
        let mut c = Config::default();
        for path in paths {
            if let Ok(text) = std::fs::read_to_string(&path) {
                c.source = Some(path.clone());
                for (n, line) in text.lines().enumerate() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    let (k, v) = line.split_once('=').ok_or(format!("{}:{}: se esperaba clave=valor", path, n + 1))?;
                    c.apply(k.trim(), v.trim()).map_err(|e| format!("{}:{}: {}", path, n + 1, e))?;
                }
                break;
            }
        }
        let mut args = argv.into_iter();
        while let Some(a) = args.next() {
            match a.as_str() {
                "--help" | "-h" => {
                    print!("{USAGE}");
                    std::process::exit(0);
                }
                "--once" => c.once = true,
                "--setcap" => c.setcap = true,
                "--version" | "-V" => {
                    println!("lanwatch {}", env!("CARGO_PKG_VERSION"));
                    std::process::exit(0);
                }
                _ if a.starts_with("--") => {
                    let (k, v) = match a[2..].split_once('=') {
                        Some((k, v)) => (k.to_string(), v.to_string()),
                        None => (a[2..].to_string(), args.next().ok_or(format!("falta el valor de {a}"))?),
                    };
                    c.apply(&k, &v)?;
                }
                _ => return Err(format!("argumento desconocido: {a}\n\n{USAGE}")),
            }
        }
        Ok(c)
    }

    fn apply(&mut self, key: &str, val: &str) -> Result<(), String> {
        match key {
            "iface" => self.iface = Some(val.to_string()),
            "netflow" => {
                self.netflow_port = match val {
                    "off" | "0" => None,
                    p => Some(p.parse().map_err(|_| format!("netflow: puerto inválido «{p}»"))?),
                }
            }
            "snmp" => {
                let (rest, ifname) = match val.split_once('/') {
                    Some((r, i)) => (r, Some(i.to_string())),
                    None => (val, None),
                };
                let (community, host) = match rest.split_once('@') {
                    Some((c, h)) => (c, Some(h.parse().map_err(|_| format!("snmp: IP inválida «{h}»"))?)),
                    None => (rest, None),
                };
                if community.is_empty() {
                    return Err("snmp: falta la comunidad".into());
                }
                self.snmp = Some(SnmpCfg { host, community: community.to_string(), ifname });
            }
            "portscan" => self.portscan = matches!(val, "on" | "1" | "true" | "si" | "sí"),
            "fingerprint" => self.fingerprint = matches!(val, "on" | "1" | "true" | "si" | "sí"),
            "scan-interval" => {
                self.scan_secs = val.parse::<u64>().map_err(|_| "scan-interval: número inválido".to_string())?.max(10)
            }
            _ => return Err(format!("opción desconocida: {key}\n\n{USAGE}")),
        }
        Ok(())
    }
}

impl SnmpCfg {
    /// Forma `comunidad@host/interfaz`, la misma que acepta `--snmp` y el fichero de configuración.
    pub fn to_value(&self) -> String {
        let mut v = self.community.clone();
        if let Some(h) = self.host {
            v.push_str(&format!("@{h}"));
        }
        if let Some(i) = &self.ifname {
            v.push_str(&format!("/{i}"));
        }
        v
    }
}

/// Fichero donde se guardan los cambios hechos desde la interfaz: el primero que exista o, si no
/// hay ninguno, `~/.config/lanwatch.conf`.
pub fn save_path() -> Option<String> {
    if let Ok(p) = std::env::var("LANWATCH_CONF") {
        return Some(p); // el usuario (o una prueba) eligió el fichero explícitamente
    }
    let paths = conf_paths();
    paths.iter().find(|p| std::path::Path::new(p.as_str()).exists()).cloned().or_else(|| {
        std::env::var("HOME").ok().map(|h| format!("{h}/.config/lanwatch.conf"))
    })
}

/// Valor de `snmp=` que hay **ahora mismo en el fichero** (lo que se recuperará al arrancar).
pub fn persisted_snmp() -> Option<(String, String)> {
    let path = save_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    text.lines().find_map(|l| l.split_once('=').filter(|(k, _)| k.trim() == "snmp").map(|(_, v)| (path.clone(), v.trim().to_string())))
}

/// Pone (o quita, con `None`) la línea `snmp=` del fichero conservando el resto.
pub fn save_snmp_to(path: &str, snmp: Option<&SnmpCfg>) -> Result<(), String> {
    let old = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = old.lines().filter(|l| l.split_once('=').is_none_or(|(k, _)| k.trim() != "snmp")).map(String::from).collect();
    if let Some(s) = snmp {
        lines.push(format!("snmp={}", s.to_value()));
    }
    if let Some(dir) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))
}

fn conf_paths() -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(p) = std::env::var("LANWATCH_CONF") {
        v.push(p);
    }
    if let Ok(h) = std::env::var("HOME") {
        v.push(format!("{h}/.config/lanwatch.conf"));
    }
    v.push("/etc/lanwatch.conf".into());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarda_y_quita_snmp_conservando_el_resto() {
        let path = std::env::temp_dir().join(format!("lanwatch-test-{}.conf", std::process::id()));
        let path = path.to_str().unwrap();
        std::fs::write(path, "# mi config\nnetflow=2055\nsnmp=vieja\n").unwrap();
        let cfg = SnmpCfg { host: Some("192.168.1.1".parse().unwrap()), community: "public".into(), ifname: Some("eth0".into()) };
        save_snmp_to(path, Some(&cfg)).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "# mi config\nnetflow=2055\nsnmp=public@192.168.1.1/eth0\n");
        save_snmp_to(path, None).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "# mi config\nnetflow=2055\n");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn el_valor_se_puede_volver_a_leer() {
        let mut c = Config::default();
        c.apply("snmp", "public@10.0.0.1/eth0").unwrap();
        assert_eq!(c.snmp.unwrap().to_value(), "public@10.0.0.1/eth0");
    }

    #[test]
    fn el_snmp_guardado_se_recupera_al_arrancar() {
        let path = std::env::temp_dir().join(format!("lanwatch-boot-{}.conf", std::process::id()));
        let p = path.to_str().unwrap().to_string();
        let cfg = SnmpCfg { host: Some("192.168.1.1".parse().unwrap()), community: "mi-comunidad".into(), ifname: Some("eth0".into()) };
        save_snmp_to(&p, Some(&cfg)).unwrap(); // lo que hace el asistente al elegir la interfaz
        let loaded = Config::parse(vec![p.clone()], vec![]).unwrap(); // lo que hace el arranque
        let s = loaded.snmp.expect("el SNMP guardado debe cargarse");
        assert_eq!((s.community.as_str(), s.host, s.ifname.as_deref()), ("mi-comunidad", Some("192.168.1.1".parse().unwrap()), Some("eth0")));
        assert_eq!(loaded.source.as_deref(), Some(p.as_str()));
        // un argumento tiene prioridad sobre el fichero
        let over = Config::parse(vec![p.clone()], vec!["--snmp".into(), "otra@10.0.0.1/eth1".into()]).unwrap();
        assert_eq!(over.snmp.unwrap().to_value(), "otra@10.0.0.1/eth1");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn sin_fichero_arranca_sin_snmp() {
        let c = Config::parse(vec!["/no/existe.conf".into()], vec![]).unwrap();
        assert!(c.snmp.is_none() && c.source.is_none());
    }
}
