# Laboratorio de cabina y carteles

Ejemplo sintético propio, con un motor y un coche. Sus formas sencillas sirven para comprobar animaciones y controles; no representan una locomotora real. Los recursos usan la licencia del repositorio. No incluye contenido descargado.

Elegí **Laboratorio de cabina y carteles** en Nueva partida, o ejecutá:

```sh
target/debug/openrailsrs-viewer3d examples/player_lab/scenario.toml --live
```

En exterior, acercate al frente para leer el cartel. En **F9**, accioná ventanas izquierda/derecha, puerta de cabina y lavaparabrisas. Las ventanas bajan y la puerta se desliza en 1,5 segundos. El depósito tiene 2 L y consume 0,02 L/s mientras haya baja tensión. Desconectar la batería detiene la bomba; no cambia el clima.

La formación usa dinámica por vehículo. El tope delantero del segundo coche dispone de 100 mm de recorrido; sigue la compresión real del enganche, después de su juego libre. Un tope sin compresión permanece en reposo. El cuerpo conserva sus dimensiones.

Para guardar y continuar, usá Pausa → Guardar. Las reservas y posiciones de ventanas/puerta continúan desde el mismo estado.
