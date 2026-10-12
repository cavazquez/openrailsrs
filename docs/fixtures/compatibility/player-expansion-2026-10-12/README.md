# Segunda tanda de 30 issues: aceptación

Los criterios y las 30 pruebas manuales están en [PLAN_30_2.md](../../../PLAN_30_2.md). [acceptance.json](acceptance.json) registra los ensayos, capturas, configuración y costes; los reportes de servicio y Class 47 están separados. Los archivos descargados, datos del jugador, binarios y DLL de Open Rails no forman parte de esta evidencia.

## Qué se comprobó

- Física, controles, tutoriales, repetición, guardado y errores con las pruebas del workspace. El instructor se comprueba con pendiente, agarre, masa personalizada y ausencia de objetivo; consultar un consejo no cambia controles ni estado.
- Masa diésel y peso adherente con cinco puntos obtenidos de las DLL originales de Open Rails 1.6.1; tolerancia 0,02 kg por los cálculos en `float` del original. No se afirma masa variable completa de reservas de vapor ni arena.
- Northolt Park, South Ruislip y Gerrards Cross desde cabina y exterior, edificios y bosques a distancia, puente con cámara baja y sombras de contacto desactivadas/8/16 pasos. Todos los casos terminan sin shaders pendientes o fallidos ni recursos cercanos incompletos.
- Lluvia, nieve y niebla de un programa propio, incluyendo transición, pausa, cámara que viaja y vuelve y cambios de origen. El arranque preparado omite la parada actual para no esperar su horario; no modifica la actividad instalada. La física y las vistas usan el mismo programa.
- Siete páginas del menú a 800×600, incluida selección de formación, estimaciones, editor meteorológico, prácticas, perfiles, biblioteca y reporte. Se verifican límites de ventana y controles visibles.
- Materiales individuales frente a instanciados con una luz, sin luna y con dos luces; imagen sintética de referencia dentro de las tolerancias existentes, sin regenerarla.
- Auditoría de 188 formaciones de la misma versión de Chiltern: 153 listas según recursos auditados. Una auditoría completa no certifica todos los sistemas del contenido.
- Respaldos, comparación de versiones, cola, retirada y recuperación con datos temporales. La recuperación conserva archivos del autor incluso si usan el nombre reservado del informe. Opciones meteorológicas corruptas se rechazan antes de importar partidas.

## Rendimiento medido

El tramo pareado usa el binario anterior de `4520fe2` y el actual, con la misma ruta, formación, cámara, fecha, semilla y GPU. Los hashes están registrados. Los shaders del anterior se conservan por separado. Son dos recorridos de 100 m, con perfil de 40 FPS en un compositor privado; no miden la frecuencia máxima que puede alcanzar la GPU.

El percentil 99 durante conducción fue 25 ms en ambos. RSS pasó de 1273 a 1335 MiB; VRAM residente por proceso, de 1096 a 1099 MiB. La entrega añade funciones y conserva más detalle, por lo que no se presenta como una reducción general de memoria. Los contadores separan carga inicial de conducción; los porcentajes de CPU abarcan toda la ejecución y la ocupación global de GPU también incluye el compositor.

La reauditoría explícita pasó de 8,42 a 5,09 s. Se compiló el auditor del commit de referencia, cuyo trabajador descartaba el caché al forzar otra revisión. El nuevo valida dependencias y reutiliza fuentes, formas y cabinas: cero análisis repetidos en la segunda pasada. La auditoría inicial pasó de 8,29 a 14,42 s, por la comprobación de huellas y dependencias; se realiza en el trabajador de fondo. Los seis cambios de ruta medidos tomaron 11,8–21,6 ms. Son mediciones de este equipo, no garantías para cualquier instalación.

## Repetir las capturas

Con Weston y una GPU disponible, desde la raíz del repositorio:

```sh
CARGO_BUILD_JOBS=2 ./check.sh
python3 scripts/check_player_expansion.py --out-dir tmp/player-expansion-qa
python3 scripts/check_player_expansion.py --skip-lab \
  --route-root /ruta/al/contenido/ROUTES/Chiltern \
  --out-dir tmp/player-expansion-native
python3 scripts/capture_player_menu.py --viewer target/debug/openrailsrs-viewer3d \
  --pages practice weather-editor vehicle-profile library-tools bug-report train weather \
  --width 800 --height 600 --out-dir tmp/player-expansion-menu
env CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_OPT_LEVEL=1 CARGO_PROFILE_DEV_DEBUG=0 \
  CARGO_PROFILE_DEV_STRIP=symbols CARGO_INCREMENTAL=0 \
  cargo build --locked --workspace --all-features --example audit_cache
target/debug/examples/audit_cache /ruta/al/contenido/TRAINS/CONSISTS
```

`check_player_expansion.py` admite `--cases` para repetir sólo las vistas elegidas y `--baseline`/`--baseline-resources` para el recorrido comparativo. El ejemplo `player_lab` y su terreno son sintéticos: sus colores sirven para localizar caras y animaciones y no representan un tren ni un paisaje real. El reporte prepara archivos locales y captura sólo la ventana del juego; el usuario revisa y decide si los comparte.

## Alcance visual

Estas imágenes comprueban nuestra implementación y sus regresiones. No son una comparación universal de píxeles con Open Rails. La selección conservadora de LOD conserva geometría completa cuando un nivel simplificado pierde superficies o materiales. Sombras de contacto sólo alcanzan superficies visibles en profundidad; no se activan en cabina. Ondas y corrientes son visuales y conservan la elevación del plano de agua.
