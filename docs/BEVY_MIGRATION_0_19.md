# Migración Bevy 0.18 → 0.19

Checklist aplicada en junio 2026. Guía oficial: [bevy.org/learn/migration-guides/0-18-to-0-19](https://bevy.org/learn/migration-guides/0-18-to-0-19/).

Relacionado: [`BEVY_ARCHITECTURE.md`](BEVY_ARCHITECTURE.md) · [`RENDER3D.md`](RENDER3D.md)

---

## Pin de versión

```toml
# Cargo.toml raíz
[workspace.dependencies]
bevy = { version = "0.19", default-features = false, features = [...] }
```

Crates Bevy: `openrailsrs-or-shader` (solo math/color), `openrailsrs-bevy-scenery`, `openrailsrs-viewer3d`, `openrailsrs-render3d`.

---

## Cambios aplicados en este repo

| Área | Cambio | Archivos |
|------|--------|----------|
| **Render graph → systems** | `OrVsmMomentNode` → `or_vsm_moment_pass` en `Core3d` | `bevy-scenery/src/vsm/render.rs` |
| **TextFont / Parley** | `font_size: N` → `FontSize::Px(N)`; helper `text_px()` | `bevy-scenery/src/ui/text.rs`, HUD viewer/render3d |
| **Luces** | `shadows_enabled` → `shadow_maps_enabled` | `scene.rs`, `cab_view.rs`, `lighting.rs` |
| **AssetMut** | `let Some(mut x) = assets.get_mut(...)` | `precipitation.rs`, `signals.rs`, `scenery.rs` |
| **WGSL sombras** | `fetch_directional_shadow` requiere `frag_coord_xy` (5º arg: `in.position.xy`) | `or_scenery.wgsl`, `or_cab.wgsl`, `or_terrain.wgsl` |

---

## Orden de bump recomendado (futuro 0.20)

1. `[workspace.dependencies] bevy`
2. `cargo check -p openrailsrs-bevy-scenery` (VSM + materiales)
3. `cargo check -p openrailsrs-viewer3d -p openrailsrs-render3d`
4. `./check.sh`

---

## Features 0.19 adoptadas / pospuestas

| Feature | Decisión |
|---------|----------|
| Partial bindless | Automático al compilar |
| Parley / FontSize | Obligatorio — centralizado en `text_px` |
| Contact shadows | Evaluación opcional (PR separado) |
| BSN / App Settings | No usado |
| Render recovery | Habilitar si estable en producción |

---

## Rollback

```bash
git checkout HEAD~1 -- Cargo.toml crates/*/Cargo.toml
cargo update -p bevy
```

Verificar que `assets/shaders/` viven en `openrailsrs-bevy-scenery/assets/`.

---

## Errores conocidos pre-migración (~33 en monolito)

Con modularización previa, el bump tocó principalmente `bevy-scenery` (VSM) + fixes mecánicos en apps.
