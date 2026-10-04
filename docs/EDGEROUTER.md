# Para qué sirve

Para ver cuánto tráfico mueve **cada equipo** (columnas ↓ Entrante / ↑ Saliente de Dispositivos), el
router tiene que contárselo a lanwatch: esta máquina solo ve su propio tráfico. Lo hace con **NetFlow**:
el router envía por UDP un resumen de cada conexión y lanwatch lo suma por equipo. Para el tráfico
total del router (pestaña Puerta de enlace) se usa **SNMP**.

> Orientativo: escrito para un EdgeRouter X con EdgeOS 2.x y no verificado en tu router (no se ha
> accedido a él). Comprueba cada comando en tu versión. En estos ejemplos, 192.168.1.232 es la IP de
> esta máquina, la que ejecuta lanwatch.

# Paso 1 · lanwatch escucha (ya está hecho)

Por defecto lanwatch escucha NetFlow en el puerto UDP 2055: lo puedes ver arriba en «NetFlow ahora».
Para cambiar el puerto, arranca con `--netflow <puerto>`; para apagarlo, `--netflow off`.

# Paso 2 · activar NetFlow en el router

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
- `version 5`: es la única que entiende lanwatch por ahora.
- Los dos `timeout` hacen que el router envíe los datos cada pocos segundos en vez de esperar a que
  cada conexión termine (por defecto puede tardar mucho y las cifras llegarían a trompicones).

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

Y arranca lanwatch con la interfaz WAN del router (normalmente `eth0`):

```
lanwatch --snmp public/eth0
```

Para no escribirlo cada vez, añade la línea `snmp=public/eth0` a `~/.config/lanwatch.conf`.
Si omites `/eth0`, usa la primera interfaz que no sea `lo`.

# Aviso · offload por hardware

> En el ER-X, el offload por hardware (hwnat) saca del kernel las conexiones ya establecidas, y
> **ni NetFlow ni los contadores SNMP las ven completas**: las cifras saldrán por debajo de la realidad.
> Si no cuadran con lo que esperas, comprueba `show ubnt offload` y valora `set system offload hwnat disable`
> (a cambio de más carga de CPU en el router).
