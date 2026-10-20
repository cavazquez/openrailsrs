# Alfa.8: servicios locales, menú y actividades originales

El conductor automático mantiene el freno ante un objetivo de velocidad cero,
incluso por debajo de la banda de regulación de marcha. La tolerancia de llegada
sigue siendo 10 m y 0,10 m/s. Esto corrige el avance lento que omitía FAR ROCKAWAY
con el servicio AI RPVBT0 de LIRR.

Varias plataformas pueden compartir el nodo final del TDB con offsets distintos.
Sólo la última parada programada es terminal; las intermedias se conservan y el
orden por distancia sigue siendo estricto.

El menú adapta sus márgenes y su resumen a la ventana y a la escala de interfaz.
En una ventana de 800×600 al 150%, las listas conservan espacio visible y las
acciones de lanzamiento permanecen dentro del panel. La estimación de recursos
sigue disponible en Hora y clima. Cambiar el tamaño de ventana actualiza el diseño.

## Mitre original instalado por el usuario

Un servicio propio en `<datos de usuario>/custom-services/` puede declarar, antes
de las secciones TOML:

```toml
player_route_label = "Mitre · Retiro–Tigre · local"
player_native_root = "/ruta/a/la/instalacion/ROUTES/RET-TIG"
```

`player_route_label` identifica la ruta en el selector de país y ruta. Acepta hasta
100 caracteres sin controles. Las rutas relativas de `player_native_root` y
`route.path` se resuelven desde el archivo del servicio. La formación y los demás
recursos conservan las comprobaciones normales de disponibilidad. El selector
respeta los colectores eléctricos declarados por el servicio sólo para su
formación original; elegir otro tren conserva sus propios colectores y límites
de tensión.

La instalación local de Retiro–Tigre referencia el TDB, paisaje y Toshiba del
usuario, incluida la cabina CVF original que el autor vincula a SERIES2000. Esa
instalación no se copia al repositorio, al portable ni al Snap. Sin esos archivos,
las prácticas Mitre del paquete siguen indicando que falta el paisaje.

## Revisión de actividades

Las formaciones estáticas derivadas se escriben con marca UTF-8 para conservar
los nombres nativos con caracteres como `£`. Los originales no se modifican.
Las lámparas de cola acopladas conservan su tamaño original inferior a medio
metro, con masa, geometría y orden de formación originales. Las dimensiones
nulas, negativas o no finitas siguen rechazándose.

Los PAT que el autor marca como inválidos, los recorridos cuya continuidad no
puede resolverse y los recursos originales ausentes conservan su diagnóstico.
Preparar una actividad no certifica que se complete toda su misión o todos los
eventos ACT. La entrega local incluye la revisión individual de las 37 actividades
antes bloqueadas y los resultados de las comprobaciones.
