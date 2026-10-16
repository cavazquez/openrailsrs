# Regiones de rodadura originales y conducción (#238)

El sonido de vía se elige debajo de cada coche. El importador conserva `TTYPE.DAT` y los límites de los archivos WORLD `.ws`; la consulta por chainage respeta el sentido y la ocurrencia concreta de un vector repetido en el recorrido. El valor nativo de conservación (`int.MaxValue`, representado como `None`) mantiene el banco anterior. Tampoco se cambia de banco a una velocidad de 0,1 m/s o menor. El índice 7 de SCE es un banco real de puente de hormigón, no un valor de conservación.

El mixer elige un único banco regional por coche y ubicación de escucha. Las voces de los bancos anteriores se atenúan durante la transición. Los sonidos propios del coche siguen teniendo sus canales; su presencia no demuestra que se haya cargado el banco regional. `VoiceReport.track_type` expone el índice del banco real junto con el programa, WAV, grupo, ganancia y coche de cada voz.

## Evidencia nativa y alcance

`fixtures/compatibility/track-sound-or161` conserva 100 consultas reales de `WorldSounds.GetTType` de OR 1.6.1, con cinco tipos y ambos sentidos en la ruta SCE original. La referencia SHA-256 es `822005282be251c55f0b727299895acc86c83116c7bc6036fa798e5dd6a3d922`. Sus entradas, salida, DLL, fuentes y tolerancias se mantienen sin regeneración: tipo exacto, incluido el valor de conservación, y sondas alejadas al menos 0,5 m de los límites. El comparador dedicado obtuvo 100 coincidencias exactas.

La conducción es una aceptación Rust separada, con el CON original `MT_MT_Class 47 & 6 mk2 PP.con`, su locomotora y seis Mk2 originales. `LiveDriveSession` integra movimiento, frenos y cambio de sentido con pasos de 50 ms. Se acelera hasta la zona de túnel, se frena hasta detenerse, se invierte la marcha y se vuelve a frenar. No se prescribe velocidad ni fuerza. Los siete coches atraviesan los tipos 0 (normal), 3 (túnel) y 7 (puente de hormigón); durante los cruces hay coches sobre regiones diferentes.

La comprobación nativa durante la conducción usa sólo intervalos entre sondas originales adyacentes con el mismo resultado y sentido. Excluye intervalos con un límite o decisión de conservación dentro. Exige una comprobación por tipo, coche y sentido. No presenta estos intervalos ni el audio como paridad de un viaje OR completo.

La pausa compara veinte pasos con tiempo cero sin cambio de snapshot. El guardado se realiza con la formación repartida entre regiones; una sesión nueva restaura el JSON y obtiene los mismos tipos debajo de cada coche antes de continuar la conducción. El estado de presentación de audio se conserva aparte: los cortes PCM se inicializan con el tipo efectivo que llevaba cada coche; el snapshot físico no pretende serializar una voz del dispositivo.

## Ensayo CPU y PCM

Con el contenido original instalado, ejecutar desde la raíz del repositorio:

```sh
cargo run -p openrailsrs-audio --example track_sound_drive -- \
  '/home/cristian/Documentos/Open Rails/Content/Demo Model 1' \
  tmp/track-sound-drive-new
```

La carpeta de salida debe ser nueva y estar fuera del contenido original. El ejemplo verifica sus hashes de ruta, importa una representación derivada y crea `scenario.toml`, `drive.json`, `paused-snapshot.json`, seis `transition-*.wav`, `pause-resume.wav` y `receipt.json`. No abre un dispositivo de audio ni usa GPU o Wine. Los archivos originales se leen sin modificarlos.

Los clips son ventanas breves del recorrido real, con una semilla explícita del tipo efectivo al cortar y 1,8 s de estado final mantenido sólo para inspeccionar la transición del mixer. Ese tiempo equivale a diez constantes del filtro exponencial de 0,18 s. El umbral de voz activa permanece en 0,0001. El primer intento se conserva en `tmp/next-ten-20261015/track-sound-drive01`: conducción, comparación nativa y silencio de pausa pasaron, pero su auditoría de bancos se efectuó demasiado pronto, después de 0,4 s. La revisión del protocolo amplió la observación del filtro; no cambió los tipos esperados ni los umbrales.

