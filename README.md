# lanwatch

Monitor de red en terminal, nativo y de consumo mínimo, pensado para una Orange Pi / Raspberry Pi
(Linux ARM64). Un solo binario estático de ~3 MB, sin dependencias ni demonios.

En una Orange Pi Zero 3: **~3,6 MB de RAM** y **~1 % de un núcleo** de media con la interfaz abierta
(incluye un barrido completo de la red cada minuto).

## Pestañas

| # | Pestaña | Qué muestra |
|---|---|---|
| 1 | **Dispositivos** | Todos los equipos de la LAN: IP, MAC, **fabricante** (decodificado de la MAC con la base de la IEEE embebida), nombre, **sistema operativo** estimado, **puertos abiertos** y **tráfico entrante/saliente** por dispositivo. `Enter` abre el detalle. |
| 2 | **Puerta de enlace** | Tráfico total entrante/saliente del router (tasa, pico, total y gráfica), por SNMP; sin SNMP, el de la interfaz de esta máquina. |
| 3 | **DNS** | Compara latencia y respuestas del DNS del sistema y de las IP que añadas (`i`). 12 dominios, primera consulta y repetición (caché). |
| 4 | **Velocidad** | Latencia, jitter, descarga y subida contra servidores públicos (LibreSpeed + Cloudflare). Lista para elegir a mano o **modo automático** (`a`): el de menor latencia. |

| 5 | **Ayuda** | Guía de la aplicación y, paso a paso, cómo activar **NetFlow** y SNMP en el router (con la IP real de esta máquina y el estado actual). El texto sale de `docs/AYUDA.md` y `docs/EDGEROUTER.md`. |

Teclas globales: `Tab` / `Shift+Tab` / `1`‑`5` cambian de pestaña, `q` sale. Cada pestaña muestra las suyas abajo.

## Instalación

**Paquete Debian/Ubuntu/Raspberry Pi OS (recomendado)** — hay uno para `arm64` y otro para `amd64`:

```bash
sudo apt install ./lanwatch_0.2.0_arm64.deb     # instala el binario, la página de manual y la documentación
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
| Dispositivos, fabricante, puertos | Nada: sin root. Barrido UDP + tabla ARP del kernel + escaneo TCP *connect*. |
| TTL → mejor detección de SO | `CAP_NET_RAW` en el binario: tecla **`c`** en la pestaña Dispositivos (pide confirmación, luego tu contraseña de `sudo`, y la app se reinicia sola) o `./lanwatch --setcap`. Si vuelves a copiar el binario hay que repetirlo. Sin él se estima por puertos, fabricante y nombre. |
| Tráfico por dispositivo | Que el router exporte **NetFlow v5** a esta máquina. Ver [docs/EDGEROUTER.md](docs/EDGEROUTER.md). |
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

- **Presencia de dispositivos:** se apoya en la tabla ARP del kernel, que puede conservar unos minutos un equipo ya apagado. Un ARP propio con `CAP_NET_RAW` daría presencia exacta (hueco previsto en `net/discovery.rs`).
- **Varias IP con la misma MAC** (proxy ARP, contenedores en macvlan) aparecen como dispositivos distintos con la misma MAC.
- **SO:** es una estimación heurística, no una huella exacta. Las MAC aleatorias de móviles modernos no tienen fabricante.
- **NetFlow** se exporta cuando el flujo caduca, así que las tasas son medias de 60 s, no instantáneas. v9/IPFIX no están implementados.
- **Tests de velocidad:** los servidores públicos limitan a quien los usa mucho (devuelven 403/429). Si pasa, la app lo indica y basta con elegir otro servidor o esperar.

## Estructura y ampliación

Ver [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): cómo añadir una pestaña, una fuente de datos o un parser de NetFlow.
