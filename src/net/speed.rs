//! Test de velocidad contra servidores públicos HTTP(S): lista de LibreSpeed (comunidad) y
//! Cloudflare. Un «servidor» solo son tres URL (ping, descarga, subida); para añadir otro
//! proveedor basta con que `list()` devuelva más `Server`.

use serde::Deserialize;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LIST_URL: &str = "https://librespeed.org/backend-servers/servers.php";
const TEST_SECS: u64 = 8;
const WARMUP: Duration = Duration::from_millis(1200);

#[derive(Clone, Debug)]
pub struct Server {
    pub name: String,
    pub ping_url: String,
    pub dl_url: String, // sin parámetros de tamaño: se añaden al pedirla
    pub ul_url: String,
    pub cloudflare: bool,
    pub latency_ms: Option<f64>,
}

#[derive(Deserialize)]
struct LsEntry {
    name: String,
    server: String,
    #[serde(rename = "dlURL")]
    dl: String,
    #[serde(rename = "ulURL")]
    ul: String,
    #[serde(rename = "pingURL")]
    ping: String,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(8))
        .timeout_write(Duration::from_secs(8))
        .user_agent("lanwatch/0.1")
        .build()
}

fn join(base: &str, path: &str) -> String {
    let base = if base.starts_with("//") { format!("https:{base}") } else { base.to_string() };
    format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/'))
}

pub fn cloudflare() -> Server {
    Server {
        name: "Cloudflare (anycast, el más cercano)".into(),
        ping_url: "https://speed.cloudflare.com/__down?bytes=0".into(),
        dl_url: "https://speed.cloudflare.com/__down".into(),
        ul_url: "https://speed.cloudflare.com/__up".into(),
        cloudflare: true,
        latency_ms: None,
    }
}

pub fn list() -> Result<Vec<Server>, String> {
    let body = agent().get(LIST_URL).call().map_err(|e| format!("no se pudo obtener la lista de servidores: {e}"))?;
    let entries: Vec<LsEntry> = body.into_json().map_err(|e| format!("lista de servidores inválida: {e}"))?;
    let mut v = vec![cloudflare()];
    v.extend(entries.into_iter().map(|e| Server {
        ping_url: join(&e.server, &e.ping),
        dl_url: join(&e.server, &e.dl),
        ul_url: join(&e.server, &e.ul),
        name: e.name,
        cloudflare: false,
        latency_ms: None,
    }));
    Ok(v)
}

fn nonce() -> u64 {
    Instant::now().elapsed().as_nanos() as u64 ^ (std::process::id() as u64) << 20
}

/// Latencia mínima de varias peticiones pequeñas reutilizando la conexión (RTT, sin el TLS inicial).
pub fn latency(s: &Server, samples: usize) -> Option<f64> {
    let a = agent();
    let url = |n: u64| if s.cloudflare { s.ping_url.clone() } else { format!("{}?r={n}", s.ping_url) };
    a.get(&url(0)).call().ok()?; // calienta TCP/TLS
    (1..=samples as u64)
        .filter_map(|i| {
            let t = Instant::now();
            a.get(&url(i + nonce() % 1000)).call().ok()?.into_reader().read_to_end(&mut Vec::new()).ok()?;
            Some(t.elapsed().as_secs_f64() * 1000.0)
        })
        .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x))))
}

/// Mide la latencia de todos en paralelo y la guarda en cada servidor.
pub fn measure_all(servers: &mut [Server]) {
    let results: Vec<Option<f64>> = std::thread::scope(|sc| {
        let hs: Vec<_> = servers.iter().map(|s| sc.spawn(move || latency(s, 3))).collect();
        hs.into_iter().map(|h| h.join().ok().flatten()).collect()
    });
    for (s, r) in servers.iter_mut().zip(results) {
        s.latency_ms = r;
    }
}

