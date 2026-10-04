# Qué es lanwatch

Un monitor de red para esta máquina: ve los equipos de tu red local, el tráfico del router, la salud
del DNS y la velocidad de la conexión. Consume muy poco (unos 4 MB de RAM).

# Teclas

```
Tab / Mayús+Tab   cambiar de pestaña        1-6   ir directo a una pestaña
q  salir          Ctrl+C  salir             ↑ ↓   mover / desplazar
```

# 1 · Dispositivos

Lista de todos los equipos de la subred, con su IP, MAC, fabricante (se deduce de los 3 primeros
bytes de la MAC), nombre, sistema operativo estimado, puertos abiertos y el tráfico de cada uno.

```
Enter  detalle del equipo     s  ordenar por IP / descarga / subida
r      volver a escanear      c  dar CAP_NET_RAW al programa (sudo setcap)
```

- Se escanea sola cada 60 s. Los equipos apagados se atenúan; la lista puede tardar unos minutos en
  notarlo porque se apoya en la tabla ARP del sistema.
- **Sistema operativo:** es una estimación a partir de fabricante, puertos, nombre y, si hay permiso, el TTL.
- **Fabricante vacío o «MAC privada»:** los móviles modernos usan MAC aleatoria por red Wi-Fi. No se puede deducir.
- **↓ Entrante / ↑ Saliente** por equipo necesitan que el router envíe NetFlow (ver la página «NetFlow y SNMP»).
  Mientras no llegue nada se ve `n/d` o `–`.
- **TTL (CAP_NET_RAW):** con la tecla `c` el programa ejecuta `sudo setcap` sobre sí mismo (te pide la
  contraseña de sudo en la terminal) y se reinicia. Si copias un binario nuevo hay que repetirlo.

# 2 · Puerta de enlace

Tráfico total que entra y sale por el router: tasa actual, pico, total acumulado desde que arrancaste
y una gráfica de los últimos minutos. Se actualiza cada 2 s.

- Con **SNMP** lee los contadores del propio router. Sin él, muestra los de esta máquina. Se configura aquí mismo:
  `s` abre un asistente (comunidad e IP del router → elegir la interfaz WAN), se aplica al instante y se guarda; `d` lo desactiva.
- Los totales cuentan desde el arranque de lanwatch, no desde el del router.

# 3 · DNS

Mide cuánto tarda en responder cada servidor DNS y si todos devuelven lo mismo.

```
r  ejecutar el test         i  añadir la IP de un DNS para compararlo
d  borrar una IP añadida    ↑ ↓  elegir
```

- Parte del DNS que usa el sistema (`/etc/resolv.conf`). Añade, por ejemplo, `1.1.1.1` u `8.8.8.8` para comparar.
- **1ª consulta** mide al servidor de verdad; **repetición** mide su caché.
- **≠** indica que un servidor devolvió direcciones sin ninguna en común con el primero. Algunos dominios
  con CDN dan IP distintas legítimamente; si lo hacen casi todos, el DNS está filtrando o redirigiendo.

# 4 · Velocidad

Mide latencia, jitter, descarga y subida contra servidores públicos.

```
a      modo automático: el servidor de menor latencia
Enter  probar el servidor elegido de la lista       Esc  cancelar        r  recargar la lista
```

- Cada medida dura unos 8 s por sentido y usa toda la línea: evita lanzarla durante una videollamada.
- Los servidores públicos limitan a quien los usa mucho. Si responde «rechazada» o «403», elige otro o espera.
- La subida depende mucho del servidor elegido; compara varios antes de sacar conclusiones.

# 5 · Ayuda y 6 · About

La ayuda es esta guía. **About** muestra los datos del proyecto y del autor y, sobre todo, el estado de esta
instalación (ejecutable, fichero de configuración, permisos, SNMP, NetFlow…): cópialo si necesitas pedir ayuda.

# Opciones de línea de órdenes

```
--once                 escanea una vez, imprime la tabla y sale (para scripts)
--snmp pública/eth0    contadores del router por SNMP
--netflow 2055|off     puerto UDP del colector NetFlow
--iface end0           interfaz a vigilar
--setcap               dar CAP_NET_RAW (pide sudo) y salir
--help                 todas las opciones y el fichero de configuración
```
