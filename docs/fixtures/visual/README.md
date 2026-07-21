# Visual regression fixtures (#43)

Golden PNG for the deterministic smoke orbit capture used by
[`scripts/visual_regression_smoke.sh`](../../../scripts/visual_regression_smoke.sh).

| File | Size | Role |
|------|------|------|
| `smoke_orbit.png` | 640×360 | Baseline orbit view of `examples/smoke/routes/test` |

## Thresholds

Compared by `openrailsrs-visual-diff` (bin of `openrailsrs-viewer3d`):

| Parameter | Default | Env override |
|-----------|---------|--------------|
| Per-channel ΔRGB “hot” | 16/255 | `OPENRAILSRS_VISUAL_TOL` |
| Max hot pixels | 2 % | `OPENRAILSRS_VISUAL_MAX_HOT_PCT` |

Same resolution is required. Optional red-highlight diff PNG is written next to the actual capture on failure.

This is **not** pixel-perfect: GPU/driver variance across CI runners is expected within the hot-pixel budget.

## Regenerate golden

```bash
UPDATE_GOLDEN=1 ./scripts/visual_regression_smoke.sh
```

Capture uses fixed camera / window env (see script): `OPENRAILSRS_CAM_*`,
`OPENRAILSRS_WINDOW_*`, `OPENRAILSRS_SCREENSHOT_AFTER_READY=1`.

Commit the updated `smoke_orbit.png` when scenery or lighting changes intentionally.