pub fn pick_auto(servers: &[Server]) -> Option<usize> {
    servers.iter().enumerate().filter_map(|(i, s)| s.latency_ms.map(|l| (i, l))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|x| x.0)
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Phase {
    #[default]
    Idle,
    Latency,
    Download,
    Upload,
    Done,
    Failed,
}

#[derive(Default, Clone)]
pub struct Progress {
    pub phase: Phase,
    pub server: String,
    pub latency_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub cur_bps: f64,
    pub dl_bps: Option<f64>,
    pub ul_bps: Option<f64>,
    pub hist: Vec<u64>, // muestras de la fase en curso (bytes/s)
    pub error: Option<String>,
}

fn set(p: &Arc<Mutex<Progress>>, f: impl FnOnce(&mut Progress)) {
    f(&mut p.lock().unwrap_or_else(|e| e.into_inner()));
}

/// Una fase de transferencia con `threads` hilos durante `TEST_SECS`; devuelve bytes/s medios
/// descontando el arranque lento.
fn transfer(s: &Server, upload: bool, threads: usize, p: &Arc<Mutex<Progress>>, cancel: &Arc<AtomicBool>) -> (f64, Option<String>) {
    let bytes = Arc::new(AtomicU64::new(0));
    let zeros = Arc::new(if upload { vec![0u8; 1 << 20] } else { Vec::new() });
    let first_err: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let end = Instant::now() + Duration::from_secs(TEST_SECS);
    let t0 = Instant::now();
    std::thread::scope(|sc| {
        for _ in 0..threads {
            let (bytes, a, s, zeros, first_err) = (bytes.clone(), agent(), s.clone(), zeros.clone(), first_err.clone());
            let cancel = cancel.clone();
            sc.spawn(move || {
                while Instant::now() < end && !cancel.load(Ordering::Relaxed) {
                    let res = if upload {
                        // Bloques en memoria: `send_bytes` rinde ~3× más que enviar con un lector en streaming.
                        a.post(&s.ul_url)
                            .set("Content-Type", "application/octet-stream")
                            .send_bytes(&zeros)
                            .map(|_| {
                                bytes.fetch_add(zeros.len() as u64, Ordering::Relaxed);
                            })
                    } else {
                        let url = if s.cloudflare {
                            format!("{}?bytes=25000000", s.dl_url)
                        } else {
                            format!("{}?ckSize=20&r={}", s.dl_url, nonce())
                        };
                        a.get(&url).call().map(|r| {
                            let mut rd = r.into_reader();
                            let mut buf = [0u8; 65536];
                            while Instant::now() < end && !cancel.load(Ordering::Relaxed) {
                                match rd.read(&mut buf) {
                                    Ok(0) | Err(_) => break,
                                    Ok(n) => {
                                        bytes.fetch_add(n as u64, Ordering::Relaxed);
                                    }
                                }
                            }
                        })
                    };
                    if let Err(e) = res {
                        first_err.lock().unwrap().get_or_insert_with(|| e.to_string());
                        std::thread::sleep(Duration::from_millis(200));
                    }
                }
            });
        }
        // muestreo en el hilo actual
        let (mut last_b, mut last_t, mut warm_b, mut warmed) = (0u64, Instant::now(), 0u64, false);
        while Instant::now() < end && !cancel.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(250));
            let (b, now) = (bytes.load(Ordering::Relaxed), Instant::now());
            let bps = (b - last_b) as f64 / now.duration_since(last_t).as_secs_f64();
            (last_b, last_t) = (b, now);
            if !warmed && t0.elapsed() >= WARMUP {
                (warmed, warm_b) = (true, b);
            }
            set(p, |pr| {
                pr.cur_bps = bps;
                pr.hist.push(bps as u64);
            });
        }
        let total = bytes.load(Ordering::Relaxed);
        let secs = (t0.elapsed().saturating_sub(WARMUP)).as_secs_f64();
        let bps = if warmed && secs > 0.5 { (total - warm_b) as f64 / secs } else { total as f64 / t0.elapsed().as_secs_f64().max(0.1) };
        let err = if total == 0 { first_err.lock().unwrap().clone() } else { None };
        (bps, err)
    })
}

