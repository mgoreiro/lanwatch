//! Interfaz de red activa, su subred y la puerta de enlace (sin ejecutar ningún comando externo).

use std::ffi::CStr;
use std::net::Ipv4Addr;

#[derive(Clone, Debug)]
pub struct IfaceInfo {
    pub name: String,
    pub ip: Ipv4Addr,
    pub prefix: u8,
    pub gateway: Option<Ipv4Addr>,
    pub mac: [u8; 6],
}

impl IfaceInfo {
    pub fn contains(&self, ip: Ipv4Addr) -> bool {
        let mask = if self.prefix == 0 { 0 } else { u32::MAX << (32 - self.prefix as u32) };
        u32::from(ip) & mask == u32::from(self.ip) & mask
    }
}

/// Ruta por defecto: (interfaz, puerta de enlace) con la métrica más baja.
fn default_route(pref: Option<&str>) -> Option<(String, Ipv4Addr)> {
    let text = std::fs::read_to_string("/proc/net/route").ok()?;
    let mut best: Option<(u32, String, Ipv4Addr)> = None;
    for l in text.lines().skip(1) {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() < 8 || f[1] != "00000000" {
            continue;
        }
        if pref.is_some_and(|p| p != f[0]) {
            continue;
        }
        let gw = Ipv4Addr::from(u32::from_str_radix(f[2], 16).ok()?.swap_bytes());
        let metric: u32 = f[6].parse().unwrap_or(0);
        if best.as_ref().is_none_or(|b| metric < b.0) {
            best = Some((metric, f[0].to_string(), gw));
        }
    }
    best.map(|(_, i, g)| (i, g))
}

fn addr_of(name: &str) -> Option<(Ipv4Addr, u8)> {
    unsafe {
        let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifap) != 0 {
            return None;
        }
        let mut found = None;
        let mut cur = ifap;
        while !cur.is_null() {
            let a = &*cur;
            if !a.ifa_addr.is_null()
                && !a.ifa_netmask.is_null()
                && (*a.ifa_addr).sa_family as i32 == libc::AF_INET
                && CStr::from_ptr(a.ifa_name).to_str() == Ok(name)
            {
                let ip = u32::from_be((*(a.ifa_addr as *const libc::sockaddr_in)).sin_addr.s_addr);
                let mask = u32::from_be((*(a.ifa_netmask as *const libc::sockaddr_in)).sin_addr.s_addr);
                found = Some((Ipv4Addr::from(ip), mask.count_ones() as u8));
                break;
            }
            cur = a.ifa_next;
        }
        libc::freeifaddrs(ifap);
        found
    }
}

fn mac_of(name: &str) -> [u8; 6] {
    let mut mac = [0u8; 6];
    if let Ok(t) = std::fs::read_to_string(format!("/sys/class/net/{name}/address")) {
        for (i, p) in t.trim().split(':').enumerate().take(6) {
            mac[i] = u8::from_str_radix(p, 16).unwrap_or(0);
        }
    }
    mac
}

pub fn detect(pref: Option<&str>) -> Result<IfaceInfo, String> {
    let (name, gw) = match default_route(pref) {
        Some((n, g)) => (n, Some(g)),
        None => match pref {
            Some(p) => (p.to_string(), None),
            None => return Err("no hay ruta por defecto; indica la interfaz con --iface".into()),
        },
    };
    let (ip, prefix) = addr_of(&name).ok_or(format!("la interfaz {name} no tiene dirección IPv4"))?;
    Ok(IfaceInfo { mac: mac_of(&name), name, ip, prefix, gateway: gw })
}
