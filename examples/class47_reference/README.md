# Class 47 y seis Mk2 de Demo Model 1

Ensayo físico de 250 segundos contra las DLL originales de Open Rails 1.6.1.
Incluye liberación, aceleración al 75 %, deriva, frenado al 60 %, nuevo arranque
al 50 % y frenado de servicio completo. El escenario numérico no es una ruta
jugable con gráficos. Para conducir la formación original, instalá Demo Model 1
desde Biblioteca y elegí su actividad Edinburgh Waverley → Linlithgow.

La formación original pesa 320674,64 kg y mide 141,088 m. Los parámetros de
`physics/` se extraen de ENG/WAG e Includes originales con
`scripts/prepare_formation_physics.py`; el manifiesto registra los hashes de cada
fuente. Los valores finales sobrescriben los anteriores, y los bloques parciales
de alimentación eléctrica se conservan. No contiene texturas, modelos ni sonidos.

```bash
python3 scripts/run_oracles.py --suite class47 --out-dir tmp/class47-oracle
```

Referencia y controles: `examples/baselines/class47`. Límites fijados antes del
ensayo: RMS 0,75 m/s, pico 2 m/s, diferencia máxima de odómetro 45 m. La versión
inicial pasa con RMS 0,2382 m/s, pico 1,9415 m/s y odómetro 25,79 m. No certifica
el servicio completo, todos los sistemas de la Class 47 ni otras formaciones.

Se usan los cilindros originales, su distribuidor UIC y la válvula relé; las
presiones reales alimentan el HUD y las agujas de cabina. El mínimo de 450 RPM
mantiene la alimentación de los coches con el arranque caliente. Emergencia,
equipamiento de servicio rápido, protección de deslizamiento y el ciclo completo
de depósitos neumáticos todavía requieren comparaciones adicionales.
