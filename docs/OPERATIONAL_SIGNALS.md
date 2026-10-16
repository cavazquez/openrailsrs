# Enlaces y estados operativos de señales

La referencia es Open Rails 1.6.1, commit
`d16e670da333d26d2edfc97d5631a19dadf49ce5`. No se han cambiado sus
fuentes, DLL, capturas anteriores ni tolerancias.

## Qué se amplió

`TrSignalDir` ahora vincula cada cabezal a su unión TDB y al vector de salida
original. `ROUTE_SET()` comprueba ese enlace contra el itinerario dirigido;
no toma como sustituto el siguiente desvío que encuentre. Un enlace a una unión
inexistente produce un error de importación.

`BLOCK_STATE()` conserva los tres valores nativos: libre (0), ocupado (1) y
obstruido por una unión/reserva incompatible (2). Una reserva ajena no se
convierte en presencia física ni autoriza una llamada de maniobra. El permiso
de maniobra sigue sujeto a su autoridad y a la ocupación efectiva.

Para conducir, varios cabezales NORMAL del mismo objeto WORLD forman una señal
física: se usa el aspecto menos restrictivo del grupo, como `this_sig_lr` en OR.
Cada cabezal conserva su propio color, programa y estado guardado. Un rojo de
otra rama del mismo poste no detiene una rama autorizada; un poste independiente
en rojo continúa protegiendo su bloque. Una espera del despachador afecta a todo
el grupo, no sólo al primer cabezal enumerado.

El bloque protegido también se comparte: una segunda cabeza del mismo poste,
aunque esté un metro más adelante en TDB, no termina el bloque de la primera.
Ambas deben detectar una sección o reserva entre ese poste y el siguiente.
Los scripts de señales C# sin host compatible se rechazan incluso si no hay
catálogo SIGSCR; no se sustituyen por una señal genérica permisiva.

## Evidencia

- `fixtures/compatibility/operational-signals-or161/`: 28 resultados obtenidos
  de `SignalObject.block_state`, `SignalHead.route_set` y `AddHead` originales:
  ocho estados operativos, dieciocho enlaces y dos vinculaciones de rama.
  Comparación discreta exacta, protocolo anterior a la ejecución y Wine
  privado sin pantalla, sonido ni procesos residuales.
- `fixtures/compatibility/operational-signals-routes/`: auditoría de diez
  instalaciones, nueve con cabezales nativos importados. Son **24.489
  cabezales**, **8.995 enlaces** y **46.848 contextos** de programas originales.
  Chiltern anterior y v4 son dos instalaciones del mismo corredor. LIRR no
  produjo cabezales nativos: su fila queda expresamente sin certificación.
- El recibo comprimido conserva todos los enlaces. `source-inputs.json`
  identifica por hash TDB, WORLD y archivos de señales observados después del
  ensayo. Los recursos del autor permanecen fuera de Git.

La auditoría compila los programas utilizados y evalúa sus combinaciones de
ocupación, habilitación, enlace y siguiente aspecto. No representa diez viajes
completos ni certifica todos los sistemas de permisos, despachadores o scripts
de otras rutas. Las funciones desconocidas siguen generando un diagnóstico;
no se reemplazan por resultados favorables.

## Repetir y probar a mano

```bash
python3 -m unittest discover -s scripts -p test_operational_oracles.py
cargo test --locked -p openrailsrs-track --test operational_signal_reference
cargo test --locked -p openrailsrs-sim --lib native_signals::tests
cargo run --locked -p openrailsrs-msts --example signal_operational_audit -- \
  /ruta/local/routes.json /ruta/local/nueva-auditoria
```

El JSON local del último comando contiene nombres de instalación y sus carpetas
originales; no hace falta copiar las rutas al repositorio. Usar una carpeta de
salida nueva conserva los intentos anteriores.

En el menú, elegir **MidEastCoast → Firsby to Spilsby Return**, con el material
original instalado. Tras salir de Firsby, mirar el grupo de señales de la
bifurcación: la cabeza de otra rama puede quedar roja y la cabeza del itinerario
debe gobernar el paso. Abrir **F4** para ver el mismo permiso en el monitor.
Al reservar el bloque otro servicio, ambos deben impedir el avance. Guardar y
reanudar conserva el permiso; no lo transforma en vía libre. La aceptación del
rodeo completo se documenta por separado en
[porciones y transferencias](PORTIONS_AND_SERVICE_TRANSFERS.md).