pub fn run_test(s: &Server, p: Arc<Mutex<Progress>>, cancel: Arc<AtomicBool>) {
    set(&p, |pr| *pr = Progress { phase: Phase::Latency, server: s.name.clone(), ..Default::default() });
    // latencia y jitter
    let a = agent();
    let url = |i: u64| if s.cloudflare { s.ping_url.clone() } else { format!("{}?r={}", s.ping_url, nonce() + i) };
    if a.get(&url(0)).call().is_err() {
        return set(&p, |pr| {
            pr.phase = Phase::Failed;
            pr.error = Some("el servidor no responde".into());
        });
    }
    let mut ms = Vec::new();
    for i in 1..=8 {
        let t = Instant::now();
        if a.get(&url(i)).call().and_then(|r| r.into_reader().read_to_end(&mut Vec::new()).map_err(Into::into)).is_ok() {
            ms.push(t.elapsed().as_secs_f64() * 1000.0);
        }
    }
    let min = ms.iter().cloned().fold(f64::MAX, f64::min);
    let jitter = if ms.len() > 1 { ms.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (ms.len() - 1) as f64 } else { 0.0 };
    set(&p, |pr| {
        pr.latency_ms = Some(min);
        pr.jitter_ms = Some(jitter);
        pr.phase = Phase::Download;
        pr.hist.clear();
    });
    let (dl, dl_err) = transfer(s, false, 4, &p, &cancel);
    if cancel.load(Ordering::Relaxed) {
        return set(&p, |pr| pr.phase = Phase::Idle);
    }
    set(&p, |pr| {
        match dl_err {
            Some(e) => pr.error = Some(format!("Descarga rechazada por el servidor ({e}). Prueba otro servidor.")),
            None => pr.dl_bps = Some(dl),
        }
        pr.phase = Phase::Upload;
        pr.hist.clear();
        pr.cur_bps = 0.0;
    });
    let (ul, ul_err) = transfer(s, true, 4, &p, &cancel);
    set(&p, |pr| {
        if cancel.load(Ordering::Relaxed) {
            pr.phase = Phase::Idle;
            return;
        }
        match ul_err {
            // Los servidores públicos limitan a quien los usa mucho: se avisa en vez de mostrar 0.
            Some(e) => pr.error = Some(format!("Subida rechazada por el servidor ({e}). Suele ser un límite temporal; prueba otro servidor.")),
            None => pr.ul_bps = Some(ul),
        }
        pr.phase = Phase::Done;
        pr.cur_bps = 0.0;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_urls() {
        assert_eq!(join("https://a.b/backend", "garbage.php"), "https://a.b/backend/garbage.php");
        assert_eq!(join("//a.b/", "/x.php"), "https://a.b/x.php");
    }

    /// Con red: `cargo test -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn lista_y_latencia_reales() {
        let mut v = list().unwrap();
        assert!(v.len() > 5);
        measure_all(&mut v);
        let i = pick_auto(&v).expect("algún servidor debe responder");
        println!("auto → {} ({:.0} ms)", v[i].name, v[i].latency_ms.unwrap());
        for s in v.iter().filter(|s| s.latency_ms.is_some()).take(5) {
            println!("  {:>6.0} ms  {}", s.latency_ms.unwrap(), s.name);
        }
    }

    /// Con red (tarda ~20 s): `cargo test -- --ignored --nocapture transferencia`
    #[test]
    #[ignore]
    fn transferencia_real() {
        let s = list().unwrap().into_iter().find(|s| s.name.contains("Sharktech")).unwrap();
        let p = Arc::new(Mutex::new(Progress::default()));
        run_test(&s, p.clone(), Arc::new(AtomicBool::new(false)));
        let r = p.lock().unwrap().clone();
        println!("{:?} lat={:?} dl={:?} ul={:?} err={:?}", r.phase, r.latency_ms, r.dl_bps.map(|b| b * 8e-6), r.ul_bps.map(|b| b * 8e-6), r.error);
        assert_eq!(r.phase, Phase::Done);
        assert!(r.dl_bps.unwrap() > 0.0);
    }
}
