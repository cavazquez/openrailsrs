//! Prepare physical cant/direction from the same cached native path as the rails.
//! Sampling happens once on load; the simulation never reads render transforms.
use bevy::prelude::*;
fn apply(session: &mut openrailsrs_sim::LiveDriveSession, assets: &crate::shapes::RouteAssets) {
    let edges = session
        .graph
        .edges_iter()
        .map(|(id, e)| (id.to_owned(), e.length_m))
        .collect::<Vec<_>>();
    for (id, length_m) in edges {
        let Some(native) = id.strip_prefix('e') else {
            continue;
        };
        let reverse = native.ends_with("_r");
        let Some(node) = native.trim_end_matches("_r").parse::<u32>().ok() else {
            continue;
        };
        let Some(path) = assets.banked_paths.get(&node) else {
            continue;
        };
        let native_length = path.length_m();
        let count = (length_m / 2.).ceil().clamp(1., 100000.) as usize;
        let mut profile = session
            .graph
            .physics_profile(&id)
            .cloned()
            .unwrap_or_default();
        let mut curves = Vec::with_capacity(count + 1);
        for i in 0..=count {
            let position = length_m * i as f64 / count as f64;
            let native_position = if reverse {
                native_length - position
            } else {
                position
            }
            .clamp(0., native_length);
            let pose = path.pose_in_frame(native_position, bevy::math::DVec3::ZERO);
            let lo = path
                .pose_in_frame((native_position - 0.25).max(0.), bevy::math::DVec3::ZERO)
                .position;
            let hi = path
                .pose_in_frame(
                    (native_position + 0.25).min(native_length),
                    bevy::math::DVec3::ZERO,
                )
                .position;
            let direction = (hi - lo).xz().normalize_or_zero() * if reverse { -1. } else { 1. };
            let curvature =
                path.signed_curvature_at(native_position) * if reverse { -1. } else { 1. };
            curves.push(openrailsrs_track::PositionCurve {
                position_m: position,
                curvature_m_inv: curvature,
                cant_rad: pose.roll_rad as f64 * if reverse { -1. } else { 1. },
                heading_rad: f64::from(direction.y.atan2(direction.x)),
            });
        }
        profile.curves = curves;
        session.graph.set_physics_profile(&id, profile);
    }
    session.path_data =
        openrailsrs_sim::path_data::PathData::from_path(&session.state.path_edges, &session.graph);
}
pub fn prepare(mut live: ResMut<crate::live::LiveDrive>, assets: Res<crate::shapes::RouteAssets>) {
    apply(&mut live.session, &assets);
    for service in &mut live.traffic.services {
        apply(&mut service.session, &assets);
    }
}
