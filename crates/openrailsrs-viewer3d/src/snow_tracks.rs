//! Wheel-derived, sector-bounded snow clearance. Absolute coordinates survive rebasing.
use bevy::{camera::primitives::Aabb, prelude::*};
use std::collections::{BTreeMap, HashMap, VecDeque};
#[derive(Clone, Copy)]
struct Segment {
    a: Vec3,
    b: Vec3,
    width: f32,
}
#[derive(Resource, Default)]
pub struct SnowTracks {
    sectors: BTreeMap<(i32, i32), VecDeque<Segment>>,
    previous: HashMap<(Entity, u8), Vec3>,
    clock: Option<f64>,
    gpu_changes: u64,
    material_uploads: u64,
    pub gpu: openrailsrs_bevy_scenery::materials::SnowTrackData,
}
pub fn reset(mut tracks: ResMut<SnowTracks>) {
    *tracks = default();
}
fn sector(p: Vec3) -> (i32, i32) {
    ((p.x / 2048.).floor() as i32, (p.z / 2048.).floor() as i32)
}
impl SnowTracks {
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"sectors":self.sectors.len(),"segments":self.sectors.values().map(VecDeque::len).sum::<usize>(),"gpu_segments":self.gpu.meta.x,"maximum_depth_m":0.08,"gpu_changes":self.gpu_changes,"material_uploads":self.material_uploads})
    }
    fn stamp(&mut self, id: (Entity, u8), p: Vec3) {
        let previous = self.previous.insert(id, p);
        let Some(a) = previous else {
            return;
        };
        if p.distance(a) < 0.02 {
            return;
        }
        if p.distance(a) > 25. {
            return;
        } // reset, teleport or a newly restored formation
        let entries = self.sectors.entry(sector(p)).or_default();
        entries.push_back(Segment {
            a,
            b: p,
            width: 0.22,
        });
        while entries.len() > 256 {
            entries.pop_front();
        }
    }
    fn prepare(&mut self, camera: Vec3, origin: Vec3, maximum: usize) -> bool {
        let previous_gpu = self.gpu.clone();
        let center = sector(camera);
        self.sectors
            .retain(|(x, z), _| x.abs_diff(center.0) <= 2 && z.abs_diff(center.1) <= 2);
        let mut segments = self
            .sectors
            .values()
            .flatten()
            .filter(|s| s.a.distance_squared(camera) < 150f32.powi(2))
            .copied()
            .collect::<Vec<_>>();
        segments.sort_by(|a, b| {
            a.b.distance_squared(camera)
                .total_cmp(&b.b.distance_squared(camera))
        });
        segments.truncate(maximum.min(64));
        self.gpu = default();
        self.gpu.meta = Vec4::new(segments.len() as f32, 0., origin.x, origin.z);
        let mut low = Vec2::splat(f32::INFINITY);
        let mut high = Vec2::splat(f32::NEG_INFINITY);
        for (i, s) in segments.iter().enumerate() {
            let a = s.a - origin;
            let b = s.b - origin;
            let aa = Vec2::new(a.x, a.z);
            let bb = Vec2::new(b.x, b.z);
            low = low.min(aa.min(bb) - Vec2::splat(s.width + 0.25));
            high = high.max(aa.max(bb) + Vec2::splat(s.width + 0.25));
            self.gpu.segments[2 * i] = Vec4::new(a.x, a.z, b.x, b.z);
            self.gpu.segments[2 * i + 1] = Vec4::new((a.y + b.y) * 0.5, s.width, 0., 0.);
        }
        if !segments.is_empty() {
            self.gpu.bounds = Vec4::new(low.x, low.y, high.x, high.y);
        }
        self.gpu != previous_gpu
    }
}
pub fn update(
    mut state: ResMut<SnowTracks>,
    live: Res<crate::live::LiveDrive>,
    wet: Res<crate::wet_surfaces::WetSurfaces>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    camera: Query<&GlobalTransform, (With<Camera3d>, Without<crate::mirrors::MirrorCamera>)>,
    wheels: Query<(
        Entity,
        &GlobalTransform,
        &Aabb,
        &crate::rolling_stock_anim::TrainWheelAnim,
    )>,
    budget: Res<crate::render_budget::RenderBudget>,
    mut terrain: ResMut<Assets<openrailsrs_bevy_scenery::TerrainMaterial>>,
) {
    let _profile = crate::performance::profile_system("snow_tracks");
    // Clock bookkeeping and the bounded history are not GPU changes. Avoid
    // forcing all snow materials/uploads to refresh on a stationary or paused
    // frame solely because this resource was mutably borrowed.
    let tracks = state.bypass_change_detection();
    let clock = live.session.time_s();
    let previous = tracks.clock.replace(clock);
    // With no snow history, every freshly streamed material already has the
    // empty default buffer. Skip wheel queries and all material comparisons;
    // empty buffers do not depend on a moving/rebased camera's origin.
    if wet.snow_cover <= 0.05 && tracks.sectors.is_empty() && tracks.gpu.meta.x == 0. {
        tracks.previous.clear();
        return;
    }
    if previous.is_some_and(|p| clock > p) && wet.snow_cover > 0.05 {
        for (e, tf, aabb, wheel) in &wheels {
            for side in 0..2 {
                let lateral =
                    aabb.half_extents.x.clamp(0.45, 0.9) * if side == 0 { -1. } else { 1. };
                let mut p =
                    tf.transform_point(Vec3::from(aabb.center) + Vec3::X * lateral) + origin.shift;
                p.y -= wheel.radius_m;
                tracks.stamp((e, side), p);
            }
        }
    }
    tracks.previous.retain(|(e, _), _| wheels.get(*e).is_ok());
    let camera = camera.single().map_or(Vec3::ZERO, |p| p.translation()) + origin.shift;
    let changed = tracks.prepare(camera, origin.shift, 64 / budget.vegetation_divisor.max(1));
    if changed {
        tracks.gpu_changes += 1;
        state.set_changed();
    }
    // Newly streamed materials still need the current segments, even when the
    // wheels did not move. Only changed handles emit AssetEvent::Modified.
    let changed_materials = terrain
        .iter()
        .filter_map(|(id, material)| (material.snow_tracks != state.gpu).then_some(id))
        .collect::<Vec<_>>();
    state.bypass_change_detection().material_uploads += changed_materials.len() as u64;
    for id in changed_materials {
        if let Some(mut material) = terrain.get_mut(id) {
            material.snow_tracks = state.gpu.clone();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stationary_frame_does_not_mark_identical_gpu_segments_as_modified() {
        let mut tracks = SnowTracks::default();
        let wheel = (Entity::PLACEHOLDER, 0);
        tracks.stamp(wheel, Vec3::ZERO);
        tracks.stamp(wheel, Vec3::new(0.1, 0., 0.));
        assert!(tracks.prepare(Vec3::ZERO, Vec3::ZERO, 64));
        assert!(!tracks.prepare(Vec3::ZERO, Vec3::ZERO, 64));
        assert!(tracks.prepare(Vec3::ZERO, Vec3::new(500., 0., 0.), 64));
        assert!(!tracks.prepare(Vec3::ZERO, Vec3::new(500., 0., 0.), 64));
    }
    #[test]
    fn paused_wheels_and_teleport_do_not_clear_new_snow() {
        let mut tracks = SnowTracks::default();
        let wheel = (Entity::PLACEHOLDER, 0);
        tracks.stamp(wheel, Vec3::ZERO);
        for _ in 0..100 {
            tracks.stamp(wheel, Vec3::ZERO);
        }
        assert!(tracks.sectors.is_empty());
        tracks.stamp(wheel, Vec3::new(500., 0., 0.));
        assert!(tracks.sectors.is_empty());
        tracks.stamp(wheel, Vec3::new(500.1, 0., 0.));
        assert_eq!(tracks.sectors.values().flatten().count(), 1);
        tracks.prepare(Vec3::new(500.1, 0., 0.), Vec3::new(400., 0., 0.), 8);
        assert_eq!(tracks.gpu.meta.x, 1.);
        let bounds = tracks.gpu.bounds;
        assert!(bounds.x < 100. && bounds.z > 100.1);
        assert!(bounds.y < 0. && bounds.w > 0.);
    }
    #[test]
    fn tracks_follow_actual_motion_are_bounded_and_rebase_without_moving() {
        let mut s = SnowTracks::default();
        let id = (Entity::PLACEHOLDER, 0);
        for n in 0..2000 {
            s.stamp(id, Vec3::new(n as f32 * 0.1, 0., 0.));
        }
        assert_eq!(s.sectors[&(0, 0)].len(), 256);
        s.prepare(Vec3::new(195., 0., 0.), Vec3::ZERO, 64);
        let first = s.gpu.segments[0];
        assert_eq!(s.gpu.meta.x, 64.);
        s.prepare(Vec3::new(195., 0., 0.), Vec3::new(100., 0., 0.), 64);
        assert_eq!(s.gpu.segments[0].x, first.x - 100.);
        s.prepare(Vec3::new(10000., 0., 10000.), Vec3::ZERO, 8);
        assert!(s.sectors.is_empty());
        assert_eq!(s.gpu.meta.x, 0.);
    }
}
