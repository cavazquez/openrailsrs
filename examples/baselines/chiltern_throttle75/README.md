# Baseline Chiltern — throttle 75 % (60 s)

Experimento **C** de calibración (OR-P1): equilibrio entre tracción y resistencia a notch fijo, complemento del Experimento E (50 %).

> OR: multi-cuerpo; openrailsrs default: masa puntual. Revisión con `multi_body` **opcional** (prioridad baja en régimen estable). [`docs/OR_PARITY_ROADMAP.md`](../../../docs/OR_PARITY_ROADMAP.md)

## Captura en Open Rails (Wine)

Ver [`../../chiltern/README.md`](../../chiltern/README.md) y el script:

```bash
./scripts/capture_chiltern_throttle75_or.sh
```

### En cabina (Explorer)

1. Pausa (`P`) al cargar.
2. Reverser adelante (`W`) si hace falta.
3. Freno de tren suelto: `;` repetido hasta BRAKEPRESSURE `-001` (o `Shift+/` parado).
4. Throttle **75 %**: `D` hasta THROTTLEPERC ~**075** (Pullman 8 notches → notch 6).
5. Despausa y dejá correr **60 s** simulados.
6. Salí de OR y instalá:

```bash
./scripts/install_chiltern_throttle75_baseline.sh
```

## Simulación openrailsrs

```bash
cd examples/chiltern
openrailsrs sim scenario_throttle75.toml --driver driver_throttle75.csv
openrailsrs compare-or ../baselines/chiltern_throttle75/or_evaluation_speed.csv run_throttle75.csv --phase-bounds 0,20,60
cargo test -p openrailsrs-cli --test chiltern_throttle75
```

## Qué mirar

| Fase | Objetivo |
|------|----------|
| 0–20 s | Run-up RPM + aceleración vs OR |
| 20–60 s | Velocidad ~constante; RMS ≤ 0.5 m/s vs OR |

El instalador **rechaza** CSV con THROTTLEPERC promedio fuera de 70–80 % en régimen.
