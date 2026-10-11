// Local Bevy lights on the legacy SDR scenery materials. Keep the authored OR
// sun shading, but let actual spotlights illuminate terrain, rails and buildings.
#import bevy_pbr::{
    mesh_view_bindings as railway_bindings,
    clustered_forward,
    mesh_view_types::{POINT_LIGHT_FLAGS_SPOT_LIGHT_Y_NEGATIVE, POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT},
    shadows::fetch_spot_shadow,
    lighting::getDistanceAttenuation,
}

// Bevy sorts directional lights by shadow/volumetric state and entity, not by
// illuminance. A zero-lux moon can therefore precede the sun. Legacy materials
// use the strongest irradiance for their authored half-Lambert shading and must
// use that same index when sampling its shadow cascades.
fn railway_primary_directional_light() -> u32 {
    var primary = 0u;
    var strongest = -1.0;
    for (var index = 0u; index < railway_bindings::lights.n_directional_lights; index += 1u) {
        let color = railway_bindings::lights.directional_lights[index].color.rgb;
        let strength = max(color.r, max(color.g, color.b));
        if strength > strongest {
            strongest = strength;
            primary = index;
        }
    }
    return primary;
}

fn railway_daylight() -> f32 {
    if railway_bindings::lights.n_directional_lights == 0u { return 0.025; }
    let light = railway_bindings::lights.directional_lights[railway_primary_directional_light()];
    let strength = max(light.color.r, max(light.color.g, light.color.b));
    // The lunar light is below one lux. Its altitude must not turn the legacy
    // SDR day multiplier on; physical lunar contributions use actual lux below.
    let solar_energy = clamp(strength / 1000.0, 0.0, 1.0);
    return 0.025 + 0.975 * solar_energy * clamp(light.direction_to_light.y * 2.0, 0.0, 1.0);
}

fn railway_lunar_lighting(normal: vec3<f32>) -> vec3<f32> {
    var sum = vec3(0.0);
    for (var index = 0u; index < railway_bindings::lights.n_directional_lights; index += 1u) {
        let light = railway_bindings::lights.directional_lights[index];
        let strength = max(light.color.r, max(light.color.g, light.color.b));
        if strength <= 1.0 {
            sum += light.color.rgb * max(dot(normal, light.direction_to_light), 0.0)
                * railway_bindings::view.exposure / 3.14159265;
        }
    }
    return sum;
}

fn railway_spot_lighting(world: vec4<f32>, normal: vec3<f32>, fragment_xy: vec2<f32>) -> vec3<f32> {
    if (railway_bindings::lights.cluster_dimensions.w == 0u) { return vec3(0.0); }
    let view_z = (railway_bindings::view.view_from_world * world).z;
    let cluster = clustered_forward::view_fragment_cluster_index(fragment_xy, view_z, false);
    let ranges = clustered_forward::unpack_clusterable_object_index_ranges(cluster);
    var sum = vec3(0.0);
    for (var index = ranges.first_spot_light_index_offset; index < ranges.first_reflection_probe_index_offset; index += 1u) {
        let id = clustered_forward::get_clusterable_object_id(index);
        let light = railway_bindings::clustered_lights.data[id];
        let to_light = light.position_radius.xyz - world.xyz;
        let distance_square = dot(to_light, to_light);
        let l = normalize(to_light);
        var direction = vec3(light.light_custom_data.x, 0.0, light.light_custom_data.y);
        direction.y = sqrt(max(0.0, 1.0 - dot(direction.xz, direction.xz)));
        if ((light.flags & POINT_LIGHT_FLAGS_SPOT_LIGHT_Y_NEGATIVE) != 0u) { direction.y = -direction.y; }
        let cone = clamp(dot(-direction, l) * light.light_custom_data.z + light.light_custom_data.w, 0.0, 1.0);
        var shadow = 1.0;
        if ((light.flags & POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {
            shadow = fetch_spot_shadow(id, world, normal, light.shadow_map_near_z, fragment_xy);
        }
        let irradiance = light.color_inverse_square_range.rgb * getDistanceAttenuation(distance_square, light.color_inverse_square_range.w)
            * cone * cone * max(dot(normal, l), 0.0) * shadow;
        sum += irradiance * railway_bindings::view.exposure / 3.14159265;
    }
    return sum;
}
