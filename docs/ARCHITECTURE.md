# Arquitectura

Tres capas, sin dependencias hacia arriba:

```
src/net/    Fuentes de datos puras: reciben parámetros y devuelven datos. No conocen la interfaz.
src/core/   Estado compartido (state.rs) + hilos de fondo que lo mantienen (workers.rs).
src/tabs/   Interfaz: una pestaña por fichero, todas detrás del trait `Tab`.
src/app.rs  Bucle de eventos y dibujo. src/config.rs: opciones. src/once.rs: modo sin interfaz.
```

Flujo: los hilos de `workers.rs` (o las propias pestañas, en DNS, velocidad, iPerf3 y WiFi, que son bajo demanda)
llaman a `net/*`, escriben en `State` (un `Mutex`) y las pestañas lo leen al dibujar. En reposo todo
duerme: la interfaz solo se redibuja con una tecla o con su reloj (1 s, o 200 ms si la pestaña dice
que está `busy()`).

## Añadir una pestaña

1. Crea `src/tabs/mi_pestana.rs` con un tipo que implemente `Tab` (`title`, `help`, `on_key`, `draw`;
   opcionales `on_tick`, `busy`, `capturing_input`).
2. Declara el módulo en `src/tabs/mod.rs` y añádela a `all()`. Ya está: el menú, los atajos
   numéricos y el pie de ayuda se generan solos.

Si la pestaña necesita datos continuos, añade los campos a `core/state.rs` y un hilo en
`core/workers.rs`; si es una acción puntual (como DNS o velocidad), lánzala en un hilo desde la
propia pestaña y guarda el resultado en un `Arc<Mutex<…>>`.

## Añadir una fuente de datos

Crea `src/net/mi_fuente.rs` (expón una función que devuelva datos, sin tocar el estado ni la UI),
regístrala en `net/mod.rs` y úsala desde un worker o una pestaña. Puntos de extensión ya pensados:

| Quiero… | Dónde |
|---|---|
| Descubrir de otra forma (p. ej. NDP para IPv6) | otra función junto a `discovery::sweep` → `Vec<(Ipv4Addr, [u8;6])>`; hoy: `arp.rs` (ARP propio, con CAP_NET_RAW) y la tabla ARP del kernel |
| Otra sonda para identificar equipos (HTTP, SNMP, SSH banner…) | una función `fn(ip) -> …` y una línea en `fingerprint::probe`; añade el campo a `Signals` y reglas en `osdetect::guess` |
| Mejor detección de SO | reglas en `osdetect::guess` (recibe todas las señales: TTL, fabricante, puertos, nombre, modelo, servicios mDNS, NetBIOS, SSDP) |
| Otro formato de flujos (sFlow…) | un parser que produzca `netflow::Record`, como `parse_v5` / `parse_template_based` |
| Otra fuente de tráfico por dispositivo (SSH al router, conntrack) | un hilo que rellene `Device.flow`, como `netflow::run` |
| Otro proveedor de test de velocidad | que `speed::list()` devuelva más `Server` (ping, descarga, subida) |
| Otra fuente de redes WiFi | rama nueva en `wifi::scan` y un analizador de su salida (`parse_nmcli`, `parse_iw_scan`) |
| Más servidores públicos de iperf3 | una línea en `iperf::SERVERS` |
| Otro origen de contadores del router | rama nueva en `workers::gateway_loop` |

## Ayuda en la app

`tabs/help.rs` muestra `docs/AYUDA.md` y `docs/EDGEROUTER.md` (embebidos con `include_str!`). Para cambiar la ayuda,
edita esos ficheros; admiten `# títulos`, bloques de código y `> avisos`. En la página del router, la IP de
ejemplo (192.168.1.232) se sustituye por la de la máquina.

## Opciones

Se añaden en `Config::apply` (`config.rs`); sirven igual por argumento y por fichero.

## Datos embebidos

`data/oui.bin` (fabricantes por MAC) se genera con `tools/gen_oui.py` a partir de los CSV públicos de la IEEE
(MA-L, MA-M y MA-S) y se incluye en el binario con `include_bytes!`; se consulta por búsqueda binaria
sin cargarlo en memoria.

## Empaquetado

`scripts/build-deb.sh` compila el binario estático (musl, con Docker) y monta el `.deb` con `dpkg-deb`; los
ficheros del paquete viven en `packaging/` (`postinst`, `copyright`, `changelog`, página de manual, ejemplo de
configuración y excepciones de lintian). El `postinst` aplica `CAP_NET_RAW`. No se instala `/etc/lanwatch.conf`
a propósito: la aplicación prefiere ese fichero si existe y no podría guardar en él los cambios hechos desde la interfaz.
Al subir de versión: `Cargo.toml` y una entrada nueva en `packaging/changelog`.
