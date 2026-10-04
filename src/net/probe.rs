//! Sondeos por dispositivo: escaneo TCP de puertos, TTL por ICMP (opcional) y nombre inverso.

use std::ffi::CStr;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Puertos que se comprueban: servicios habituales de PC, NAS, impresoras, cámaras, IoT y multimedia.
pub const PORTS: &[u16] = &[
    21, 22, 23, 25, 53, 80, 110, 111, 135, 139, 143, 161, 443, 445, 515, 548, 554, 631, 993, 995, 1723, 1883,
    1900, 2049, 3000, 3306, 3389, 3689, 5000, 5001, 5009, 5060, 5432, 5555, 5900, 6379, 7000, 7659, 7660, 8000,
    8008, 8009, 8080, 8081, 8096, 8123, 8200, 8443, 8554, 8888, 9000, 9100, 9999, 10000, 32400, 49152, 62078,
];

/// Escaneo TCP «connect» con `threads` hilos. Solo cuenta como abierto un puerto que acepta la conexión.
pub fn scan_ports(ip: Ipv4Addr, timeout: Duration, threads: usize) -> Vec<u16> {
    let next = AtomicUsize::new(0);
    let open = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(&port) = PORTS.get(i) else { break };
                if TcpStream::connect_timeout(&SocketAddr::from((ip, port)), timeout).is_ok() {
                    open.lock().unwrap().push(port);
                }
            });
        }
    });
    let mut v = open.into_inner().unwrap();
    v.sort_unstable();
    v
}

/// ¿Podemos abrir un socket ICMP en bruto? (necesita `CAP_NET_RAW`: `setcap cap_net_raw+ep lanwatch`)
pub fn raw_icmp_available() -> bool {
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_RAW, libc::IPPROTO_ICMP);
        if fd < 0 {
            return false;
        }
        libc::close(fd);
        true
    }
}

fn checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    for c in data.chunks(2) {
        sum += u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]) as u32;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// TTL de la respuesta a un ping: 64 → Linux/Android/iOS/macOS, 128 → Windows, 255 → equipos de red.
pub fn icmp_ttl(ip: Ipv4Addr, timeout: Duration) -> Option<u8> {
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_RAW, libc::IPPROTO_ICMP);
        if fd < 0 {
            return None;
        }
        let tv = libc::timeval { tv_sec: 0, tv_usec: 200_000 };
        libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_RCVTIMEO, &tv as *const _ as *const _, std::mem::size_of::<libc::timeval>() as u32);
        let ident = (std::process::id() & 0xffff) as u16;
        let mut pkt = [8u8, 0, 0, 0, (ident >> 8) as u8, ident as u8, 0, 1, b'l', b'w', b'a', b't'];
        let ck = checksum(&pkt);
        pkt[2] = (ck >> 8) as u8;
        pkt[3] = ck as u8;
        let mut dst: libc::sockaddr_in = std::mem::zeroed();
        dst.sin_family = libc::AF_INET as _;
        dst.sin_addr.s_addr = u32::from(ip).to_be();
        let sent = libc::sendto(fd, pkt.as_ptr() as *const _, pkt.len(), 0, &dst as *const _ as *const _, std::mem::size_of::<libc::sockaddr_in>() as u32);
        let mut ttl = None;
        if sent > 0 {
            let start = Instant::now();
            let mut buf = [0u8; 256];
            while start.elapsed() < timeout {
                let n = libc::recv(fd, buf.as_mut_ptr() as *mut _, buf.len(), 0);
                if n < 28 {
                    continue;
                }
                let ihl = (buf[0] & 0x0f) as usize * 4;
                let from = Ipv4Addr::new(buf[12], buf[13], buf[14], buf[15]);
                // echo reply (0) a nuestra petición
                if from == ip && n as usize >= ihl + 8 && buf[ihl] == 0 && buf[ihl + 4] == (ident >> 8) as u8 && buf[ihl + 5] == ident as u8 {
                    ttl = Some(buf[8]);
                    break;
                }
            }
        }
        libc::close(fd);
        ttl
    }
}

/// Nombre inverso (DNS del sistema). Puede tardar si el DNS no responde: se llama desde un hilo propio.
pub fn reverse_name(ip: Ipv4Addr) -> Option<String> {
    unsafe {
        let mut sa: libc::sockaddr_in = std::mem::zeroed();
        sa.sin_family = libc::AF_INET as _;
        sa.sin_addr.s_addr = u32::from(ip).to_be();
        let mut host = [0 as libc::c_char; 256];
        let r = libc::getnameinfo(
            &sa as *const _ as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_in>() as u32,
            host.as_mut_ptr(),
            host.len() as u32,
            std::ptr::null_mut(),
            0,
            libc::NI_NAMEREQD,
        );
        if r != 0 {
            return None;
        }
        let name = CStr::from_ptr(host.as_ptr()).to_string_lossy().into_owned();
        let name = name.split('.').next().unwrap_or(&name).to_string();
        if name.is_empty() || name.parse::<Ipv4Addr>().is_ok() { None } else { Some(name) }
    }
}