Cada clip exige voces regionales de grupo `Rolling` con el `track_type` esperado y el programa exterior exacto de `TTYPE.DAT`, sin otro banco regional activo sobre el umbral. Cada WAV de esas voces se vincula a una ruta original y SHA-256; se cuentan por separado las voces de rodadura propias del coche. También se exige PCM no nulo y silencio exacto en el intervalo interior de la pausa. Estas verificaciones acreditan selección, decodificación y mezcla, sin juicio de escucha ni equivalencia acústica con OR.

El render usa PCM estéreo de 16 bits a 44.100 Hz, volumen 0,35 y una distancia virtual fija de 12 m para cada coche. Es una condición de inspección repetible de los bancos, no una reconstrucción de la posición de un micrófono en un viaje. Las variables de motor, freno, sentido y velocidad proceden de los estados de conducción del coche.

El recibo inventaría todas las referencias autorales. Distingue programas de stock cuyos eventos no se activaron, regiones no seleccionadas y programas interiores que los clips exteriores no usan. La ausencia de alguno de esos recursos se declara, aunque no invalide una muestra exterior seleccionada y resuelta. Ningún SMS, WAV, shape ni textura original se agrega al repositorio; el PCM y la ruta importada quedan en la carpeta temporal. La evidencia versionada contiene sólo metadatos, hashes y una traza derivada comprimida.

El intento 02 pasó con 3.485 pasos (174,25 s) y 1.122,8349 m recorridos. Sus 4.152 comprobaciones de intervalos cubren los siete coches, los tres tipos y ambos sentidos. Los seis clips contienen 42 comprobaciones de voces regionales activas y 72 voces de rodadura propias de los coches contadas aparte. En ambos sentidos se resolvieron `x_mt_rail_low1.wav` (tipo 0), `x_steam_rail_low4.wav` (tipo 3) y `x_steam_rail_low8.wav` (tipo 7). No faltó ningún recurso exterior seleccionado. Las 99 referencias únicas ausentes corresponden a 21 referencias de programas de stock y 78 referencias del catálogo no seleccionado; no se presentan como sonidos reproducidos.

El [recibo del ensayo](fixtures/compatibility/track-sound-drive/receipt.json), la [traza comprimida](fixtures/compatibility/track-sound-drive/drive.json.gz) y el [índice de evidencia](fixtures/compatibility/track-sound-drive/evidence.json) conservan los hashes del ejecutable medido, fuente observada, muestras originales y PCM generado. El recibo SHA-256 es `a6fe93ee25b7ca72e23941aa61e5df1a9999ef967c27035c98b38e1c38d5f8e7`. El intento 01 fallido y su protocolo inicial también están archivados. El protocolo conserva su encabezado inicial; su campo `revision=2` y el índice de evidencia identifican la ampliación de observación declarada antes del segundo ensayo.

El validador de la evidencia registrada no recompila ni recaptura:

```sh
python3 scripts/test_track_sound_drive_evidence.py
python3 scripts/test_track_sound_drive_evidence.py --originals \
  --pcm-directory tmp/next-ten-20261015/track-sound-drive02
```

La ejecución con `--originals` y `--pcm-directory` pasó: comprobó el recibo y volvió a verificar los hashes de los 101 archivos originales resueltos, los siete PCM y el silencio interior de la pausa. La forma básica de validación de metadatos puede ejecutarse sin instalar ese contenido; no se interpreta como una nueva ejecución del viaje.

## Comprobación manual

Abrir el `scenario.toml` generado con el visor habitual, usando `--live` y `--route-root` apuntando a la ruta SCE original. Avanzar por el puente y la entrada del túnel, observar una formación repartida a ambos lados y cambiar el coche de escucha. El tipo debe seguir la vía debajo de ese coche. No hace falta accionar una tecla de banco ni abrir una pantalla para que cambie.

Frenar hasta detenerse, invertir y repetir el cruce. Al detenerse cerca de un límite debe conservarse el banco anterior. Pausar y reanudar, y guardar/restaurar mientras los coches están repartidos entre regiones; continuar en ambos sentidos. Una revisión humana puede valorar los cambios audibles y la continuidad de la mezcla. El ensayo CPU documentado aquí no afirma haber realizado esa escucha ni sustituye la aceptación manual del dispositivo.
