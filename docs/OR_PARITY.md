# Paridad física con Open Rails

Complementa [`ROADMAP.md`](../ROADMAP.md). Baselines: `examples/baselines/` · escenarios: `examples/chiltern/`.

## Modelo (importante)

| | Open Rails | openrailsrs default | `multi_body = true` |
|---|---|---|---|
| Dinámica | Multi-coche + acopladores | **Masa puntual** | Masas + acopladores |
| Davis | Por coche | Agregado | Por vehículo |
| Baselines CSV OR | Multi-cuerpo | Comparación mixta ⚠️ | Más comparable |

RMS publicados (Chiltern ~0.39 m/s, etc.) calibran a menudo **puntual vs OR multi-cuerpo**.

## Estado olas (resumen)

| Ola | Fases | Objetivo | Estado típico |
|-----|-------|----------|---------------|
| 1 | OR-P1…P3 | Diesel thr aparente, CN, run-up | ✅ / 🔶 |
| 2 | OR-P4…P6 | Multi-cuerpo, Davis/veh, frenos | 🔶 (multi_body estable Chiltern) |
| 3 | OR-P7…P8 | Señales + driver sin assume-clear | 🔶 |
| 4 | OR-P9+ | Gearbox, dinámico, vapor | 🔲 / parcial |

Detalle de cada OR-P*: historial en commits anteriores del repo y código en `openrailsrs-sim`.

## Calibración rápida

```bash
cargo run -p openrailsrs-cli -- sim examples/chiltern/scenario.toml
# Comparar: openrailsrs compare-or …  → OR_TRACE_COMPARISON.md
```

| Escenario | Notas |
|-----------|--------|
| Birmingham ~136 s | `assume_signals_clear`; RMS v ~0.39 |
| `scenario_multi_body.toml` | Acopladores; `time_step=1.0` |
| SCE Glasgow | Umbral ≤1 m/s |

Referencias OR: `MSTSDieselLocomotive.cs`, `DieselEngine.cs`, `TrainCar.cs`. Audit ENG/WAG: [`FORMATS.md`](FORMATS.md).
