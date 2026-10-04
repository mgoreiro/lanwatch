//! Fabricante a partir de la MAC. La base (`data/oui.bin`, ver `tools/gen_oui.py`) va embebida
//! en el binario y se consulta por búsqueda binaria: no se carga nada en memoria.

static DB: &[u8] = include_bytes!("../../data/oui.bin");

fn u32_at(o: usize) -> usize {
    u32::from_le_bytes(DB[o..o + 4].try_into().unwrap()) as usize
}

fn lookup_in(table_start: usize, key: u64) -> Option<(usize, usize)> {
    // devuelve (offset de nombre en el pool, fin de la tabla)
    let n = u32_at(table_start);
    let base = table_start + 4;
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let o = base + mid * 12;
        let k = u64::from_le_bytes(DB[o..o + 8].try_into().unwrap());
        match k.cmp(&key) {
            std::cmp::Ordering::Equal => return Some((u32_at(o + 8), base + n * 12)),
            std::cmp::Ordering::Less => lo = mid + 1,
            std::cmp::Ordering::Greater => hi = mid,
        }
    }
    None
}

pub fn vendor(mac: &[u8; 6]) -> String {
    if DB.len() < 16 || &DB[..4] != b"OUI2" {
        return String::new();
    }
    // MAC administrada localmente (bit 1 del primer byte): suele ser aleatoria/privada.
    if mac[0] & 0x02 != 0 {
        return "(MAC privada)".into();
    }
    let v: u64 = mac.iter().fold(0u64, |a, b| (a << 8) | *b as u64);
    let t24 = 4usize;
    let n24 = u32_at(t24);
    let t28 = t24 + 4 + n24 * 12;
    let n28 = u32_at(t28);
    let t36 = t28 + 4 + n28 * 12;
    let n36 = u32_at(t36);
    let pool = t36 + 4 + n36 * 12;
    // de más específico a menos: 36 bit, 28 bit, 24 bit
    let found = lookup_in(t36, v >> 12).or_else(|| lookup_in(t28, v >> 20)).or_else(|| lookup_in(t24, v >> 24));
    match found {
        Some((off, _)) => {
            let s = &DB[pool + off..];
            let end = s.iter().position(|b| *b == 0).unwrap_or(s.len());
            String::from_utf8_lossy(&s[..end]).into_owned()
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::vendor;

    #[test]
    fn fabricante_por_mac() {
        assert_eq!(vendor(&[0x00, 0x03, 0x93, 1, 2, 3]), "Apple");
        assert_eq!(vendor(&[0xb8, 0x27, 0xeb, 1, 2, 3]), "Raspberry Pi Foundation");
    }

    #[test]
    fn mac_privada_y_desconocida() {
        assert_eq!(vendor(&[0x02, 0xeb, 0xd8, 0, 0, 1]), "(MAC privada)");
        assert!(vendor(&[0x00, 0x00, 0x00, 0, 0, 0]).to_lowercase().contains("xerox")); // 00:00:00 está registrado
        assert_eq!(vendor(&[0x00, 0x01, 0x02, 0xff, 0xff, 0xff]).is_empty(), false);
    }
}
