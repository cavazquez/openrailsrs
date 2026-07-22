# Cab fixtures (CI-safe)

Redistributable `.eng` snippets for cab/viewpoint unit tests. These always run in CI.

| File | Purpose |
|------|---------|
| `orts3d_viewpoints.eng` | `ORTS3DCab` + `RotationLimit` + `StartDirection` + alternate rear viewpoint + `HeadOut` |

Content-heavy Pullman `.s` / ACE tests live in `openrailsrs-viewer3d` and are marked:

```text
#[ignore = "requires OPENRAILSRS_MSTS_CONTENT with Chiltern RF_Blue_Pullman …"]
```

Run them locally with:

```bash
export OPENRAILSRS_MSTS_CONTENT="/path/to/Open Rails/Content"
cargo test -p openrailsrs-viewer3d --lib -- --ignored
```
