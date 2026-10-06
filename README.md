# lanwatch

Monitor de red en terminal, nativo y de consumo mínimo, pensado para una Orange Pi / Raspberry Pi
(Linux ARM64). Un solo binario estático de ~3 MB, sin dependencias ni demonios.

En una Orange Pi Zero 3: **~3,6 MB de RAM** y **~1 % de un núcleo** de media con la interfaz abierta
(incluye un barrido completo de la red cada minuto).

## Pestañas

| # | Pestaña | Qué muestra |
|---|---|---|
| 1 | **Dispositivos** | Todos los equipos de la LAN: IP, MAC, **fabricante** (decodificado de la MAC con la base de la IEEE embebida), nombre, **sistema operativo** (identificado preguntando por mDNS, NetBIOS y UPnP, y estimado por puertos/TTL), **puertos abiertos** y **tráfico entrante/saliente** por dispositivo. `Enter` abre el detalle. |
| 2 | **Puerta de enlace** | Tráfico total entrante/saliente del router (tasa, pico, total y gráfica), por SNMP; sin SNMP, el de la interfaz de esta máquina. |
| 3 | **DNS** | Compara latencia y respuestas del DNS del sistema y de las IP que añadas (`i`). 12 dominios, primera consulta y repetición (caché). |
| 4 | **Velocidad** | Latencia, jitter, descarga y subida contra servidores públicos (LibreSpeed + Cloudflare). Lista para elegir a mano o **modo automático** (`a`): el de menor latencia. |

| 5 | **iPerf3** | Lista de servidores públicos de iperf3 para lanzar una prueba (`Enter`), o modo **servidor** (por defecto, puerto 5201) para que otro equipo mida contra este. Se puede elegir modo, puerto, destino y sentido; si el servidor o el puerto están ocupados, ofrece esperar 1 minuto y reintentar. Necesita el programa `iperf3`. |
| 6 | **WiFi** | Redes visibles con la calidad de su señal (barra, dBm, canal, seguridad) y, arriba, los datos de la conexión actual. `Enter` abre el **detalle** de la red (fabricante del AP, canal, ancho, cifrado, congestión). Sin conexión no hay recuadro; sin WiFi, «WiFi no disponible». Usa `nmcli` o `iw`. |
| 7 | **Ayuda** | Guía de la aplicación y, paso a paso, cómo activar **NetFlow** y SNMP en el router (con la IP real de esta máquina y el estado actual). El texto sale de `docs/AYUDA.md` y `docs/EDGEROUTER.md`. |
| 8 | **About** | Datos del proyecto y del autor (de `Cargo.toml`) y estado de esta instalación: ejecutable, configuración, permisos, SNMP, NetFlow. |

**Programas opcionales** (el `.deb` los recomienda): `iperf3` para la pestaña iPerf3, y `nmcli` (NetworkManager) o `iw`
para la pestaña WiFi. Sin ellos, esas pestañas lo indican y el resto funciona igual.

Teclas globales: `Tab` / `Shift+Tab` / `1`‑`8` cambian de pestaña, `q` sale. Cada pestaña muestra las suyas abajo.

## Instalación

**Paquete Debian/Ubuntu/Raspberry Pi OS (recomendado)** — hay uno para `arm64` y otro para `amd64`:

```bash
sudo apt install ./lanwatch_0.4.0_arm64.deb     # instala el binario, la página de manual y la documentación
lanwatch
```

El paquete da `CAP_NET_RAW` al binario (con `setcap`, recomendado por `libcap2-bin`) en cada instalación y
**actualización**, así que no hay que repetirlo. Se desinstala con `sudo apt remove lanwatch`; tu configuración
(`~/.config/lanwatch.conf`) no se toca.

Para generar los paquetes (necesita Docker): `./scripts/build-deb.sh [arm64|amd64|all] [--lint]` → `dist/lanwatch_<versión>_<arch>.deb`.

**Binario suelto:** `./scripts/build-aarch64.sh` y copiar `dist/lanwatch-aarch64` donde quieras; para el TTL, tecla `c` o `--setcap`.

## Uso

```bash
./lanwatch                                  # interfaz completa
./lanwatch --once                           # sin interfaz: escanea, imprime la tabla y sale (scripts/cron)
./lanwatch --snmp public/eth0               # contadores del router por SNMP (host = puerta de enlace)
./lanwatch --netflow 2055 --iface end0      # colector NetFlow y selección de interfaz
./lanwatch --help
```

Las opciones también pueden ir en `~/.config/lanwatch.conf` o `/etc/lanwatch.conf` (`clave=valor`, sin `--`).

### Qué necesita cada función

| Función | Necesita |
|---|---|
| Dispositivos, fabricante, puertos | Nada: sin root. Barrido UDP + tabla ARP del kernel + escaneo TCP *connect* + sondas mDNS/NetBIOS/SSDP. Con `CAP_NET_RAW`, **ARP propio** (presencia exacta). |
| TTL → mejor detección de SO | `CAP_NET_RAW` en el binario: tecla **`c`** en la pestaña Dispositivos (pide confirmación, luego tu contraseña de `sudo`, y la app se reinicia sola) o `./lanwatch --setcap`. Si vuelves a copiar el binario hay que repetirlo. Sin él se estima por puertos, fabricante y nombre. |
| Tráfico por dispositivo | Que el router exporte **NetFlow (v5, v9 o IPFIX)** a esta máquina. Ver [docs/EDGEROUTER.md](docs/EDGEROUTER.md). |
| Tráfico de la puerta de enlace | SNMP v2c activado en el router. Se configura desde la pestaña (tecla `s`) o con `--snmp`. |
| DNS, velocidad | Salida a Internet. |

Sin la configuración del router, el resto funciona y las columnas de tráfico por equipo muestran `n/d`.

## Compilar

```bash
cargo run                         # en cualquier Linux/macOS (el descubrimiento usa /proc: solo Linux)
cargo test                        # pruebas unitarias
cargo test -- --ignored --nocapture   # pruebas con red (DNS, lista y latencia de servidores)
./scripts/build-aarch64.sh        # binario estático para Orange Pi / Raspberry Pi 64 bit, vía Docker
./scripts/build-deb.sh            # paquetes .deb (arm64 y amd64)
scp dist/lanwatch-aarch64 usuario@pi:~/lanwatch
```

## Límites conocidos

- **Presencia de dispositivos:** con `CAP_NET_RAW` es exacta (ARP propio). Sin él se apoya en la tabla ARP del kernel, que puede conservar unos minutos un equipo ya apagado. En ambos casos un equipo se da por apagado tras 2 barridos seguidos sin respuesta.
- **Varias IP con la misma MAC** (proxy ARP, contenedores en macvlan) aparecen como dispositivos distintos con la misma MAC.
- **SO:** es una identificación a partir de lo que el equipo cuenta (mDNS, NetBIOS, UPnP) y de heurísticas; no una huella exacta. Las MAC aleatorias de móviles modernos no tienen fabricante.
- **NetFlow:** se exporta al caducar el flujo, así que las tasas son medias de 60 s, no instantáneas. Solo se cuenta IPv4.
- **Tests de velocidad:** los servidores públicos limitan a quien los usa mucho (devuelven 403/429). Si pasa, la app lo indica y basta con elegir otro servidor o esperar.

## Estructura y ampliación

Ver [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): cómo añadir una pestaña, una fuente de datos o un parser de NetFlow.
