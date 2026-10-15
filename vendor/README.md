# Bevy 0.19.1 lifecycle fixes

These two crates keep the published Bevy version and manifests. They are selected
through the root `[patch.crates-io]` section and excluded from workspace feature
selection. Every other locked dependency keeps its previous selection.

* `bevy_core_pipeline`: cache OIT resolve pipelines by their complete descriptor
  key, so replacing cameras and returning to an earlier depth-prepass setting
  reuses the existing pipeline.
* `bevy_pbr`: deduplicate new shared samplers/texture views when planning a
  bindless allocation; count actual insertions so freeing the last reference
  restores the resource count and slab admission capacity.

Each directory includes the upstream `LICENSE-MIT` and `LICENSE-APACHE` and a
short `OPENRAILSRS_BACKPORT.md`. Upstream shaders, bindings and rendering
parameters are retained. These fixes address two confirmed accumulation paths;
process memory and route transitions are measured separately.

The focused CPU regressions can be run from the project root:

```sh
cargo test --offline --locked -p bevy_pbr -p bevy_core_pipeline --lib regressions -- --test-threads=1
```

The filter selects eight tests and does not create a graphics adapter. Cargo may
still need to compile the library test executables and their dependencies.
