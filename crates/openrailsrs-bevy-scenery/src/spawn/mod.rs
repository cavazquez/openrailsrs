//! Progressive WORLD / track scenery spawn (shared between viewer3d and render3d).
//!
//! [#52](https://github.com/cavazquez/openrailsrs/issues/52): shared `SystemSet`s, budgets,
//! progress messages and a spawn-cycle guard. App-specific FSMs (`WorldSpawnProgress`,
//! `LoadStage`) stay in each binary and schedule into these sets.

pub mod dyntrack;
pub mod tdb_track;
pub mod wire;

use bevy::prelude::*;

/// How shared scenery spawn systems batch GPU work.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScenerySpawnMode {
    /// Tile/stream batches (default for large routes).
    #[default]
    Progressive,
    /// Spawn everything in one startup pass (small routes / tests).
    Eager,
}

/// Logical scenery load phases shared by both viewers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScenerySpawnPhase {
    #[default]
    Catalog,
    Terrain,
    Track,
    Objects,
    Ready,
}

impl ScenerySpawnPhase {
    pub const ALL: [Self; 5] = [
        Self::Catalog,
        Self::Terrain,
        Self::Track,
        Self::Objects,
        Self::Ready,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::Terrain => "terrain",
            Self::Track => "track",
            Self::Objects => "objects",
            Self::Ready => "ready",
        }
    }
}

/// Ordered system sets for scenery load / stream work.
///
/// Both binaries place their systems here; internal FSMs may still advance multiple
/// phases inside a single system (e.g. render3d `progressive_world_load`).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ScenerySpawnSet {
    Catalog,
    Terrain,
    Track,
    Objects,
    Ready,
}

impl From<ScenerySpawnPhase> for ScenerySpawnSet {
    fn from(phase: ScenerySpawnPhase) -> Self {
        match phase {
            ScenerySpawnPhase::Catalog => Self::Catalog,
            ScenerySpawnPhase::Terrain => Self::Terrain,
            ScenerySpawnPhase::Track => Self::Track,
            ScenerySpawnPhase::Objects => Self::Objects,
            ScenerySpawnPhase::Ready => Self::Ready,
        }
    }
}

/// Per-frame budgets (defaults align with viewer3d progressive spawn).
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct ScenerySpawnBudgets {
    pub classify_items_per_frame: usize,
    pub shapes_per_frame: usize,
    pub textures_per_frame: usize,
    pub shape_assets_per_frame: usize,
    pub build_queue_shapes_per_frame: usize,
    pub entities_per_frame: usize,
}

impl Default for ScenerySpawnBudgets {
    fn default() -> Self {
        Self {
            classify_items_per_frame: 12_000,
            shapes_per_frame: 16,
            textures_per_frame: 32,
            shape_assets_per_frame: 64,
            build_queue_shapes_per_frame: 32,
            entities_per_frame: 600,
        }
    }
}

/// Active spawn cycle — prevents double parse/spawn after a cycle finishes.
#[derive(Resource, Clone, Debug)]
pub struct ScenerySpawnCycle {
    pub generation: u64,
    pub phase: ScenerySpawnPhase,
    pub active: bool,
    /// Test/telemetry: how many times [`Self::note_spawn_work`] was called this generation.
    pub spawn_work_count: u64,
}

impl Default for ScenerySpawnCycle {
    fn default() -> Self {
        Self {
            generation: 0,
            phase: ScenerySpawnPhase::Catalog,
            active: false,
            spawn_work_count: 0,
        }
    }
}

impl ScenerySpawnCycle {
    /// Start (or restart) a cycle. Increments `generation` and clears work counters.
    pub fn begin(&mut self, phase: ScenerySpawnPhase) {
        self.generation = self.generation.saturating_add(1);
        self.phase = phase;
        self.active = true;
        self.spawn_work_count = 0;
    }

    pub fn set_phase(&mut self, phase: ScenerySpawnPhase) {
        self.phase = phase;
    }

    pub fn finish(&mut self) {
        self.active = false;
        self.phase = ScenerySpawnPhase::Ready;
    }

    pub fn note_spawn_work(&mut self) {
        if self.active {
            self.spawn_work_count = self.spawn_work_count.saturating_add(1);
        }
    }
}

/// Progress notification for HUD / loading UI (optional consumers).
#[derive(Message, Clone, Debug)]
pub struct ScenerySpawnProgress {
    pub generation: u64,
    pub phase: ScenerySpawnPhase,
    /// 0.0–1.0 within the overall load when known.
    pub fraction: f32,
    pub detail: String,
}

impl ScenerySpawnProgress {
    pub fn new(
        cycle: &ScenerySpawnCycle,
        fraction: f32,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            generation: cycle.generation,
            phase: cycle.phase,
            fraction: fraction.clamp(0.0, 1.0),
            detail: detail.into(),
        }
    }
}

/// `run_if` — scenery spawn systems that must not run after [`ScenerySpawnCycle::finish`].
pub fn scenery_spawn_cycle_active(cycle: Res<ScenerySpawnCycle>) -> bool {
    cycle.active
}

