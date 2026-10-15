# Bindless allocation accounting

Base: published `bevy_pbr 0.19.1`. Original license files are retained. Only
`src/material_bind_groups.rs` changes executable code.

Planning originally counted repeated, initially absent samplers/texture views
more than once within a material. Insertion used that stale plan to increase
the resource count twice, although the binding array held one entry with two
references. The final free removed one entry and decremented the count once.
The ghost count affected admission and could allocate unnecessary slabs.

Planning now deduplicates by binding-array type and resource ID. Generic array
insertion reports whether the entry was new, and the counter follows that result.
Per-binding buffer arrays and the original oversized-material exception remain.

Five CPU regressions cover shared samplers/textures, typed arrays, final owner
release, successive materials and 128 allocation/free cycles at capacity. They
use the production generic array with simulated resources, without a GPU.
Layouts, resource reference counts and shader specialization remain unchanged.
Process RSS is evaluated independently of these slot counts.
