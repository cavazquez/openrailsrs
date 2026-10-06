# Sonido original: reproducción y comprobaciones

Los sonidos proceden de los SMS y WAV del contenido instalado. El hilo de audio
lee y decodifica los archivos; Bevy envía estados de cada vehículo mediante una
cola acotada. Cambiar de cámara mantiene los bancos y bucles ya cargados.

## Cambios comprobables

- Los disparadores por distancia guardan la distancia anterior y comparan
  umbrales cuadrados, como Open Rails. `Distance_Dec_Past` puede iniciar un
  bucle al cargar una fuente que ya está cerca. Permanecer bajo el umbral no
  reinicia el sonido en cada cuadro.
- Activación y desactivación conservan sus distancias distintas. Un bloque
  declarado sin distancia usa el valor original de 1000 m; `Distance (0)`
  desactiva esa condición. Se conserva el corte general de 2 km.
- La atenuación de fuentes externas usa la fórmula inversa de distancia de
  OR 1.6.1: referencia de 8 m y ganancia de 0,025 a la distancia de desactivación
  cuando está dentro del límite general. `Ignore3D` y `Stereo` evitan esa
  atenuación; se respetan también sus valores booleanos explícitos.
- En cabina o pasajeros, las fuentes externas usan el paso de sonido declarado
  por el coche donde está el jugador, incluso al escuchar otro servicio.
  `ORTSExternalSoundPassedThroughPercent` reemplaza el valor predeterminado
  original de 50 %. No se afirma que un SMS externo sea audible en cabina si
  sus condiciones de cámara lo excluyen.
- Cada motor usa sus propias RPM y cada vehículo sus propias presiones de
  freno. Los coches separados conservan presión y distancia propias, sin
  heredar la velocidad o demanda del tren en marcha. Los vehículos sin motor
  no reciben la carga de la locomotora. La ausencia de un parámetro de consumo
  no cambia la escala de un diésel con regulador de RPM a la de un eléctrico.
- Los cambios del cilindro activan los eventos de freno del tren; los de
  tubería usan sus propios eventos. Se comprueban cada medio segundo de
  simulación, con umbral de 0,1 PSI y un evento de finalización al estabilizarse,
  siguiendo `AirSinglePipe`. Un cambio continuo no reinicia el siseo cada
  cuadro. Los eventos de freno independiente no se inventan usando el cilindro
  del freno del tren.
- Se leen referencias y porcentajes dentro de `Include` mediante el lector
  acotado del contenido. Los programas interiores se conservan por coche;
  al cambiar de coche de pasajeros no se reutiliza el interior del primero.

La mezcla del dispositivo y la del ensayo WAV comparten el limitador a −1 dBFS.
No se reemplaza un WAV ausente por un sonido inventado ni se descarga un recurso
de terceros para rellenarlo. El diagnóstico y la auditoría indican las
referencias que faltan.

## Oráculo de Open Rails 1.6.1

[openrails-audio.json](../oracles/openrails-audio.json) se capturó ejecutando
las DLL verificadas de la versión fijada, sin arrancar un simulador ni un
dispositivo de audio. Comprueba el lector SMS original, siete pasos de
`ORTSVariableTrigger` con cruces y límites exactos, y 36 valores calculados con
`SoundSource.SetRolloffFactor`, además de los identificadores originales de
eventos de freno. Rust admite un error absoluto de `1e-6` para la
ganancia y exige coincidencia de comandos y umbrales. La
[procedencia](../oracles/openrails-audio-provenance.json) contiene hashes del
ejecutable, DLLs, ayudante, fixture y salida; la referencia se verifica junto a
los demás oráculos.

Para recapturar con una instalación original 1.6.1 y .NET Framework disponible
en un prefijo Wine, elegí una carpeta de salida nueva:

```bash
python3 scripts/capture_audio_reference.py \
  --installation-root "/ruta/Program Files/Open Rails" \
  --wine-source-prefix "/ruta/prefijo-original" \
  --wine-prefix tmp/or161-wine-prefix \
  --out-dir tmp/audio-reference
```

El script trabaja sobre una copia privada. No cambia el contenido ni sustituye
automáticamente la referencia versionada.

## Comprobar una formación sin abrir audio

Compilá los binarios y el ejemplo con los mismos ajustes habituales del proyecto:

```bash
cargo build --locked --workspace --all-features --bins --examples
python3 scripts/check_native_audio.py \
  --route-root "/ruta/Content/Chiltern/ROUTES/Chiltern" \
  --consist "/ruta/Content/Chiltern/TRAINS/CONSISTS/Bristol Pullman.con" \
  --out-dir tmp/pullman-audio
```

Se puede repetir `--consist` para probar varias formaciones. El script genera
cabina y exterior, WAV, un informe por vista y `report.json`. Comprueba señal,
duración y ausencia de saturación; todos los avisos provocan fallo por defecto.
Si se conoce un paquete incompleto, `--allow-missing-wav` conserva y enumera
esas faltas como `pass_with_missing_wav`; no tolera otros avisos. La lista
completa indica cada WAV referenciado y su SMS de origen. Los archivos
generados son locales y no forman parte del contenido versionado.

El ejemplo `native_oracle` acepta `--probe-device` después de la vista para
comprobar la apertura del dispositivo y la mezcla real **con volumen cero**.
Requiere `OPENRAILSRS_DISABLE_AUDIO` sin definir; el informe debe indicar
`device: true`. No sustituye la escucha manual.

En Demo Model 1 la Class 47 declara WAV de material compartido GP38 y otros
archivos que no vienen en el paquete: la prueba debe conservar esas faltas.
La auditoría del menú indica el destino donde colocar recursos del autor.

## Límites

Este oráculo comprueba semántica y ganancia, **no identidad acústica de una
partida completa con OpenAL**. Faltan, entre otras cosas, propagación estéreo
espacial/Doppler equivalentes, corrección por apertura de ventanas, sonidos
automáticos de cambios de tipo de vía y todos los comandos/scripts de sonido
del contenido. El freno independiente necesita su propia física y telemetría;
la primera muestra de presiones fija la referencia al iniciar/restaurar para
evitar cambios de sonido artificiales. La carga eléctrica y la presión de admisión de vapor todavía
usan demanda como aproximación; la variable de freno dinámico no reproduce
un subsistema completo. Ver [alcance de tracción](TRACTION_SUPPORT.md).

La escucha manual se describe en la sección 36 de
[PLAYER_MANUAL_TESTS.md](PLAYER_MANUAL_TESTS.md).