/// Registers shared scenery spawn resources, messages and ordered system sets.
pub struct ScenerySpawnPlugin;

impl Plugin for ScenerySpawnPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScenerySpawnMode>()
            .init_resource::<ScenerySpawnBudgets>()
            .init_resource::<ScenerySpawnCycle>()
            .add_message::<ScenerySpawnProgress>()
            .configure_sets(
                Startup,
                (
                    ScenerySpawnSet::Catalog,
                    ScenerySpawnSet::Terrain,
                    ScenerySpawnSet::Track,
                    ScenerySpawnSet::Objects,
                    ScenerySpawnSet::Ready,
                )
                    .chain(),
            )
            .configure_sets(
                Update,
                (
                    ScenerySpawnSet::Catalog,
                    ScenerySpawnSet::Terrain,
                    ScenerySpawnSet::Track,
                    ScenerySpawnSet::Objects,
                    ScenerySpawnSet::Ready,
                )
                    .chain(),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct PhaseLog(Vec<&'static str>);

    fn log_catalog(mut log: ResMut<PhaseLog>, mut cycle: ResMut<ScenerySpawnCycle>) {
        if cycle.active {
            log.0.push("catalog");
            cycle.set_phase(ScenerySpawnPhase::Terrain);
        }
    }

    fn log_terrain(mut log: ResMut<PhaseLog>, mut cycle: ResMut<ScenerySpawnCycle>) {
        if cycle.active {
            log.0.push("terrain");
            cycle.set_phase(ScenerySpawnPhase::Objects);
        }
    }

    fn log_objects(mut log: ResMut<PhaseLog>, mut cycle: ResMut<ScenerySpawnCycle>) {
        if cycle.active {
            log.0.push("objects");
            cycle.note_spawn_work();
            cycle.finish();
        }
    }

    fn log_ready(mut log: ResMut<PhaseLog>) {
        log.0.push("ready");
    }

    fn spawn_work_if_active(mut cycle: ResMut<ScenerySpawnCycle>) {
        cycle.note_spawn_work();
    }

    #[test]
    fn plugin_registers_shared_resources() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(ScenerySpawnPlugin);
        assert!(app.world().contains_resource::<ScenerySpawnMode>());
        assert!(app.world().contains_resource::<ScenerySpawnBudgets>());
        assert!(app.world().contains_resource::<ScenerySpawnCycle>());
        assert!(!app.world().resource::<ScenerySpawnCycle>().active);
    }

    #[test]
    fn system_sets_run_in_catalog_terrain_objects_ready_order() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(ScenerySpawnPlugin)
            .init_resource::<PhaseLog>()
            .add_systems(
                Update,
                (
                    log_catalog.in_set(ScenerySpawnSet::Catalog),
                    log_terrain.in_set(ScenerySpawnSet::Terrain),
                    log_objects.in_set(ScenerySpawnSet::Objects),
                    log_ready.in_set(ScenerySpawnSet::Ready),
                ),
            );

        app.world_mut()
            .resource_mut::<ScenerySpawnCycle>()
            .begin(ScenerySpawnPhase::Catalog);
        app.update();

        assert_eq!(
            app.world().resource::<PhaseLog>().0,
            vec!["catalog", "terrain", "objects", "ready"]
        );
    }

    #[test]
    fn finished_cycle_does_not_accept_more_spawn_work() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(ScenerySpawnPlugin)
            .add_systems(
                Update,
                spawn_work_if_active.run_if(scenery_spawn_cycle_active),
            );

        {
            let mut cycle = app.world_mut().resource_mut::<ScenerySpawnCycle>();
            cycle.begin(ScenerySpawnPhase::Objects);
        }
        app.update();
        assert_eq!(app.world().resource::<ScenerySpawnCycle>().spawn_work_count, 1);

        app.world_mut().resource_mut::<ScenerySpawnCycle>().finish();
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<ScenerySpawnCycle>().spawn_work_count,
            1,
            "no spawn work after finish"
        );
        assert!(!app.world().resource::<ScenerySpawnCycle>().active);
    }

    #[test]
    fn begin_increments_generation_and_resets_work() {
        let mut cycle = ScenerySpawnCycle::default();
        cycle.begin(ScenerySpawnPhase::Catalog);
        cycle.note_spawn_work();
        assert_eq!(cycle.generation, 1);
        assert_eq!(cycle.spawn_work_count, 1);
        cycle.finish();
        cycle.begin(ScenerySpawnPhase::Objects);
        assert_eq!(cycle.generation, 2);
        assert_eq!(cycle.spawn_work_count, 0);
        assert!(cycle.active);
    }

    #[test]
    fn or_scenery_plugins_includes_spawn_plugin() {
        let app = crate::test_harness::minimal_scenery_app();
        assert!(app.world().contains_resource::<ScenerySpawnCycle>());
        assert!(app.world().contains_resource::<ScenerySpawnBudgets>());
        assert_eq!(
            *app.world().resource::<ScenerySpawnMode>(),
            ScenerySpawnMode::Progressive
        );
    }
}
