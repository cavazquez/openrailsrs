# Entradas ambientales registradas para la física nativa

El replay opcional permite comparar la física con Open Rails 1.6.1 usando el mismo ruido ambiental externo. No introduce velocidades, fuerzas, posiciones, RPM ni resultados de adherencia observados en la simulación. La partida habitual conserva su modelo ambiental actual; esto no constituye un puerto del generador aleatorio completo de Open Rails.

El ensayo de Chiltern conserva intactos `scenario_oracle.toml`, los mandos originales y su referencia de 15.003 muestras. La variante `examples/chiltern_local/scenario_oracle_adhesion.toml` añade únicamente estas opciones:

```toml
[simulation.native_physics]
adhesion_input_file = "physics/native-adhesion-inputs.csv"
adhesion_input_step_s = 0.05
```

El CSV debe estar dentro del directorio del escenario, también después de resolver enlaces simbólicos. Su cabecera es:

```csv
time_s,vehicle,input_factor,base_factor,filter_x0,filter_x1,filter_y0,filter_y1
```

`vehicle` identifica el coche original de la formación, antes de cualquier corte o permutación. Cada locomotora debe tener una fila inicial y una muestra por paso de física hasta cubrir la duración declarada. Las cuatro columnas del filtro sólo se admiten en la fila inicial; las siguientes deben estar vacías. Los índices repetidos, huecos, datos no finitos y fuentes que no cubren el ensayo impiden cargarlo.

En cada paso se obtiene el ruido externo con `input_factor − base_factor`, en precisión Single. Se suma a la base que nuestra simulación calcula según su propia posición y clima. Así, la base registrada no fija el momento en que nuestro tren entra en el tramo resbaladizo. El filtro Butterworth de primer orden y 1 Hz se integra con el orden de operaciones de la versión fijada de Open Rails. Su salida queda limitada a `[0.05, 2.5]` para el eje, como en el original. En LiveDrive esa salida aporta la base de cada contacto rueda–riel; después se aplican los factores habituales de clima, arena y superficie. Sin CSV, el contacto conserva su cálculo habitual.

La captura ambiental de Chiltern reproduce byte por byte la referencia original de movimiento. Sus dos filtros empiezan con `x = [0, 0]` e `y = [0, 0]`; no se aplica un reinicio artificial a 0.5. Las salidas observadas del filtro se guardan aparte en `docs/fixtures/compatibility/twenty-20261015/chiltern-adhesion161-native01/filter-oracle.json`, exclusivamente para una prueba aislada. El código de ejecución de la partida no lee ese archivo.

Los estados guardados conservan los dos retardos del filtro, cursor, índice original y SHA-256 del CSV. Reemplazar el CSV en la misma ubicación invalida la restauración. Al cortar o recoger una sección, el estado viaja con la identidad del coche, junto con su fuente inmutable; los filtros de las locomotoras que siguen perteneciendo al catálogo avanzan durante la espera aunque estén desacopladas. Un acople de formaciones independientes exige relojes coincidentes cuando alguna utiliza estas entradas.

El agotamiento del registro o un cambio del paso de física produce un error explícito. El visor detiene la sesión con ese diagnóstico y frena; el runner devuelve un error. No se prolonga la última muestra ni se rellena la trayectoria para aparentar cobertura. El runner múltiple anterior rechaza esta opción mientras no implemente el mismo contrato.

El mismo cambio conserva la fase de `AbsSpeed` del original: sin patinaje, la curva de tracción usa la velocidad absoluta cacheada por el anterior `TrainCar.Update`; con patinaje usa el canal de rueda. Ambos son estados calculados que se conservan al guardar, no canales tomados de la referencia.

La resistencia de curva usa otra magnitud del original: `Train.WagonCoefficientFriction`, calculada con la base ambiental sin el ruido filtrado. Las locomotoras la publican en orden; los coches siguientes usan la última publicación y los coches anteriores a la primera locomotora conservan la del paso previo. Su valor inicial es 0.35 y también forma parte del guardado. La fórmula mantiene el orden de operaciones Single, incluida la conversión de masa a toneladas.

La importación de `.trk` conserva los nombres de bloques nativos y los indicadores vacíos a través de los `include` de `OpenRails`. Esto permite reconocer `MilepostUnitsMiles ()` y seleccionar el estándar imperial de peralte del autor. Las curvas y el peralte usados por la partida provienen de la geometría original y de ese estándar; las fuerzas observadas se reservan para la comparación aislada.

## Validación

La suite escrita en `crates/openrailsrs-sim/tests/native_adhesion_inputs.rs` verifica las 30.006 salidas nativas del filtro con el límite previamente declarado de `1e-6`, además de rutas seguras, identidad, guardado, corte/recogida, separación del ruido y agotamiento. La [comparación completa de Chiltern14](fixtures/compatibility/twenty-20261015/physical-final14/chiltern-service/report.json) ya cubrió los 750,1 s históricos: error RMS de velocidad 0,0137895 m/s y error máximo de odómetro 5,803 m, con los límites previos intactos. El [check completo](fixtures/compatibility/twenty-20261015/final-check/result.json) también pasó con 2.346 pruebas Rust, cero fallos, contratos Python, oráculos y servicio completo. El [recibo del batch](fixtures/compatibility/twenty-20261015/physical-final14/result.json) fija build, fuentes, datos y binarios. La geometría importada y su alcance de tres estaciones se explican en [OR_PARITY.md](OR_PARITY.md).

Para probarlo a mano, cargá la variante del escenario, avanzá, guardá y reanudá: la continuación debe conservar los filtros y la trayectoria. En una copia del escenario, cambiar el CSV antes de restaurar debe informar incompatibilidad de la fuente. Aumentar la duración por encima de la cobertura debe impedir iniciar; intentar continuar después del último paso debe informar que se agotaron las entradas. El archivo de datos es una herramienta de ensayo: para jugar con tiempo y clima propios, usá los escenarios habituales sin esta opción.
