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
    pub scan_secs: u64,
    pub setcap: bool, // da CAP_NET_RAW al binario (pide sudo) y sale
    pub once: bool, // modo sin interfaz: escanea una vez, imprime la tabla y sale
}

impl Default for Config {
    fn default() -> Self {
        Config { iface: None, netflow_port: Some(2055), snmp: None, portscan: true, scan_secs: 60, setcap: false, once: false }
    }
}

pub const USAGE: &str = "\
lanwatch — monitor de red en terminal

USO: lanwatch [opciones]

  --iface <nombre>            interfaz a vigilar (por defecto, la de la ruta por defecto)
  --netflow <puerto|off>      colector NetFlow v5 para el tráfico por dispositivo (2055)
  --snmp <comunidad>[@host][/interfaz]
                              contadores del router por SNMP v2c (host = puerta de enlace)
  --portscan <on|off>         escaneo de puertos de los dispositivos (on)
  --scan-interval <segundos>  cada cuánto se barre la red (60)
  --setcap                    da CAP_NET_RAW al binario (sudo) para medir el TTL; también en la app (tecla c)
  --once                      sin interfaz: escanea una vez, imprime los dispositivos y sale
  --help, --version

También se leen de ~/.config/lanwatch.conf o /etc/lanwatch.conf (una opción por línea,
`clave=valor`, sin los guiones). Los argumentos tienen prioridad.
";

impl Config {
    pub fn load() -> Result<Config, String> {
        let mut c = Config::default();
        for path in conf_paths() {
            if let Ok(text) = std::fs::read_to_string(&path) {
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
        let mut args = std::env::args().skip(1);
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
            "scan-interval" => {
                self.scan_secs = val.parse::<u64>().map_err(|_| "scan-interval: número inválido".to_string())?.max(10)
            }
            _ => return Err(format!("opción desconocida: {key}\n\n{USAGE}")),
        }
        Ok(())
    }
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
