# Configurar un EdgeRouter X para lanwatch

> Orientativo: escrito para EdgeOS 2.x y **no verificado en tu router** (no se ha accedido a él).
> Comprueba cada comando en tu versión. Sustituye `192.168.1.232` por la IP de la máquina con lanwatch.

## NetFlow → tráfico por dispositivo (pestaña Dispositivos)

```
configure
set system flow-accounting interface switch0          # una sola interfaz (la LAN) para no contar dos veces
set system flow-accounting netflow server 192.168.1.232 port 2055
set system flow-accounting netflow version 5
set system flow-accounting netflow timeout expiry-interval 30
set system flow-accounting netflow timeout max-active-life 60
commit ; save
```

Después, `lanwatch` mostrará «NetFlow UDP 2055: N flujos…» en la cabecera de la pestaña. Las columnas
↓/↑ son medias de 60 s (los flujos se exportan al caducar; los intervalos de arriba los acortan).

## SNMP → tráfico de la puerta de enlace (pestaña Puerta de enlace)

```
configure
set service snmp community public authorization ro
set service snmp community public client 192.168.1.232   # solo esta máquina
commit ; save
```

Ejecuta `lanwatch --snmp public/eth0` (la interfaz WAN suele ser `eth0`). Si omites `/eth0` usa la primera que no sea `lo`.

## Aviso: offload por hardware

En el ER-X, `hwnat` (offload) saca del kernel los flujos ya establecidos: **ni NetFlow ni los contadores
de `eth0` los ven completos**, y las cifras salen por debajo de la realidad. Si no cuadran con lo que
esperas, comprueba `show ubnt offload` y valora `set system offload hwnat disable` (a costa de rendimiento
de enrutado en el ER-X).
