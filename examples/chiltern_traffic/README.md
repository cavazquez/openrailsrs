# Chiltern local con tráfico

Northolt Park → South Ruislip → West Ruislip, con ocho coches y dos servicios
Pullman adicionales. Es el servicio recomendado del menú y del lanzador.
El horario de demostración fue creado aquí; no reproduce el tráfico de una
actividad original de Open Rails.

El Pullman adelantado comienza cerca de South Ruislip, atiende pasajeros y
libera las señales al avanzar. Después de West Ruislip entra en una vía posterior
para que su cola deje libre la terminal. El Pullman contrario aparece a los
700 s sobre la vía contigua, cruza el corredor y para en South Ruislip.

Los tres trenes usan la misma física, puertas, controlador automático y reloj.
La ocupación abarca cada vehículo completo y ambos sentidos de un tramo físico.
La pausa congela todos; guardar/cargar recupera posiciones, paradas y servicios
que aún esperan su salida. Los modelos y texturas se comparten en GPU.

Los tramos adicionales se importaron del TDB Chiltern instalado. `provenance.json`
registra su hash y los identificadores nativos. Se mantiene el mismo recorrido
del jugador y los extremos de andén de `chiltern_local`; el subconjunto de grafo
contiene sólo las vías necesarias para esta demostración.

```bash
./scripts/run_chiltern_service.sh
./scripts/run_chiltern_service.sh --autodrive --cab
```

Elegí **Chiltern local con tráfico** y **birmingham_pullman**. F7 muestra el
horario; la salida de Northolt es a las 10:00. `+` acelera el tiempo y `−` lo
reduce. M muestra las ocupaciones y los otros trenes en violeta. Para conservar
el servicio anterior sin tráfico:

```bash
CHILTERN_SERVICE=examples/chiltern_local/scenario.toml \
  ./scripts/run_chiltern_service.sh --direct
```

La prueba `chiltern_services_stop_clear_signals_and_restore_together` completa
las tres paradas del jugador, dos del adelantado y una del contrario, comprueba
el paso de rojo a libre y compara una restauración completa. SIGSCR completo,
scripts C# y paridad física del servicio original siguen fuera de esta prueba.
