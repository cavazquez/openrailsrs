# Brooklyn–Hempstead: referencia en condiciones de partida

Esta referencia usa las DLL originales de **Open Rails 1.6.1**, commit `d16e670da333d26d2edfc97d5631a19dadf49ce5`, y los originales `01Brooklyn-Hempstead.act` / `SFexpress-500`. Contiene **32.583 muestras a 20 Hz**, 1629,10 s y 31.361,20 m con los siete vehículos y las dos tareas de estación originales completadas. Es una referencia nativa completa; **la aceptación de Rust está pendiente**.

La declaración se escribió antes del intento. Se mantienen los seis límites: velocidad RMS 0,75 m/s, error máximo de odómetro 45 m, tubería y cilindro RMS 5 psi cada uno, fuerza motriz y freno RMS 5000 N cada uno. No se aumentaron al observar fallos.

## Contexto explícito

El intento 05 heredó `Simulator.Paused=true` y omitió el inicializador climático del visor original. Eso afecta la adhesión aunque `Simulator.Update` haga avanzar el tren. Sus bytes y replays fallidos se conservan en `../lirr-native05`; no representan la aceptación de una partida despausada. El intento 06 falló al compilar nuestra telemetría, antes de ejecutar la física original, y también se conserva.

El intento 07 declara `Paused=false`, adhesión 100 y variación 0. La actividad original ya tiene clima Clear. El helper completa sus entradas estáticas igual que `Weather.SetInitialWeatherParameters` y `UpdateWeatherParameters` del visor original: visibilidad 20.000 m, nubes 0,05, precipitación 0 y liquidez 1. No modifica el ACT, los trenes ni una DLL. No hace un calentamiento, no impone velocidades, fuerzas, estados de rueda ni presiones observadas. El ruido de adhesión se desactiva expresamente para este ensayo determinista. Estos parámetros se deben compartir con Rust; no se deben copiar estados evolutivos de las filas para conseguir coincidencia.

## Evidencia

`reference.csv.gz` comprime sin pérdida el CSV original de 49.921.414 bytes. Tiene 164 columnas: mandos reales y demandados, fuerzas locales y orientadas por coche, presiones y fuerzas de freno, velocidad local, radio e inercia motrices, patinaje, factor ambiental y estado de cada eje. El signo del powercar trasero permanece explícito. El CSV registra el número adaptativo de subpasos y el error/espera de integración del eje, no sólo el resultado total del tren.

`coverage.json` exige ambas estaciones, duración/distancia completas, formación original y el contexto declarado. `telemetry-integrity.json` comprueba que todas las filas mantienen el simulador despausado, Clear seco y la suma orientada por coche. `initial.json` conserva la inicialización original antes del primer tick. Los campos de fuerza originales aún no actualizados en t = 0 se conservan; esa muestra no se elimina de la aceptación.

`capture-execution.json`, `source-inputs.json`, las fuentes del helper y `wine-isolation/` identifican la ejecución y sus DLL verificadas. El prefijo privado quedó sin procesos y no cargó controladores del escritorio. Las rutas del equipo se sustituyen por marcadores, manteniendo los hashes separados de los recibos privados originales.

No se incluyen archivos originales ENG/WAG/CON/PAT/TDB, un grafo importado, DLL, ejecutables ni un prefijo Wine. Los originales se identifican mediante hashes y deben obtenerse desde su autor.

## Reproducción y aceptación pendiente

Desde la raíz del repositorio, con los originales instalados, `scripts/capture_lirr_full_service.py --native-only --protocol oracles/lirr-full-service-live-protocol.json --activity RUTA/01Brooklyn-Hempstead.act --consist RUTA/SFexpress-500.con --wine-prefix PREFIJO_PRIVADO --out-dir SALIDA_NUEVA` vuelve a ejecutar las DLL en un prefijo aislado. Ese modo no compila ni aprueba Rust.

Para comparar Rust, se importa el mismo ACT/CON, se descomprime el CSV en una carpeta temporal y se pasa a `target/debug/examples/activity_trajectory escenario/scenario.toml referencia.csv rust.csv`, usando binarios recién compilados. Se exige exactamente la misma cantidad de muestras y mandos reales, además de los seis umbrales. Un recorrido corto, una terminación anticipada o la comparación contra otro contexto no cuentan como aprobación. El manifiesto conserva hashes del archivo comprimido y de sus bytes originales.
