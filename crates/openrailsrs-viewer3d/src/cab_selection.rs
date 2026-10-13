//! Move the cab hierarchy and authored eye to the selected physical vehicle.
use bevy::prelude::*;

pub fn sync(
    mut commands: Commands,
    live: Res<crate::live::LiveDrive>,
    mut scene: ResMut<crate::rolling_stock::TrainConsistScene>,
    assets: Res<crate::shapes::RouteAssets>,
    mut state: ResMut<crate::cab_view::CabInteriorState>,
    mut cvf: ResMut<crate::cab_cvf::CabCvfState>,
    mut cab: ResMut<crate::camera::LiveDriverCab>,
    mut look: ResMut<crate::camera::DriverLookOffset>,
    cars: Query<(Entity, &crate::live::LiveTrainCar)>,
    roots: Query<Entity, With<crate::cab_view::CabInteriorRoot>>,
    mut previous: Local<Option<(String, usize)>>,
) {
    let index = live.session.cab_vehicle();
    let key = (
        format!("{:?}/{:?}", assets.route_dir, scene.primary_consist_rel),
        index,
    );
    if previous.as_ref() == Some(&key) {
        return;
    }
    // Initial eye was already prepared from the full native mesh during spawn.
    let first = previous.is_none();
    scene.active_cab_vehicle = index;
    for (entity, car) in &cars {
        if car.index == index {
            commands
                .entity(entity)
                .insert(crate::cab_view::CabLeadVehicle);
        } else {
            commands
                .entity(entity)
                .remove::<crate::cab_view::CabLeadVehicle>();
        }
    }
    if !first || index != 0 {
        for entity in &roots {
            commands.entity(entity).despawn();
        }
        state.reset_for_vehicle();
        crate::cab_cvf::reset_cab_cvf_state(&mut cvf);
        look.reset();
        let Some(vehicle) = scene.cab_vehicle() else {
            return;
        };
        let dirs = scene.shape_search_dirs(&assets.route_dir);
        let refs = dirs.iter().map(|p| p.as_path()).collect::<Vec<_>>();
        let mesh = Mesh::from(Cuboid::new(3., 4., vehicle.length_m));
        *cab = crate::live::driver_cab_from_lead_vehicle(vehicle, &refs, &assets.route_dir, &mesh);
    }
    *previous = Some(key);
}
