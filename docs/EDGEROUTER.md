# Para qué sirve

Para ver cuánto tráfico mueve **cada equipo** (columnas ↓ Entrante / ↑ Saliente de Dispositivos), el
router tiene que contárselo a lanwatch: esta máquina solo ve su propio tráfico. Lo hace con **NetFlow**:
el router envía por UDP un resumen de cada conexión y lanwatch lo suma por equipo. Para el tráfico
total del router (pestaña Puerta de enlace) se usa **SNMP**.

> Los pasos de EdgeRouter (EdgeOS 2.x) son orientativos y no están verificados en tu router; los de
> OpenWrt sí se han probado en un router real. Comprueba cada comando en tu versión. En estos ejemplos, 192.168.1.232 es la IP de
> esta máquina, la que ejecuta lanwatch.

# Paso 1 · lanwatch escucha (ya está hecho)

Por defecto lanwatch escucha NetFlow en el puerto UDP 2055: lo puedes ver arriba en «NetFlow ahora».
Para cambiar el puerto, arranca con `--netflow <puerto>`; para apagarlo, `--netflow off`.

# Paso 2 · activar NetFlow en el router (EdgeRouter)

Entra en el router por SSH (la puerta de enlace) y escribe:

```
ssh admin@192.168.1.1
configure
set system flow-accounting interface switch0
set system flow-accounting netflow server 192.168.1.232 port 2055
set system flow-accounting netflow version 5
set system flow-accounting netflow timeout expiry-interval 30
set system flow-accounting netflow timeout max-active-life 60
commit
save
exit
```

- `interface switch0`: la interfaz de la **LAN**. Pon solo una, o los flujos se contarán dos veces.
  Si tu LAN no es `switch0`, mira `show interfaces` en el router.
- `version 5`: lanwatch entiende NetFlow **v5, v9 e IPFIX (versión 10)**; cualquiera vale. v9/IPFIX envían plantillas, así que la primera
  cifra puede tardar uno o dos minutos en aparecer. Solo se cuenta el tráfico IPv4.
- Los dos `timeout` hacen que el router envíe los datos cada pocos segundos en vez de esperar a que
  cada conexión termine (por defecto puede tardar mucho y las cifras llegarían a trompicones).

# Paso 2 · activar NetFlow en el router (OpenWrt)

OpenWrt no exporta NetFlow de serie: se usa **softflowd**. Entra por SSH (`ssh root@192.168.1.1`) e instálalo:

```
opkg update && opkg install softflowd
```

(En OpenWrt 24.10 o posterior el gestor es `apk`: `apk update && apk add softflowd`.)

El script de arranque que trae el paquete **ignora los timeouts**, y con los de fábrica una conexión tarda
media hora o más en exportarse: lanwatch no recibiría nada. Por eso se crea un servicio propio con los
timeouts bajos. Primero se desactiva el original:

```
/etc/init.d/softflowd stop
/etc/init.d/softflowd disable
```

Después, el servicio nuevo (pega el bloque entero):

```
cat > /etc/init.d/softflowd-lw <<'EOF'
#!/bin/sh /etc/rc.common
START=99
USE_PROCD=1

start_service() {
    procd_open_instance
    procd_set_param command /usr/sbin/softflowd -d -i br-lan -m 8192 \
        -n 192.168.1.232:2055 -v 9 \
        -p /var/run/softflowd.pid -c /var/run/softflowd.ctl \
        -t maxlife=60 -t expint=30 -t general=30 \
        -t tcp=30 -t tcp.rst=10 -t tcp.fin=10 -t udp=30 -t icmp=30
    procd_set_param respawn
    procd_close_instance
}
EOF
chmod +x /etc/init.d/softflowd-lw
/etc/init.d/softflowd-lw enable
/etc/init.d/softflowd-lw start
```

- `-i br-lan`: la interfaz de la **LAN**. Si capturas en la WAN solo verás la IP pública tras el NAT, no
  los equipos. Pon una sola interfaz, o los flujos se contarán dos veces.
- `-n 192.168.1.232:2055`: IP de esta máquina y puerto de lanwatch. Si cambian, edita esa línea y haz
  `/etc/init.d/softflowd-lw restart`.
- `-v 9`: v5, v9 e IPFIX valen. Solo se cuenta el tráfico IPv4.
- Los `-t` hacen que los flujos caduquen y se envíen cada 30–60 s. Es lo que hace que lanwatch reciba datos.

Para comprobar que el router exporta (tras un par de minutos):

```
softflowctl statistics | grep -E "active|expired|exported"
tr '\0' ' ' < /proc/$(pidof softflowd)/cmdline; echo
```

`Flows exported` debe subir solo. Si `expired` sigue en 0 con flujos activos, la segunda orden te enseña con
qué argumentos corre: tienen que aparecer los `-t` (`ps` los corta). Si `active` es 0, softflowd no ve
tráfico: revisa el nombre de la interfaz con `ip link show`.

> En OpenWrt, el «flow offloading» del cortafuegos (Red → Cortafuegos → Enrutamiento/NAT offloading) saca
> las conexiones del kernel y softflowd no las ve completas: las cifras saldrán por debajo de la realidad.
> Si no cuadran, desactívalo.

# Paso 3 · comprobar que llega

En unos 30–60 segundos, «NetFlow ahora» (arriba) pasará a **✔ recibiendo** con un contador de flujos,
y en Dispositivos las columnas ↓/↑ dejarán de ser `n/d`. Las cifras son medias de los últimos 60 s.

Si no llega nada:

```
sudo tcpdump -ni any udp port 2055
```

- Si `tcpdump` no ve paquetes, el problema está en el router: repasa el paso 2 (`show system flow-accounting`).
- Si los ve pero lanwatch no cuenta nada, mira si el cortafuegos de esta máquina bloquea UDP 2055.
- Si la cabecera de Dispositivos dice que no se pudo abrir el puerto, otro programa lo está usando.

# SNMP · tráfico de la puerta de enlace

En el router:

```
configure
set service snmp community public authorization ro
set service snmp community public client 192.168.1.232
commit
save
exit
```

Después, en lanwatch, ve a la pestaña **Puerta de enlace** y pulsa `s`: te pide la comunidad (`public`) y la
IP del router, lo consulta, te muestra sus interfaces para que elijas la WAN (normalmente `eth0`) y lo activa
al momento. Se guarda en `~/.config/lanwatch.conf`, así que no hay que repetirlo. Con `d` se desactiva.

Si prefieres la línea de órdenes: `lanwatch --snmp public/eth0`, o la línea `snmp=public/eth0` en el fichero.

# Aviso · offload por hardware

> En el ER-X, el offload por hardware (hwnat) saca del kernel las conexiones ya establecidas, y
> **ni NetFlow ni los contadores SNMP las ven completas**: las cifras saldrán por debajo de la realidad.
> Si no cuadran con lo que esperas, comprueba `show ubnt offload` y valora `set system offload hwnat disable`
> (a cambio de más carga de CPU en el router).
