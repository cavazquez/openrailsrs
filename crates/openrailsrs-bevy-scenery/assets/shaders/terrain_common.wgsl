// Shared terrain lighting helpers (#121).
// TODO(#121): fold dual-tex / night / fog paths into one fragment once golden
// Birmingham patches match under both TerrainMaterial and OrTerrainMaterial.

fn terrain_half_lambert(normal: vec3<f32>, light_dir: vec3<f32>) -> f32 {
    return dot(normalize(normal), normalize(light_dir)) * 0.5 + 0.5;
}
