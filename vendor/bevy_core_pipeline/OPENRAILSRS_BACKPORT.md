# OIT pipeline reuse

Base: published `bevy_core_pipeline 0.19.1`, upstream Bevy commit
`b56fc29d3016e641754765244b5ba3f9cc504671`. Original license files are retained.
Only `src/oit/resolve/mod.rs` changes executable code.

The upstream system remembered only the last key/ID per camera entity. Returning
to an earlier configuration or replacing a camera queued the same descriptor
again, while PipelineCache retained its previous pipelines. The local system
caches by target format, maximum sorted-fragment count and depth-prepass state:
the complete varying inputs to the existing descriptor specialization.

Three CPU regressions cover configuration round trips, camera replacement and
separation of every key parameter. They exercise the production cache helper;
rendered camera/route transitions and process memory have separate measurements.
The public PipelineCache API, shader handles, layouts and shader code are unchanged.
