# Referencia original de la Class 47 — Open Rails 1.6.1

`capture/trace.csv` contiene 5001 muestras de las DLL originales, a 50 ms.
`driver.csv` exporta sus controles. El cliente `FormationCapture.cs` configura
la actividad original de Demo Model 1 y captura 250 segundos; no reemplaza ni
modifica las DLL. Los controles incluyen frenado parcial y de servicio completo;
la posición inicial se conserva después de estabilizar los frenos durante 20 s.

`manifest.json` registra la actividad, el cliente, los binarios y los archivos.
La referencia, sus controles y los límites están fijados en los oráculos. El
archivo fuente de OR sigue en el commit d16e670da333d26d2edfc97d5631a19dadf49ce5.

Para regenerar una captura aparte, conservando estos archivos:

```bash
python3 scripts/capture_formation_or.py --activity "$DEMO_ACTIVITY" \
  --wine-prefix tmp/class47-private-prefix --out-dir tmp/class47-new-reference
```

Hace falta una instalación original de OR 1.6.1, un prefix Wine ya preparado y
Demo Model 1 descargado de su distribuidor original. Nada de ese contenido se
incluye en esta carpeta. El ensayo no es un servicio completo ni una certificación
de todos los sistemas de esta locomotora.
