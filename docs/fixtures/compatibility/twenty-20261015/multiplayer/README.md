# Multijugador: evidencia de los issues #407–#410

Se conservaron tres capturas auténticas de 1280×720: [anfitrión/despachador](host.png),
[conductor](driver.png) y [espectador](spectator.png). Se renderizaron con Vulkan en
una AMD Radeon RX 7600 (RADV NAVI33), PCI `0000:03:00.0`. El anfitrión estuvo abierto
con un cliente a la vez; los dos clientes se capturaron sucesivamente.

[qa-summary.json](qa-summary.json) identifica el binario, versiones, hashes de
entradas, capturas, medidas y cierre. [pixel-review.json](pixel-review.json) es la
revisión posterior de los tres PNG, conservada sin reescribir el resultado original
que todavía indicaba revisión de píxeles pendiente. Los paneles iniciales se leen
y entran en la resolución usada. El desplazamiento con rueda queda pendiente manual.

[El extracto CPU](cpu-tests-excerpt.txt) contiene los 14 PASS del bloque: siete de
protocolo/transporte, cinco del plugin real del visor y dos de persistencia. El caso
principal conecta cinco identidades: dos conductores, despachador, fogonero y
espectador. Ocho es el límite del protocolo, no una concurrencia ensayada aquí.
El primer ensayo del workspace tuvo fallos en otros bloques; este recibo no presenta
esa ejecución completa como aprobada.

Los clientes de las capturas mostraron RTT de 25 ms por loopback. La diferencia
entre tiempo del estado mostrado y reloj de render fue 0,05 s en los clientes y
0,10 s en el host. No son medidas WAN ni sincronización de relojes de máquinas.
La prueba CPU de latencia usa un proxy TCP con 60 ms por sentido y exige RTT entre
100 ms inclusive y 4 s exclusive; el valor exacto no quedó impreso en su log.

El guardado se prueba durante una sesión TCP: se cierran extremos, se restaura un
mundo cargado independientemente y se reconecta la identidad; roles, circulación,
presiones, reservas, hora y clima se conservan. Un caso adicional comprueba dos
calderas termodinámicas y movimiento de ambas locomotoras después de recuperar los
reguladores. No se publican guardados privados, claves, roster completo ni datos del
jugador. La guía de uso está en [MULTIPLAYER_VIEWER.md](../../../../MULTIPLAYER_VIEWER.md).
