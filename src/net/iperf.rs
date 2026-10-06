//! iperf3: lista de servidores públicos y ejecución del binario `iperf3` (cliente o servidor).
//! No se implementa el protocolo: se lanza el programa y se interpreta su salida de texto.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

pub const DEFAULT_PORT: u16 = 5201;

pub struct PublicServer {
    pub host: &'static str,
    pub port: u16,
    pub place: &'static str,
}

/// Servidores públicos conocidos (los mantienen voluntarios: pueden estar ocupados o caídos).
/// Lista más completa y viva: https://iperf3serverlist.net
pub const SERVERS: &[PublicServer] = &[
    PublicServer { host: "iperf.he.net", port: 5201, place: "Hurricane Electric · EE. UU." },
    PublicServer { host: "ping.online.net", port: 5200, place: "Scaleway · Francia" },
    PublicServer { host: "ping-90ms.online.net", port: 5200, place: "Scaleway · Francia (90 ms artificiales)" },
    PublicServer { host: "speedtest.serverius.net", port: 5002, place: "Serverius · Países Bajos" },
    PublicServer { host: "iperf.eenet.ee", port: 5201, place: "EENet · Estonia" },
    PublicServer { host: "iperf.volia.net", port: 5201, place: "Volia · Ucrania" },
    PublicServer { host: "iperf.biznetnetworks.com", port: 5201, place: "Biznet · Indonesia" },
    PublicServer { host: "speedtest.wtnet.de", port: 5200, place: "WTNet · Alemania" },
    PublicServer { host: "iperf.par2.as49434.net", port: 9200, place: "Harmony Hosting · Francia" },
    PublicServer { host: "iperf3.moji.fr", port: 5200, place: "Moji · Francia" },
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Server,
    Client,
}

#[derive(Clone, Debug)]
pub struct Params {
    pub mode: Mode,
    pub port: u16,
    pub host: String,
    /// Solo cliente: el servidor envía y este equipo recibe (`-R`), es decir, mide la descarga.
    pub reverse: bool,
    pub secs: u32,
}

pub fn args(p: &Params) -> Vec<String> {
    let mut a: Vec<String> = vec!["-p".into(), p.port.to_string(), "-f".into(), "m".into(), "-i".into(), "1".into(), "--forceflush".into()];
    match p.mode {
        Mode::Server => a.push("-s".into()),
        Mode::Client => {
            a.extend(["-c".into(), p.host.clone(), "-t".into(), p.secs.to_string(), "--connect-timeout".into(), "5000".into()]);
            if p.reverse {
                a.push("-R".into());
            }
        }
    }
    a
}

/// Estado compartido con la interfaz.
#[derive(Default)]
pub struct Run {
    pub running: bool,
    pub lines: Vec<String>,
    /// Bytes/s de cada intervalo (para la gráfica y para `util::mbit`).
    pub hist: Vec<u64>,
    pub error: Option<String>,
    pub finished: bool,
    /// Terminó porque el servidor está ocupado o el puerto en uso: tiene sentido esperar y reintentar.
    pub retryable: bool,
}

/// ¿El mensaje de iperf3 indica ocupación (servidor con otra prueba, o puerto ya en uso)?
pub fn is_busy(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("server is busy") || m.contains("address already in use")
}

/// Mensaje de iperf3 traducido a algo comprensible.
pub fn explain(msg: &str) -> String {
    let m = msg.to_lowercase();
    if m.contains("server is busy") {
        "El servidor está ocupado con otra prueba.".into()
    } else if m.contains("address already in use") {
        "El puerto ya está en uso en este equipo.".into()
    } else if m.contains("timed out") || m.contains("timeout") {
        "El servidor no responde (tiempo agotado).".into()
    } else if m.contains("refused") {
        "Conexión rechazada: el servidor no escucha en ese puerto.".into()
    } else if m.contains("unable to resolve") || m.contains("not known") || m.contains("no address associated") {
        "No se puede resolver el nombre del servidor.".into()
    } else {
        msg.to_string()
    }
}

pub type Handle = Arc<Mutex<Option<Child>>>;

pub fn available() -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("iperf3").is_file()))
}

/// Un intervalo de iperf3: `[  5]   0.00-1.00   sec  11.2 MBytes  94.1 Mbits/sec ...`.
/// Devuelve (bytes/s, es_resumen_final).
pub fn parse_line(l: &str) -> Option<(f64, bool)> {
    if !l.starts_with('[') || l.contains("[ ID]") {
        return None;
    }
    let toks: Vec<&str> = l.split_whitespace().collect();
    let i = toks.iter().position(|t| t.ends_with("bits/sec"))?;
    let v: f64 = toks.get(i.checked_sub(1)?)?.parse().ok()?;
    let mult = match toks[i].chars().next()? {
        'K' => 1e3,
        'M' => 1e6,
        'G' => 1e9,
        _ => 1.0,
    };
    let fin = l.contains("sender") || l.contains("receiver");
    Some((v * mult / 8.0, fin))
}

