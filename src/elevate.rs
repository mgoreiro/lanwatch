//! Acciones que necesitan privilegios. La app **nunca** guarda ni maneja la contraseña: delega en
//! `sudo`, que la pide directamente en la terminal del usuario.
//!
//! Dar `CAP_NET_RAW` al binario permite medir el TTL por ICMP (mejor detección de SO). Los permisos
//! de fichero solo se aplican al arrancar, por eso después hay que relanzar el proceso (`reexec`).
//! Ojo: si se vuelve a copiar el binario (scp, actualización), el permiso se pierde y hay que repetirlo.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

fn find_setcap() -> Option<PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    let mut dirs: Vec<&str> = path.split(':').collect();
    dirs.extend(["/usr/sbin", "/sbin"]);
    dirs.into_iter().map(|d| PathBuf::from(d).join("setcap")).find(|p| p.is_file())
}

/// Órden exacta que se va a ejecutar (se muestra al usuario antes de pedir confirmación).
pub fn command_line() -> String {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "lanwatch".into());
    let sudo = if unsafe { libc::geteuid() } == 0 { "" } else { "sudo " };
    format!("{sudo}setcap cap_net_raw+ep {exe}")
}

pub fn grant_raw_cap() -> Result<(), String> {
    let setcap = find_setcap().ok_or("no se encontró «setcap»; instálalo con: sudo apt install libcap2-bin")?;
    let exe = std::env::current_exe().map_err(|e| format!("no se pudo localizar el ejecutable: {e}"))?;
    let mut cmd = if unsafe { libc::geteuid() } == 0 {
        Command::new(&setcap)
    } else {
        let mut c = Command::new("sudo");
        c.arg(&setcap);
        c
    };
    cmd.arg("cap_net_raw+ep").arg(&exe);
    let status = cmd.status().map_err(|e| format!("no se pudo ejecutar la orden: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("la orden terminó con error ({status}). ¿Contraseña incorrecta o sin permisos de sudo?"))
    }
}

/// Sustituye el proceso por una copia nueva de sí mismo (mismos argumentos) para que aplique los permisos.
pub fn reexec() -> String {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("lanwatch"));
    let err = Command::new(exe).args(std::env::args().skip(1)).exec();
    format!("no se pudo relanzar: {err}")
}
