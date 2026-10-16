# Observación ambiental Chiltern: adhesión original

Las DLL originales de Open Rails 1.6.1 repitieron el servicio histórico con
un cliente temporal que añade exclusivamente lecturas de campos. La traza de
20 columnas y 15.003 filas conserva exactamente el SHA256
`ad9e690435d784df243c8a49bfdb37137c6e5f98f8941045adfde27e4500a7e2`.
`trace-integrity.json` comprueba también cada cadena por columna/fila. La
referencia existente no se reemplazó ni se cambiaron sus límites.

Se observó el estado antes del primer `Simulator.Update` y tras los 15.002
pasos de 50 ms. En ambas locomotoras, índices 0 y 7, los buffers iniciales son
`x=[0,0]`, `y=[0,0]`, con factor base cero. No se llamó ni se supuso
`Reset(0,5)`: ese reset corresponde a otra inicialización del original.
`initial-filter-context.json` conserva buffers, coeficientes y hardware reales.

El filtro original es Butterworth de orden 1 y 1 Hz. El primer paso publica
`x[0]=0,5452486`, `y[0]=0,07455116` en cabeza y `x[0]=0,563465059`,
`y[0]=0,0770418644` en cola. El observador lee `x[0]` directamente después de
la llamada original al filtro; no reconstruye una entrada a partir de `y`.

El archivo de ejecución está en
`examples/chiltern_local/physics/native-adhesion-inputs.csv`, SHA256
`355be676ad0d64120aa66014efd3f31f18d5848e863451d61ecc01d5570155a7`.
Sus 30.006 filas llevan ocho columnas:
`time_s,vehicle,input_factor,base_factor,filter_x0,filter_x1,filter_y0,filter_y1`.
Sólo el instante cero contiene buffers. Todas las columnas de estado del
filtro posteriores están vacías; `input_factor` y `base_factor` recuperan
exactamente los bits `Single` observados.

La ejecución separa la variación realizada con
`Single(input_factor - base_factor)` y la suma al factor base calculado desde
su propia posición y condiciones. El factor base observado se conserva para
esa resta y su validación; no impone a Rust el instante de paso por la zona
resbaladiza nativa. Es una realización ambiental concreta, sin afirmar un
puerto universal del generador aleatorio del simulador.

`filter-oracle.json` conserva todos los valores originales `y[0]` exclusivamente
para una comparación aislada del filtro, con límite absoluto previo de 1e-6.
El runtime no lee ese oráculo. La captura rica añade diagnósticos de ruedas,
patinaje, esfuerzo y freno para investigación; ninguno alimenta el recorrido.
Su gzip es sin pérdida y `publication.json` distingue el hash comprimido del
JSON original, SHA256
`3392f303463a7d54b48cda1bfd8bd260855ffcc952fb77747e9bf3eea71a1103`.

Los recibos y fuentes capturados son literales, incluidos sus hashes y rutas
del equipo. Los cuatro bloques marcados del cliente se pueden retirar para
recuperar el helper histórico. OracleWine confirmó gráficos nulos, diálogo de
fallo deshabilitado y cierre sin procesos propios ni drivers de escritorio o
audio. No se empaquetan originales de contenido, DLL, prefijo, ruta completa
ni las seis trazas humanas. Esta publicación registra captura y extracción;
no afirma una ejecución Rust ni un pase del gate completo.