/// Lanza iperf3 en un hilo y vuelca su salida en `run`. El proceso queda en `handle` para poder pararlo.
pub fn start(p: Params, run: Arc<Mutex<Run>>, handle: Handle) {
    {
        let mut r = run.lock().unwrap();
        *r = Run { running: true, ..Default::default() };
    }
    std::thread::spawn(move || {
        let spawned = Command::new("iperf3").args(args(&p)).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => {
                let mut r = run.lock().unwrap();
                r.running = false;
                r.error = Some(if e.kind() == std::io::ErrorKind::NotFound { "iperf3 no está instalado (sudo apt install iperf3)".into() } else { e.to_string() });
                return;
            }
        };
        let out = child.stdout.take();
        let err = child.stderr.take();
        *handle.lock().unwrap() = Some(child);

        let push = |run: &Arc<Mutex<Run>>, line: String, is_err: bool| {
            let mut r = run.lock().unwrap();
            if is_err {
                r.error = Some(line.trim().to_string());
            }
            if let Some((bps, false)) = parse_line(&line) {
                r.hist.push(bps as u64);
            }
            r.lines.push(line);
            if r.lines.len() > 200 {
                r.lines.remove(0);
            }
        };
        let run2 = run.clone();
        let t = err.map(|e| std::thread::spawn(move || BufReader::new(e).lines().map_while(Result::ok).filter(|l| !l.trim().is_empty()).for_each(|l| push(&run2, l, true))));
        if let Some(o) = out {
            for l in BufReader::new(o).lines().map_while(Result::ok) {
                if !l.trim().is_empty() {
                    push(&run, l, false);
                }
            }
        }
        if let Some(t) = t {
            let _ = t.join();
        }
        if let Some(mut c) = handle.lock().unwrap().take() {
            let _ = c.wait();
        }
        let mut r = run.lock().unwrap();
        r.running = false;
        r.finished = true;
        r.retryable = r.error.as_deref().is_some_and(is_busy);
    });
}

pub fn stop(handle: &Handle) {
    if let Some(c) = handle.lock().unwrap().as_mut() {
        let _ = c.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpreta_intervalos_y_resumen() {
        let (b, fin) = parse_line("[  5]   0.00-1.00   sec  11.2 MBytes  94.1 Mbits/sec    0    390 KBytes").unwrap();
        assert!((b - 94.1e6 / 8.0).abs() < 1.0 && !fin);
        let (b, fin) = parse_line("[  5]   0.00-10.00  sec  1.10 GBytes   945 Mbits/sec   12             sender").unwrap();
        assert!((b - 945e6 / 8.0).abs() < 1.0 && fin);
        let (b, _) = parse_line("[  5]   0.00-1.00   sec   128 KBytes  1.05 Gbits/sec").unwrap();
        assert!((b - 1.05e9 / 8.0).abs() < 1.0);
        assert!(parse_line("[ ID] Interval           Transfer     Bitrate").is_none());
        assert!(parse_line("Connecting to host iperf.he.net, port 5201").is_none());
    }

    #[test]
    fn detecta_ocupado() {
        assert!(is_busy("iperf3: error - the server is busy running a test. try again later"));
        assert!(is_busy("iperf3: error - unable to start listener for connections: Address already in use"));
        assert!(!is_busy("iperf3: error - unable to connect to server: Connection refused"));
        assert!(explain("iperf3: error - unable to connect to server - server may have stopped running or use a different port: Connection timed out").contains("tiempo agotado"));
    }

    #[test]
    fn argumentos_de_cliente_y_servidor() {
        let p = Params { mode: Mode::Client, port: 5200, host: "x.example".into(), reverse: true, secs: 10 };
        let a = args(&p).join(" ");
        assert!(a.contains("-c x.example") && a.contains("-p 5200") && a.ends_with("-R"), "{a}");
        let s = args(&Params { mode: Mode::Server, ..p });
        assert!(s.contains(&"-s".to_string()) && !s.contains(&"-c".to_string()));
    }

    /// Necesita `iperf3` instalado: arranca un servidor local y mide contra él. `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn cliente_contra_servidor_local() {
        let wait = |r: &Arc<Mutex<Run>>| {
            for _ in 0..200 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if !r.lock().unwrap().running {
                    return;
                }
            }
        };
        let (srv, srv_h) = (Arc::new(Mutex::new(Run::default())), Handle::default());
        start(Params { mode: Mode::Server, port: 52011, host: String::new(), reverse: false, secs: 2 }, srv.clone(), srv_h.clone());
        std::thread::sleep(std::time::Duration::from_millis(500));
        let (cli, cli_h) = (Arc::new(Mutex::new(Run::default())), Handle::default());
        start(Params { mode: Mode::Client, port: 52011, host: "127.0.0.1".into(), reverse: false, secs: 2 }, cli.clone(), cli_h);
        wait(&cli);
        stop(&srv_h);
        wait(&srv);
        let r = cli.lock().unwrap();
        assert!(r.finished && r.error.is_none(), "{:?} {:?}", r.error, r.lines);
        assert!(!r.hist.is_empty() && r.hist.iter().any(|b| *b > 0), "{:?}", r.lines);
    }
}
