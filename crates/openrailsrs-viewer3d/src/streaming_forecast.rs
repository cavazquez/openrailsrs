//! Shared bounded forecast for terrain and scenery, sampled on the actual TDB
//! itinerary. Camera/tail residency is independent of the speculative lead.
use bevy::prelude::*;
use openrailsrs_bevy_scenery::stream::{StreamWindowPolicy, TileCoord};
use std::collections::HashSet;
#[derive(Resource, Default)]
pub struct StreamingForecast {
    pub centres: Vec<Vec3>,
    pub camera: Vec3,
    pub forward: Vec3,
    pub speed_mps: f32,
    pub pressured: bool,
    elapsed: f32,
    last_center: Option<Vec3>,
}
fn tile(point: Vec3) -> TileCoord {
    TileCoord::new(
        openrailsrs_formats::msts_tile_x_index_for_coord(point.x),
        openrailsrs_formats::msts_tile_z_index_for_coord(point.z),
    )
}
fn tile_center(x: i32, z: i32) -> Vec3 {
    let (x, z) = openrailsrs_formats::msts_tile_world_origin(x, z);
    Vec3::new(x + 1024., 0., z + 1024.)
}
impl StreamingForecast {
    pub fn priority_in_focus(&self, position: Vec3, focus: &crate::world::RouteFocus) -> f32 {
        self.priority(position + Vec3::new(focus.center.x, focus.height_origin, focus.center.z))
    }
    pub fn priority(&self, position: Vec3) -> f32 {
        let delta = position - self.camera;
        let horizontal = Vec3::new(delta.x, 0., delta.z);
        let view = Vec3::new(self.forward.x, 0., self.forward.z).normalize_or_zero();
        let visible =
            horizontal.length_squared() < 100. || horizontal.normalize_or_zero().dot(view) > 0.5;
        let nearest = self
            .centres
            .iter()
            .map(|p| Vec2::new(p.x - position.x, p.z - position.z).length())
            .fold(horizontal.length(), f32::min);
        nearest / self.speed_mps.max(5.) - if visible { 12. } else { 0. }
    }
    pub fn requests(
        &self,
        policy: &StreamWindowPolicy,
        center: Vec3,
        known: &HashSet<TileCoord>,
    ) -> Vec<TileCoord> {
        let base = policy.diff_disk(tile(center), &HashSet::new()).to_load;
        let mut result = base.clone();
        let mut extra = self
            .centres
            .iter()
            .flat_map(|p| policy.diff_disk(tile(*p), &HashSet::new()).to_load)
            .filter(|t| !base.contains(t))
            .collect::<Vec<_>>();
        extra.sort_by_key(|t| (t.x, t.z));
        extra.dedup();
        extra.sort_by(|a, b| {
            self.priority(tile_center(a.x, a.z))
                .total_cmp(&self.priority(tile_center(b.x, b.z)))
        });
        // Extra sectors have a hard cap. A distant free camera cannot multiply
        // the baseline disk into an unlimited second scene.
        result.extend(extra.into_iter().take(if self.pressured { 6 } else { 18 }));
        result.retain(|t| !known.contains(t));
        result.sort_by(|a, b| {
            self.priority(tile_center(a.x, a.z))
                .total_cmp(&self.priority(tile_center(b.x, b.z)))
                .then((a.x, a.z).cmp(&(b.x, b.z)))
        });
        result
    }
    pub fn keep(&self, policy: &StreamWindowPolicy, coord: TileCoord) -> bool {
        self.centres.first().is_some_and(|p| {
            policy.should_keep_loaded(tile(*p), coord)
                || self.requests(policy, *p, &HashSet::new()).contains(&coord)
        })
    }
}
pub fn update(
    time: Res<Time<Real>>,
    mut forecast: ResMut<StreamingForecast>,
    live: Res<crate::live::LiveDrive>,
    window: Res<crate::view_window::ViewWindow>,
    residency: Res<crate::gpu_memory::ResidencyPolicy>,
    focus: Res<crate::world::RouteFocus>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    scene: Res<crate::track::TrackScene>,
    offset: Res<crate::world::RouteWorldOffset>,
    assets: Res<crate::shapes::RouteAssets>,
    resolver: Res<crate::track_position::TrackPositionResolverCache>,
    terrain: Option<Res<crate::terrain::TerrainElevation>>,
    camera: Query<&Transform, (With<Camera3d>, Without<crate::mirrors::MirrorCamera>)>,
) {
    let _profile = crate::performance::profile_system("streaming_forecast.update");
    forecast.pressured = residency.pressured;
    forecast.elapsed += time.delta_secs();
    if forecast.elapsed < 0.2
        && forecast
            .last_center
            .is_some_and(|p| p.distance_squared(window.center_world) < 100.)
    {
        return;
    }
    forecast.elapsed = 0.;
    forecast.last_center = Some(window.center_world);
    forecast.centres.clear();
    forecast.centres.push(window.center_world);
    if let Ok(camera) = camera.single() {
        forecast.camera = camera.translation
            + origin.shift
            + Vec3::new(focus.center.x, focus.height_origin, focus.center.z);
        forecast.forward = *camera.forward();
    }
    forecast.speed_mps = live.session.velocity_mps() as f32;
    let tdb = assets
        .track_db()
        .map(|t| resolver.resolver(t, Some(assets.tsection())));
    for distance in [
        0.,
        -live.session.formation.length_m(),
        (live.session.velocity_mps() * 30.).clamp(0., 2000.),
    ] {
        let Some((edge, at)) = live.visual_position_at_head_offset(distance) else {
            continue;
        };
        let Some((pos, _)) = crate::track_position::vehicle_pose_on_graph_edge(
            &scene.graph,
            &edge,
            at,
            tdb.as_ref(),
            &scene,
            offset.delta,
            &focus,
            terrain.as_deref(),
        ) else {
            continue;
        };
        let world = pos + Vec3::new(focus.center.x, focus.height_origin, focus.center.z);
        if !forecast
            .centres
            .iter()
            .any(|p| Vec2::new(p.x - world.x, p.z - world.z).length() < 250.)
        {
            forecast.centres.push(world);
        }
    }
    forecast.centres.truncate(4);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forecast_residency_is_bounded_even_after_repeated_requests_and_a_distant_camera() {
        let center = tile_center(0, 0);
        let plan = StreamingForecast {
            centres: vec![center, tile_center(2, 0), tile_center(20, 20)],
            camera: center,
            forward: Vec3::X,
            speed_mps: 30.,
            ..Default::default()
        };
        let policy = StreamWindowPolicy::chebyshev(1, 1);
        let mut known = HashSet::new();
        for _ in 0..10 {
            known.extend(plan.requests(&policy, center, &known));
        }
        assert!(known.len() <= 27);
        assert!(known.contains(&tile(center)));
        let stale = TileCoord::new(100, 100);
        assert!(!plan.keep(&policy, stale));
        assert!(plan.priority(center + Vec3::X * 500.) < plan.priority(center - Vec3::X * 500.));
        assert_eq!(
            tile(tile_center(-6080, 14925)),
            TileCoord::new(-6080, 14925)
        );
    }
    #[test]
    fn shape_priority_uses_world_coordinates_after_a_remote_native_focus() {
        let focus = crate::world::RouteFocus {
            center: Vec3::new(-12_000_000., 0., 30_000_000.),
            height_origin: 120.,
        };
        let camera = Vec3::new(focus.center.x, 120., focus.center.z);
        let plan = StreamingForecast {
            centres: vec![camera],
            camera,
            forward: Vec3::X,
            speed_mps: 30.,
            ..Default::default()
        };
        assert_eq!(
            plan.priority_in_focus(Vec3::X * 500., &focus),
            plan.priority(camera + Vec3::X * 500.)
        );
        assert!(
            plan.priority_in_focus(Vec3::X * 500., &focus)
                < plan.priority_in_focus(Vec3::NEG_X * 500., &focus)
        );
    }
    #[test]
    fn pressure_preserves_the_bounded_requested_set_without_reload_churn() {
        let center = tile_center(0, 0);
        let plan = StreamingForecast {
            centres: vec![center, tile_center(2, 0), tile_center(20, 20)],
            camera: center,
            forward: Vec3::X,
            speed_mps: 30.,
            pressured: true,
            ..Default::default()
        };
        let policy = StreamWindowPolicy::chebyshev(1, 0);
        let requested = plan.requests(&policy, center, &HashSet::new());
        assert!(requested.len() <= 15);
        assert!(requested.iter().all(|t| plan.keep(&policy, *t)));
        let known = requested.into_iter().collect();
        assert!(plan.requests(&policy, center, &known).is_empty());
        assert!(!plan.keep(&policy, TileCoord::new(100, 100)));
    }
}
