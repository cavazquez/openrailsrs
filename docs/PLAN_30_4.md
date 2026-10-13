# Cuarta entrega de 30 mejoras

Seleccionada el 12 de octubre de 2026. Referencia: Open Rails 1.6.1; se conservan Bevy 0.19.1 y Hanabi 0.19.0. Los criterios originales de cada issue se conservan. Un issue se cierra sólo cuando su alcance está implementado y comprobado.

## Alcance

El lote profundiza los equipos de los trenes, su alimentación y frenado, cabinas y puertas, y la interacción con el clima. Incluye iluminación de estaciones y efectos que ayudan a conducir. Los cuatro issues de audio de la entrega anterior siguen separados y abiertos hasta la escucha manual.

## Seguimiento

- [x] [#226](https://github.com/cavazquez/openrailsrs/issues/226) — Gestionar varios motores dentro de un mismo vehículo
- [x] [#227](https://github.com/cavazquez/openrailsrs/issues/227) — Completar filtros de tensión y protecciones eléctricas originales
- [x] [#228](https://github.com/cavazquez/openrailsrs/issues/228) — Gestionar varios pantógrafos independientes por locomotora
- [x] [#229](https://github.com/cavazquez/openrailsrs/issues/229) — Transportar alimentación entre vehículos mediante conexiones
- [x] [#230](https://github.com/cavazquez/openrailsrs/issues/230) — Incorporar baterías de tracción y propulsión dual
- [x] [#232](https://github.com/cavazquez/openrailsrs/issues/232) — Calcular peralte con límites locales, categoría y sentido
- [x] [#233](https://github.com/cavazquez/openrailsrs/issues/233) — Interpretar y validar tablas antiguas de peralte
- [x] [#235](https://github.com/cavazquez/openrailsrs/issues/235) — Simular temperatura, hielo, congelamiento y deshielo
- [x] [#249](https://github.com/cavazquez/openrailsrs/issues/249) — Implementar inclinación activa en material compatible
- [x] [#257](https://github.com/cavazquez/openrailsrs/issues/257) — Controlar puertas por lado y vehículo compatible
- [x] [#259](https://github.com/cavazquez/openrailsrs/issues/259) — Ampliar cambio entre cabinas y locomotoras de una formación
- [x] [#260](https://github.com/cavazquez/openrailsrs/issues/260) — Asociar cada cabina al vehículo que realmente comanda
- [x] [#272](https://github.com/cavazquez/openrailsrs/issues/272) — Recorrer interiores de pasajeros con límites de cámara
- [x] [#281](https://github.com/cavazquez/openrailsrs/issues/281) — Iluminar andenes y accesos con luminarias de estaciones
- [x] [#286](https://github.com/cavazquez/openrailsrs/issues/286) — Refinar protección meteorológica bajo techos y túneles
- [x] [#299](https://github.com/cavazquez/openrailsrs/issues/299) — Aplicar viento lateral a dinámica de trenes sensibles
- [x] [#300](https://github.com/cavazquez/openrailsrs/issues/300) — Crear bancos de niebla móviles y locales
- [x] [#302](https://github.com/cavazquez/openrailsrs/issues/302) — Incorporar sombras de nubes sobre el escenario
- [x] [#303](https://github.com/cavazquez/openrailsrs/issues/303) — Mejorar gotas del parabrisas con unión y escurrimiento
- [x] [#317](https://github.com/cavazquez/openrailsrs/issues/317) — Completar freno reostático según velocidad
- [x] [#318](https://github.com/cavazquez/openrailsrs/issues/318) — Modelar regeneración y receptividad de la red
- [x] [#319](https://github.com/cavazquez/openrailsrs/issues/319) — Incorporar calentamiento y pérdida de eficacia de frenos
- [x] [#320](https://github.com/cavazquez/openrailsrs/issues/320) — Implementar protección contra deslizamiento de frenado
- [x] [#321](https://github.com/cavazquez/openrailsrs/issues/321) — Profundizar control de tracción y recuperación de adherencia
- [x] [#327](https://github.com/cavazquez/openrailsrs/issues/327) — Agregar circuitos de aceite y refrigeración diésel
- [x] [#328](https://github.com/cavazquez/openrailsrs/issues/328) — Incorporar fallos opcionales de motor con causas físicas
- [x] [#330](https://github.com/cavazquez/openrailsrs/issues/330) — Diferenciar inyectores y equipos auxiliares de vapor
- [x] [#331](https://github.com/cavazquez/openrailsrs/issues/331) — Relacionar consumo de compresores y eyectores con trabajo real
- [x] [#332](https://github.com/cavazquez/openrailsrs/issues/332) — Ampliar mando múltiple entre familias de locomotoras
- [x] [#334](https://github.com/cavazquez/openrailsrs/issues/334) — Incorporar mercancías con carga y velocidades propias

## Aceptación

Implementación y verificaciones locales completas. [Evidencias por grupo](PLAYER_30_4_QA.md) y [prueba manual de cada ítem](PLAYER_30_4_MANUAL.md). Pasaron check.sh, seis oráculos anteriores, diecinueve capturas de GPU, el contrato de rendimiento y la regresión visual sin cambiar límites. El cierre en GitHub se publica después de comprobar la CI del commit.
