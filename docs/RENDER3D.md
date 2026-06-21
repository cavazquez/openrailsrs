# openrailsrs-render3d

App Bevy para **validación visual** contra Open Rails: terreno por tiles, spawn WORLD, shaders OR WGSL y VSM (modos `pcf+or` / `approx` / `exact`).

Relacionado: [`BEVY_ARCHITECTURE.md`](BEVY_ARCHITECTURE.md) · [`openrailsrs-viewer3d`](../crates/openrailsrs-viewer3d/) (app jugable)

---

## Diferencia vs viewer3d

| | **render3d** | **viewer3d** |
|---|-------------|--------------|
| Objetivo | Paridad visual OR, tile stream | Simulación jugable (`--live`, cabina) |
| Plugin | `Render3dPlugin` | `ViewerPlugin` |
| VSM | Completo (Exact) | Opcional / futuro |
| Loading | `AppState::Loading` → `Playing` | Startup progresivo |

---

## Uso

```bash
cargo run -p openrailsrs-render3d -- \
  --route "$CHILTERN_ROUTE" \
  --tile-x -6084 --tile-z 14923 --radius 2
```

Scripts: `./scripts/run_render3d_*.sh` (si existen en el repo).

Controles: WASD/QE · botón derecho + mouse · F3 HUD · F4–F8 VSM · F9 preset debug · Esc salir.

---

## Crates compartidos

- **`openrailsrs-bevy-scenery`**: materiales OR, shaders, VSM pass, assets en `assets/shaders/`
- **`openrailsrs-or-shader`**: clasificación shaders MSTS, coordenadas (sin GPU)

Assets: `openrailsrs_bevy_scenery::shared_asset_plugin()` — no hace falta copiar shaders por app.

---

## Variables de entorno VSM

| Variable | Valores | Default |
|----------|---------|---------|
| `OPENRAILSRS_OR_VSM` | `pcf+or`, `approx`, `exact` | `pcf+or` |
| `OPENRAILSRS_OR_SHADERS` | `0` / `1` | `1` |

---

## Bevy 0.19

Ver [`BEVY_MIGRATION_0_19.md`](BEVY_MIGRATION_0_19.md). El pass VSM usa `Core3d` systems (no RenderGraph).
