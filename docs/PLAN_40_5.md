# Quinto lote: cuarenta issues

Este lote conserva Open Rails 1.6.1 como referencia y Bevy 0.19.1 con Hanabi 0.19. Las pruebas distinguen compatibilidad original, extensiones propias y resultados medidos. Los recursos descargados del autor se guardan en los datos del jugador, fuera de Git y del paquete Snap.

Los comentarios de cada issue registran implementación, fallos encontrados y aceptación. Un ensayo de render o de frenos no certifica todos los sistemas de un tren. La evidencia y los límites de cada área se describen en las guías enlazadas.

## Tren, cabina y entorno

Pruebas manuales y alcance: [PLAN_40_5_VISUAL_QA.md](PLAN_40_5_VISUAL_QA.md).

- [#243: Animar fuelles acompañando curvas y uniones entre coches](https://github.com/cavazquez/openrailsrs/issues/243) — alta.
- [#244: Animar mangueras y cables flexibles entre vehículos](https://github.com/cavazquez/openrailsrs/issues/244) — alta.
- [#245: Afinar coincidencia entre posición física y visual de acopladores](https://github.com/cavazquez/openrailsrs/issues/245) — alta.
- [#253: Mejorar trenes articulados con bogies compartidos](https://github.com/cavazquez/openrailsrs/issues/253) — alta.
- [#254: Representar pasajeros entrando y saliendo por puertas habilitadas](https://github.com/cavazquez/openrailsrs/issues/254) — alta.
- [#265: Agregar espejos funcionales para formación y andén](https://github.com/cavazquez/openrailsrs/issues/265) — alta.
- [#273: Mantener horizonte convincente con terreno cargado por sectores](https://github.com/cavazquez/openrailsrs/issues/273) — alta.
- [#276: Afinar sombras de trenes, edificios y vegetación](https://github.com/cavazquez/openrailsrs/issues/276) — alta.
- [#277: Mejorar cristales y transparencias superpuestas](https://github.com/cavazquez/openrailsrs/issues/277) — alta.
- [#278: Agregar reflejos de lluvia en rieles y superficies compatibles](https://github.com/cavazquez/openrailsrs/issues/278) — alta.
- [#282: Implementar pasos a nivel operativos](https://github.com/cavazquez/openrailsrs/issues/282) — alta.
- [#283: Coordinar tránsito vial y colas en pasos a nivel](https://github.com/cavazquez/openrailsrs/issues/283) — alta.
- [#284: Agregar peatones en estaciones y espacios habilitados](https://github.com/cavazquez/openrailsrs/issues/284) — alta.
- [#285: Animar instalaciones industriales y equipos de carga](https://github.com/cavazquez/openrailsrs/issues/285) — alta.
- [#291: Evitar que cámara exterior atraviese objetos sólidos](https://github.com/cavazquez/openrailsrs/issues/291) — alta.
- [#296: Dar espesor visual a nieve acumulada](https://github.com/cavazquez/openrailsrs/issues/296) — alta.
- [#297: Mostrar huellas y despeje de nieve por vehículos](https://github.com/cavazquez/openrailsrs/issues/297) — alta.

## Operaciones, tráfico y editor

Pruebas manuales y alcance: [TRAIN_OPERATIONS_FIFTH40.md](TRAIN_OPERATIONS_FIFTH40.md).

- [#333: Cargar más tráfico original durante actividades completas](https://github.com/cavazquez/openrailsrs/issues/333) — alta.
- [#335: Programar adelantamientos entre locales y expresos](https://github.com/cavazquez/openrailsrs/issues/335) — alta.
- [#339: Planificar encuentros en vía única y apartaderos](https://github.com/cavazquez/openrailsrs/issues/339) — alta.
- [#340: Profundizar autorizaciones de maniobra con límites](https://github.com/cavazquez/openrailsrs/issues/340) — alta.
- [#341: Permitir tomar control de otra locomotora o servicio](https://github.com/cavazquez/openrailsrs/issues/341) — alta.
- [#342: Acoplar manualmente formaciones independientes](https://github.com/cavazquez/openrailsrs/issues/342) — alta.
- [#343: Clasificar vagones por destino en patios](https://github.com/cavazquez/openrailsrs/issues/343) — alta.
- [#347: Agregar preparación y mantenimiento en depósitos](https://github.com/cavazquez/openrailsrs/issues/347) — alta.
- [#350: Conducir jornadas de varios servicios con el mismo material](https://github.com/cavazquez/openrailsrs/issues/350) — alta.
- [#396: Crear editor gráfico de itinerarios, inversiones y esperas](https://github.com/cavazquez/openrailsrs/issues/396) — alta.

## Contenido y distribución

Pruebas manuales y alcance: [OFFICIAL_CONTENT.md](OFFICIAL_CONTENT.md).

- [#373: Elegir carpeta de contenido desde la interfaz](https://github.com/cavazquez/openrailsrs/issues/373) — media.
- [#412: Chiltern extendido no resuelve Blue Pullman y recomienda copiar recursos dentro de /snap](https://github.com/cavazquez/openrailsrs/issues/412) — media.
- [#413: instalar rutas y trenes pegando una URL del origen original](https://github.com/cavazquez/openrailsrs/issues/413) — media.

## Rendimiento y estabilidad

Pruebas manuales y alcance: [PERFORMANCE_FIFTH40.md](PERFORMANCE_FIFTH40.md).

- [#240: Reducir tirones restantes al preparar objetos grandes](https://github.com/cavazquez/openrailsrs/issues/240) — alta.
- [#314: Reducir carga y memoria de bancos de audio grandes](https://github.com/cavazquez/openrailsrs/issues/314) — alta.
- [#380: Precargar recorrido según velocidad e itinerario](https://github.com/cavazquez/openrailsrs/issues/380) — alta.
- [#381: Priorizar sectores visibles y próximos al tren](https://github.com/cavazquez/openrailsrs/issues/381) — alta.
- [#383: Guardar geometría preparada en caché persistente](https://github.com/cavazquez/openrailsrs/issues/383) — alta.
- [#384: Reutilizar importaciones y señales mediante caché persistente](https://github.com/cavazquez/openrailsrs/issues/384) — alta.
- [#390: Compartir presupuesto de sombras, vegetación, luces y partículas](https://github.com/cavazquez/openrailsrs/issues/390) — alta.
- [#391: Gestionar presión de VRAM y liberación de recursos](https://github.com/cavazquez/openrailsrs/issues/391) — alta.
- [#393: Paralelizar tráfico conservando reproducibilidad](https://github.com/cavazquez/openrailsrs/issues/393) — alta.
- [#403: Ejecutar pruebas prolongadas de fugas y acumulación](https://github.com/cavazquez/openrailsrs/issues/403) — media.


## Reproducción

`./check.sh` ejecuta formato, clippy, pruebas, empaquetado y los oráculos fijados. Conserva los CSV/JSON existentes del jugador aunque una prueba falle. Las capturas de aceptación usan una sesión gráfica privada, datos de jugador aislados y el hash del binario. Los ensayos largos usan tiempo real de ejecución y distinguen una prueba interrumpida de una prueba terminada.

Para reproducir el cambio de conductor y la recogida de una formación con sus modelos:

```bash
python3 scripts/check_traffic_operations_gpu.py --viewer target/debug/openrailsrs-viewer3d --out-dir tmp/traffic-operations-qa
```

La prueba requiere un binario con `dev-tools` y una GPU accesible al compositor privado. El caso de cambio de conductor valida los trenes en un ramal que queda fuera del pequeño WORLD del laboratorio; no afirma aceptación del escenario de esa zona. El caso de acople valida cuatro modelos y el guardado inmediato de sus equipos.

## Revisión de seguridad

La revisión del lote encontró dos fallos en entradas externas. El lector ACE ahora limita los bytes del archivo, su expansión ZLIB y los píxeles retenidos antes de reservar memoria. El horizonte usa un límite de 32 MiB; las texturas que lo exceden conservan el color de respaldo. Las pruebas incluyen una cabecera con tamaño falso, un DXT pequeño que declara una imagen enorme y texturas originales válidas.

Los repositorios agregados por URL usan un identificador SHA-256 del origen canónico en un espacio separado del catálogo. La cola no puede sustituir un origen por otro con un nombre parecido. Las pruebas cubren colisiones entre cuenta/repositorio, nombres del catálogo, variantes de mayúsculas y una sustitución maliciosa de un trabajo pendiente.
