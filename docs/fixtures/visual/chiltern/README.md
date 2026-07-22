# Chiltern Birmingham visual goldens (#71)

Baseline **openrailsrs** PNGs for reproducible exterior + cabina captures.

| File | View |
|------|------|
| `birmingham_exterior.png` | Orbit near Birmingham (tile ≈ −6080 / 14925) |
| `birmingham_cabina.png` | `OPENRAILSRS_FOLLOW=driver` first-person |

## Regenerate

```bash
export OPENRAILSRS_MSTS_CONTENT="$HOME/Documentos/Open Rails/Content"
UPDATE_GOLDEN=1 ./scripts/visual_regression_chiltern.sh
```

Compare without updating:

```bash
./scripts/visual_regression_chiltern.sh
```

## Open Rails side-by-side (optional)

Place OR screenshots as:

- `docs/fixtures/visual/or_reference/desdeafuera.png`
- `docs/fixtures/visual/or_reference/cabina.png`

These are **not** CI goldens; use for manual parity. Automated fail/pass uses the openrailsrs baselines above + `openrailsrs-visual-diff`.

## Injection tests

Unit tests in `visual_diff_core` prove the diff fails when a synthetic train blob is scaled ×1.5 or shifted down (sink stand-in). No Chiltern content required in CI.
