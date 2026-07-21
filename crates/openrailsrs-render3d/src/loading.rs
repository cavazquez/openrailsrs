//! Pantalla de carga y spawn progresivo del mundo (evita congelar la ventana).

use std::collections::HashSet;
use std::time::Instant;

use bevy::ecs::query::Without;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};

use crate::consist::{StaticConsistPlan, spawn_static_consist};
use crate::debug_hud::{FlyCamera, spawn_debug_hud, spawn_ui_overlay_camera};
use crate::objects::ObjectMarker;
use crate::or_cascade::{or_limits_from_view_distance, or_max_shadow_view_distance};
use crate::or_scenery_material::OrSceneryMaterial;
use crate::or_terrain_material::OrTerrainMaterial;
use crate::or_vsm_render::OrMomentAtlasImage;
use crate::player_spawn::{PlayerStartPose, PlayerStartPoseResource};
use crate::stream::{
    SavedTerrainCtx, StreamWorldAssets, TileCatalog, TileStreamConfig, TileStreamState,
    initial_loaded_tiles,
};
use crate::terrain::TileGeometry;
use openrailsrs_bevy_scenery::MstsLoadDiagnostics;

use crate::tile_parse::{ParsedTiles, TileParseRequest, parse_tiles_for_load};
use crate::world_spawn::{
    AssetIndex, ObjectSpawnCtx, TerrainSpawnCtx, TextureLoadStats, TrackSpawnStats,
    spawn_objects_batch, spawn_terrain_patches, spawn_tile_track,
};

/// Fases de la app: carga → juego.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    #[default]
    Loading,
    Playing,
}

/// Progreso mostrado en la UI (0..1).
#[derive(Resource)]
pub struct LoadProgress {
    pub label: String,
    pub fraction: f32,
}

impl LoadProgress {
    fn set(&mut self, label: impl Into<String>, fraction: f32) {
        self.label = label.into();
        self.fraction = fraction.clamp(0.0, 1.0);
    }
}

/// Líneas recientes del log de carga (visible en pantalla).
#[derive(Resource)]
pub struct LoadLog {
    entries: Vec<String>,
    max_entries: usize,
}

impl LoadLog {
    fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
        }
    }

    fn push(&mut self, line: impl Into<String>) {
        self.entries.push(line.into());
        if self.entries.len() > self.max_entries {
            let drop = self.entries.len() - self.max_entries;
            self.entries.drain(0..drop);
        }
    }

    fn body(&self) -> String {
        self.entries.join("\n")
    }
}

/// Entidades de la pantalla de carga (para despawn al terminar).
#[derive(Resource)]
pub struct LoadingScreen {
    pub root: Entity,
    pub bar_fill: Entity,
    pub status: Entity,
    pub log: Entity,
    /// Texto secundario: "Tile N/M".
    pub tile_counter: Entity,
}

// ─── Etapas de carga ─────────────────────────────────────────────────────────

/// Un slot de tile pendiente de procesar: datos ya en memoria.
pub struct TileSlot {
    geometry: crate::terrain::TileGeometry,
    world_offset: Vec3,
    track: crate::track::TrackRibbon,
    objects: Vec<ObjectMarker>,
}

/// Estado interno del spawn por fases.
#[derive(Resource)]
pub enum LoadStage {
    /// Parse CPU de tiles WORLD/terrain (hilo en background, #55).
    ParsingTiles {
        task: Task<Result<ParsedTiles, String>>,
    },
    /// Indexando SHAPES/TEXTURES (hilo en background).
    Indexing {
        task: Task<AssetIndex>,
        /// Tiles listos para procesar.
        slots: Vec<TileSlot>,
    },
    /// Terreno texturizado (tile_i = índice del tile actual).
    Terrain {
        index: AssetIndex,
        ctx: TerrainSpawnCtx,
        slots: Vec<TileSlot>,
        tile_i: usize,
        patch_i: usize,
    },
    /// Cinta de vía (todos los tiles).
    Track {
        index: AssetIndex,
        obj_ctx: ObjectSpawnCtx,
        slots: Vec<TileSlot>,
        tile_i: usize,
        /// Built once for the whole Track batch (#63).
        height_index: Option<crate::tdb_track::TileHeightIndex>,
        /// How many times the height index was constructed (expect 1 per batch).
        height_index_builds: u32,
    },
    /// Objetos `.s` en lotes.
    Objects {
        index: AssetIndex,
        obj_ctx: ObjectSpawnCtx,
        slots: Vec<TileSlot>,
        /// Cursor global: (tile_i, objeto_i_dentro_del_tile).
        tile_i: usize,
        obj_i: usize,
        /// Objetos filtrables del tile actual.
        filtered: Vec<ObjectMarker>,
    },
    /// Carga terminada; `finish_world_load` activa streaming y pasa a `Playing`.
    Finished {
        index: AssetIndex,
        obj_ctx: ObjectSpawnCtx,
        initial_tile_coords: Vec<(i32, i32)>,
    },
}

const TERRAIN_PATCHES_PER_FRAME: usize = 32;
const OBJECTS_PER_FRAME: usize = 40;
const LOG_MAX_LINES: usize = 14;
const LOG_NAME_PREVIEW: usize = 6;

/// Activar con `OPENRAILSRS_PERF_DEBUG=1`.
pub fn perf_debug() -> bool {
    std::env::var_os("OPENRAILSRS_PERF_DEBUG").is_some()
}

/// Umbral de duración de un batch (ms) a partir del cual se imprime una
/// advertencia de cuello de botella aun cuando perf_debug no esté activo.
const SLOW_BATCH_MS: f64 = 32.0;

/// Acumulador de tiempos por fase de carga.
#[derive(Resource)]
pub struct LoadPerfState {
    pub total_start: Instant,
    pub phase_start: Instant,
    /// Tiempo acumulado de indexado (seg).
    pub secs_indexing: f64,
    /// Tiempo acumulado de terreno (seg).
    pub secs_terrain: f64,
    /// Tiempo acumulado de vía (seg).
    pub secs_track: f64,
    /// Tiempo acumulado de objetos (seg).
    pub secs_objects: f64,
    /// Batches de terreno procesados (para promedios).
    pub terrain_batches: u32,
    /// Batches de objetos procesados.
    pub object_batches: u32,
    /// Lado del tile central (m) para la cámara inicial.
    pub scene_side_m: f32,
    /// Posición inicial del jugador (cámara cabina).
    pub player_start: Option<PlayerStartPose>,
    /// HUD de depuración visible al entrar en juego.
    pub hud_enabled: bool,
    /// Materiales PBR iluminados (día); unlit con emissive en noche.
    pub materials_lit: bool,
    /// Tiles cargados (para radio del domo de cielo).
    pub tile_count: usize,
    /// Posición de la cámara al cargar (LOD de shapes).
    pub viewer_pos: Vec3,
}

impl LoadPerfState {
    fn new(
        scene_side_m: f32,
        player_start: Option<PlayerStartPose>,
        hud_enabled: bool,
        materials_lit: bool,
        tile_count: usize,
    ) -> Self {
        let now = Instant::now();
        let viewer_pos = player_start.map(|p| p.position).unwrap_or(Vec3::ZERO);
        Self {
            total_start: now,
            phase_start: now,
            secs_indexing: 0.0,
            secs_terrain: 0.0,
            secs_track: 0.0,
            secs_objects: 0.0,
            terrain_batches: 0,
            object_batches: 0,
            scene_side_m,
            player_start,
            hud_enabled,
            materials_lit,
            tile_count,
            viewer_pos,
        }
    }

    fn elapsed_phase(&self) -> f64 {
        self.phase_start.elapsed().as_secs_f64()
    }

    fn elapsed_total(&self) -> f64 {
        self.total_start.elapsed().as_secs_f64()
    }

    fn reset_phase(&mut self) {
        self.phase_start = Instant::now();
    }

    fn print_phase(&self, phase: &str, detail: &str, elapsed: f64) {
        eprintln!(
            "[PERF] {phase:<10} {detail:<50}  {elapsed:6.3}s  (total {:.3}s)",
            self.elapsed_total()
        );
    }
}

// ─── Setup ───────────────────────────────────────────────────────────────────

pub fn setup_loading_screen(mut commands: Commands) {
    commands.insert_resource(LoadProgress {
        label: "Iniciando…".into(),
        fraction: 0.0,
    });
    commands.insert_resource(LoadLog::new(LOG_MAX_LINES));

    let root = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(12.0),
                padding: UiRect::all(Val::Px(24.0)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.10, 0.14)),
        ))
        .id();

    let title = commands
        .spawn((
            Text::new("openrailsrs-render3d"),
            TextFont {
                font_size: FontSize::Px(28.0),
                ..default()
            },
            TextColor(Color::srgb(0.92, 0.94, 0.98)),
        ))
        .id();

    let tile_counter = commands
        .spawn((
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(Color::srgb(0.55, 0.65, 0.85)),
        ))
        .id();

    let status = commands
        .spawn((
            Text::new("Cargando…"),
            TextFont {
                font_size: FontSize::Px(17.0),
                ..default()
            },
            TextColor(Color::srgb(0.75, 0.80, 0.88)),
        ))
        .id();

    // Barra de progreso.
    let bar_bg = commands
        .spawn((
            Node {
                width: Val::Px(520.0),
                height: Val::Px(10.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.18, 0.20, 0.25)),
            BorderColor::all(Color::srgb(0.30, 0.34, 0.44)),
        ))
        .id();

    let bar_fill = commands
        .spawn((
            Node {
                width: Val::Percent(0.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.28, 0.60, 0.98)),
        ))
        .id();

    // Panel de log.
    let log_panel = commands
        .spawn((
            Node {
                width: Val::Px(520.0),
                height: Val::Px(200.0),
                margin: UiRect::top(Val::Px(6.0)),
                padding: UiRect::all(Val::Px(12.0)),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.06, 0.09, 0.95)),
            BorderColor::all(Color::srgb(0.20, 0.24, 0.32)),
        ))
        .id();

    let log = commands
        .spawn((
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(Color::srgb(0.55, 0.65, 0.72)),
        ))
        .id();

    commands.entity(log_panel).add_child(log);
    commands.entity(bar_bg).add_child(bar_fill);
    commands
        .entity(root)
        .add_children(&[title, tile_counter, status, bar_bg, log_panel]);

    commands.spawn((
        Camera2d,
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.08, 0.10, 0.14)),
            ..default()
        },
    ));

    commands.insert_resource(LoadingScreen {
        root,
        bar_fill,
        status,
        log,
        tile_counter,
    });
}

#[allow(clippy::too_many_arguments)]
pub fn begin_load_stage(
    mut commands: Commands,
    request: Res<TileParseRequest>,
    mut progress: ResMut<LoadProgress>,
    mut log: ResMut<LoadLog>,
    hud_enabled: Res<crate::debug_hud::DebugHudEnabled>,
    texture_env: Res<crate::textures::TextureEnvironment>,
    mut cycle: ResMut<openrailsrs_bevy_scenery::ScenerySpawnCycle>,
    mut progress_msg: MessageWriter<openrailsrs_bevy_scenery::ScenerySpawnProgress>,
) {
    cycle.begin(openrailsrs_bevy_scenery::ScenerySpawnPhase::Catalog);
    progress.set("Parseando tiles WORLD/terrain…", 0.01);
    log.push(format!(
        "→ Parseando tiles (centro {:?}, radio {})…",
        request.center, request.radius
    ));
    progress_msg.write(openrailsrs_bevy_scenery::ScenerySpawnProgress::new(
        &cycle,
        0.01,
        "parsing tiles",
    ));

    commands.insert_resource(TextureLoadStats::default());
    commands.insert_resource(TrackSpawnStats::default());
    // Placeholders until parse finishes (sun/extent already seeded from main).
    commands.insert_resource(LoadPerfState::new(
        2048.0,
        None,
        hud_enabled.0,
        !texture_env.night,
        0,
    ));

    let req = request.clone();
    let task = AsyncComputeTaskPool::get().spawn(async move { parse_tiles_for_load(req) });
    commands.insert_resource(LoadStage::ParsingTiles { task });
    if perf_debug() {
        eprintln!(
            "[PERF] begin_load_stage: ventana abierta → parse async tiles {:?}",
            request.center
        );
    }
}

fn start_indexing_from_parsed(
    commands: &mut Commands,
    parsed: ParsedTiles,
    route: &crate::RouteDir,
    msts_root: &crate::MstsRootDir,
    progress: &mut LoadProgress,
    log: &mut LoadLog,
    perf: &mut LoadPerfState,
    debug_ctx: &mut crate::debug_hud::SceneDebugContext,
) -> LoadStage {
    let n = parsed.tiles_to_render.0.len();
    let object_count: usize = parsed.tiles_to_render.0.iter().map(|e| e.objects.len()).sum();
    println!(
        "render3d: {} tiles cargados ({} sin .t ignorados), {} patches, {} segmentos de vía, {} objetos",
        parsed.catalog.entries.len(),
        parsed.skipped,
        parsed.total_patches,
        parsed.total_segments,
        parsed.total_objects,
    );
    if let Some(pose) = &parsed.player_start {
        println!(
            "cámara: ({:.0}, {:.1}, {:.0}) yaw {:.0}°",
            pose.position.x,
            pose.position.y,
            pose.position.z,
            pose.yaw_rad.to_degrees()
        );
    }
    if let Some(plan) = &parsed.consist_plan {
        println!("consist: {} vehículo(s)", plan.vehicles.len());
    }

    let player_start = parsed.player_start;
    debug_ctx.tile_count = n;
    debug_ctx.object_count = object_count;
    perf.scene_side_m = parsed.scene_side_m;
    perf.player_start = player_start;
    perf.tile_count = n;
    perf.viewer_pos = player_start.map(|p| p.position).unwrap_or(Vec3::ZERO);

    let mut slots: Vec<TileSlot> = Vec::with_capacity(n);
    for entry in &parsed.tiles_to_render.0 {
        slots.push(TileSlot {
            geometry: entry.geometry.clone(),
            world_offset: entry.world_offset,
            track: entry.track.clone(),
            objects: entry.objects.clone(),
        });
    }

    commands.insert_resource(parsed.catalog);
    commands.insert_resource(parsed.stream_config);
    commands.insert_resource(parsed.tiles_to_render);
    commands.insert_resource(crate::SceneExtent {
        side_m: parsed.scene_side_m,
    });
    commands.insert_resource(PlayerStartPoseResource(player_start));
    commands.insert_resource(parsed.load_diag);
    if let Some(plan) = parsed.consist_plan {
        commands.insert_resource(plan);
    }

    progress.set(format!("Indexando shapes y texturas… ({n} tiles)"), 0.05);
    log.push(format!("→ Indexando SHAPES y TEXTURES… ({n} tiles)"));

    let route_dir = route.0.clone();
    let msts_dir = msts_root.0.clone();
    let task =
        AsyncComputeTaskPool::get().spawn(async move { AssetIndex::build(&route_dir, &msts_dir) });
    if perf_debug() {
        eprintln!("[PERF] tiles parsed → indexing shapes/texturas ({n} tiles)");
    }
    LoadStage::Indexing { task, slots }
}

// ─── UI update ───────────────────────────────────────────────────────────────

pub fn update_loading_ui(
    progress: Res<LoadProgress>,
    log: Res<LoadLog>,
    screen: Res<LoadingScreen>,
    mut texts: Query<&mut Text>,
    mut bar: Query<&mut Node, Without<Text>>,
) {
    if progress.is_changed() {
        if let Ok(mut t) = texts.get_mut(screen.status) {
            *t = Text::new(format!(
                "{}  ({:.0} %)",
                progress.label,
                progress.fraction * 100.0
            ));
        }
        if let Ok(mut node) = bar.get_mut(screen.bar_fill) {
            node.width = Val::Percent(progress.fraction * 100.0);
        }
    }
    if log.is_changed() {
        if let Ok(mut t) = texts.get_mut(screen.log) {
            *t = Text::new(log.body());
        }
    }
}

// ─── Máquina de estados de carga ─────────────────────────────────────────────

#[derive(SystemParam)]
pub struct LoadRenderAssets<'w> {
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub materials: ResMut<'w, Assets<StandardMaterial>>,
    pub or_materials: ResMut<'w, Assets<OrSceneryMaterial>>,
    pub or_terrain_materials: ResMut<'w, Assets<OrTerrainMaterial>>,
    pub images: ResMut<'w, Assets<Image>>,
}

#[derive(SystemParam)]
pub struct LoadProgressUi<'w> {
    progress: ResMut<'w, LoadProgress>,
    log: ResMut<'w, LoadLog>,
    debug_ctx: ResMut<'w, crate::debug_hud::SceneDebugContext>,
}

#[allow(clippy::too_many_arguments)]
pub fn progressive_world_load(
    mut commands: Commands,
    mut stage: ResMut<LoadStage>,
    ui: LoadProgressUi,
    mut load_assets: LoadRenderAssets,
    route: Res<crate::RouteDir>,
    msts_root: Res<crate::MstsRootDir>,
    screen: Res<LoadingScreen>,
    mut tex_stats: ResMut<TextureLoadStats>,
    mut track_stats: ResMut<TrackSpawnStats>,
    texture_env: Res<crate::textures::TextureEnvironment>,
    mut texts: Query<&mut Text>,
    mut perf: ResMut<LoadPerfState>,
    moment_atlas: Res<OrMomentAtlasImage>,
    tdb_track: Option<Res<crate::TdbTrackResource>>,
    stream_config: Res<crate::stream::TileStreamConfig>,
) {
    let LoadProgressUi {
        mut progress,
        mut log,
        mut debug_ctx,
    } = ui;
    let materials_lit = perf.materials_lit;
    'progress: loop {
        match &mut *stage {
            // ── 0. Parse tiles (hilo async, #55) ─────────────────────────
            LoadStage::ParsingTiles { task } => {
                let Some(result) = check_ready(task) else {
                    break 'progress;
                };
                match result {
                    Ok(parsed) => {
                        let elapsed = perf.elapsed_phase();
                        if perf_debug() {
                            perf.print_phase(
                                "ParseTiles",
                                &format!(
                                    "{} tiles, {} objetos",
                                    parsed.catalog.entries.len(),
                                    parsed.total_objects
                                ),
                                elapsed,
                            );
                        }
                        perf.reset_phase();
                        log.push(format!(
                            "✓ Tiles: {} cargados ({} huecos)",
                            parsed.catalog.entries.len(),
                            parsed.skipped
                        ));
                        *stage = start_indexing_from_parsed(
                            &mut commands,
                            parsed,
                            &route,
                            &msts_root,
                            &mut progress,
                            &mut log,
                            &mut perf,
                            &mut debug_ctx,
                        );
                        continue 'progress;
                    }
                    Err(err) => {
                        progress.set(format!("Error: {err}"), 0.0);
                        log.push(format!("✗ {err}"));
                        error!("render3d tile parse failed: {err}");
                        commands.remove_resource::<LoadStage>();
                        break 'progress;
                    }
                }
            }

            // ── 1. Indexado (hilo async) ──────────────────────────────────
            LoadStage::Indexing { task, slots } => {
                if let Some(index) = check_ready(task) {
                    let elapsed = perf.elapsed_phase();
                    perf.secs_indexing = elapsed;
                    perf.reset_phase();
                    if perf_debug() {
                        perf.print_phase(
                            "Indexing",
                            &format!(
                                "{} shapes, {} texturas",
                                index.shape_count(),
                                index.texture_count()
                            ),
                            elapsed,
                        );
                    }
                    let n = slots.len();
                    log.push(format!(
                        "✓ Índice: {} shapes, {} texturas",
                        index.shape_count(),
                        index.texture_count()
                    ));
                    update_tile_counter(&screen, &mut texts, 0, n);
                    progress.set("Preparando terreno…", 0.05);
                    let ctx = TerrainSpawnCtx::new(
                        &mut load_assets.or_terrain_materials,
                        &mut load_assets.images,
                        materials_lit,
                        texture_env.night,
                    );
                    let slots = std::mem::take(slots);
                    *stage = LoadStage::Terrain {
                        index,
                        ctx,
                        slots,
                        tile_i: 0,
                        patch_i: 0,
                    };
                }
                break 'progress;
            }

            // ── 2. Terreno (patch a patch, tile a tile) ───────────────────
            LoadStage::Terrain {
                index,
                ctx,
                slots,
                tile_i,
                patch_i,
            } => {
                let n_tiles = slots.len();
                if *tile_i >= n_tiles {
                    // Todos los tiles de terreno terminados → pasar a vía.
                    let elapsed = perf.elapsed_phase();
                    perf.secs_terrain = elapsed;
                    perf.reset_phase();
                    if perf_debug() {
                        let avg_ms = if perf.terrain_batches > 0 {
                            elapsed * 1000.0 / perf.terrain_batches as f64
                        } else {
                            0.0
                        };
                        perf.print_phase(
                            "Terrain",
                            &format!(
                                "{n_tiles} tiles, {} batches, avg {avg_ms:.2}ms/batch",
                                perf.terrain_batches
                            ),
                            elapsed,
                        );
                    }
                    let slots = std::mem::take(slots);
                    commands.insert_resource(SavedTerrainCtx(ctx.clone()));
                    let moment_atlas = moment_atlas.0.clone();
                    let side = perf.scene_side_m.max(256.0);
                    let shadow_map_limits =
                        or_limits_from_view_distance(or_max_shadow_view_distance(side));
                    let obj_ctx = ObjectSpawnCtx::new(
                        &mut load_assets.materials,
                        materials_lit,
                        moment_atlas,
                        shadow_map_limits,
                    );
                    *stage = LoadStage::Track {
                        index: index.clone(),
                        obj_ctx,
                        slots,
                        tile_i: 0,
                        height_index: None,
                        height_index_builds: 0,
                    };
                    progress.set("Vía…", terrain_fraction(n_tiles, n_tiles));
                    continue 'progress;
                }

                let slot = &slots[*tile_i];
                let total_patches = slot.geometry.patches.len().max(1);
                let start = *patch_i;
                let end = (start + TERRAIN_PATCHES_PER_FRAME).min(total_patches);

                let batch_t = Instant::now();
                spawn_terrain_patches(
                    &mut commands,
                    &mut load_assets.meshes,
                    ctx,
                    &mut load_assets.or_terrain_materials,
                    &mut load_assets.images,
                    &route.0,
                    &slot.geometry,
                    start,
                    end,
                    slot.world_offset,
                    slot.geometry.tile_x,
                    slot.geometry.tile_z,
                );
                let batch_ms = batch_t.elapsed().as_secs_f64() * 1000.0;
                perf.terrain_batches += 1;

                if perf_debug() {
                    eprintln!(
                        "[PERF] terrain    T[{}/{}] patches {start}–{end}/{total_patches}  {batch_ms:.2}ms",
                        *tile_i + 1,
                        n_tiles
                    );
                } else if batch_ms > SLOW_BATCH_MS {
                    eprintln!(
                        "[PERF] SLOW terrain T[{}/{}] patches {start}–{end}: {batch_ms:.1}ms",
                        *tile_i + 1,
                        n_tiles
                    );
                }

                log.push(format!(
                    "→ T[{}/{}] patches {start}–{end}/{total_patches}",
                    *tile_i + 1,
                    n_tiles
                ));
                for name in terrain_texture_names(&slot.geometry, start, end) {
                    log.push(format!("  · TERRTEX {name}"));
                }
                *patch_i = end;

                // Fin del tile actual.
                if end >= total_patches {
                    if perf_debug() {
                        eprintln!(
                            "[PERF] terrain    T[{}/{}] done  (total_phase {:.3}s)",
                            *tile_i + 1,
                            n_tiles,
                            perf.elapsed_phase()
                        );
                    }
                    *tile_i += 1;
                    *patch_i = 0;
                }

                update_tile_counter(&screen, &mut texts, *tile_i, n_tiles);
                let frac = terrain_fraction(*tile_i, n_tiles);
                progress.set(
                    format!(
                        "Terreno… (tile {}/{n_tiles}, patch {end}/{total_patches})",
                        *tile_i + 1
                    ),
                    frac,
                );
                break 'progress;
            }

            // ── 3. Vía (un tile por frame) ───────────────────────────────
            LoadStage::Track {
                index,
                obj_ctx,
                slots,
                tile_i,
                height_index,
                height_index_builds,
            } => {
                let n_tiles = slots.len();
                if *tile_i >= n_tiles {
                    let elapsed = perf.elapsed_phase();
                    perf.secs_track = elapsed;
                    perf.reset_phase();
                    if perf_debug() {
                        perf.print_phase(
                            "Track",
                            &format!(
                                "{n_tiles} tiles, height_index_builds={height_index_builds}"
                            ),
                            elapsed,
                        );
                    }
                    debug_assert!(
                        n_tiles == 0 || *height_index_builds == 1,
                        "TileHeightIndex must be built once per Track batch (#63), got {height_index_builds} for {n_tiles} tiles"
                    );
                    // Preparar Objects.
                    let slots = std::mem::take(slots);
                    let filtered = build_filtered_objects(&slots, 0);
                    *stage = LoadStage::Objects {
                        index: index.clone(),
                        obj_ctx: obj_ctx.clone(),
                        slots,
                        tile_i: 0,
                        obj_i: 0,
                        filtered,
                    };
                    progress.set("Objetos…", TRACK_END_FRACTION);
                    continue 'progress;
                }

                let center = stream_config.center_tile;
                if height_index.is_none() {
                    *height_index = Some(crate::tdb_track::TileHeightIndex::from_tile_heights(
                        slots.iter().map(|s| {
                            (
                                s.geometry.tile_x,
                                s.geometry.tile_z,
                                &s.geometry.height,
                            )
                        }),
                        center,
                    ));
                    *height_index_builds += 1;
                }
                let height_index = height_index
                    .as_ref()
                    .expect("height_index populated for Track batch");

                let slot = &slots[*tile_i];
                let segs = slot.track.segment_count();
                let track_objs = crate::objects::count_track_objects(&slot.objects);
                let shaped = tdb_track
                    .as_ref()
                    .map(|tdb| {
                        crate::tdb_track::collect_tdb_shaped_chords(
                            &tdb.ctx,
                            center.0,
                            center.1,
                            tdb.grid_radius,
                        )
                    })
                    .unwrap_or_default();
                let batch_t = Instant::now();
                let suppressed = crate::objects::tile_suppresses_tdb_ribbon(&slot.objects);
                let bypass = suppressed && crate::world_spawn::tdb_procedural_forced();
                if suppressed && !bypass {
                    if perf_debug() {
                        eprintln!(
                            "[PERF] track      T[{}/{}] omitida ({track_objs} TrackObj UKFS)",
                            *tile_i + 1,
                            n_tiles
                        );
                    }
                    log.push(format!(
                        "→ Vía T[{}/{}]: omitida ({track_objs} TrackObj UKFS)",
                        *tile_i + 1,
                        n_tiles
                    ));
                } else if bypass {
                    log.push(format!(
                        "→ Vía T[{}/{}]: TrackObj presente, .tdb procedural forzado",
                        *tile_i + 1,
                        n_tiles
                    ));
                }
                spawn_tile_track(
                    &mut commands,
                    &mut load_assets.meshes,
                    &mut load_assets.materials,
                    &mut load_assets.or_materials,
                    &mut load_assets.images,
                    Some(index),
                    Some(obj_ctx),
                    &route.0,
                    &msts_root.0,
                    tdb_track.as_deref().map(|r| &r.ctx),
                    &shaped,
                    &slot.track,
                    &slot.objects,
                    center,
                    &height_index,
                    slot.world_offset,
                    materials_lit,
                    slot.geometry.tile_x,
                    slot.geometry.tile_z,
                    &mut tex_stats,
                    &texture_env,
                    perf.viewer_pos,
                    Some(track_stats.as_mut()),
                );
                if !suppressed || bypass {
                    let batch_ms = batch_t.elapsed().as_secs_f64() * 1000.0;
                    if perf_debug() {
                        eprintln!(
                            "[PERF] track      T[{}/{}] {segs} segs  {batch_ms:.2}ms",
                            *tile_i + 1,
                            n_tiles
                        );
                    } else if batch_ms > SLOW_BATCH_MS {
                        eprintln!(
                            "[PERF] SLOW track T[{}/{}] {segs} segs: {batch_ms:.1}ms",
                            *tile_i + 1,
                            n_tiles
                        );
                    }
                    log.push(format!(
                        "→ Vía T[{}/{}]: {segs} segmentos",
                        *tile_i + 1,
                        n_tiles
                    ));
                }
                *tile_i += 1;
                progress.set(
                    format!("Vía… (tile {}/{n_tiles})", *tile_i),
                    TRACK_END_FRACTION * (*tile_i as f32 / n_tiles as f32),
                );
                break 'progress;
            }

            // ── 4. Objetos (por lotes, tile a tile) ──────────────────────
            LoadStage::Objects {
                index,
                obj_ctx,
                slots,
                tile_i,
                obj_i,
                filtered,
            } => {
                let n_tiles = slots.len();
                let total_objs_all: usize = slots
                    .iter()
                    .map(|s| count_filtered_objects(&s.objects))
                    .sum();

                // Fin de tile actual (shapes completos → bosque/agua del tile).
                if *obj_i >= filtered.len() {
                    let finished = *tile_i;
                    spawn_scenery_for_slot(
                        &mut commands,
                        &mut load_assets.meshes,
                        &mut load_assets.materials,
                        &mut load_assets.images,
                        index,
                        obj_ctx,
                        &route.0,
                        &msts_root.0,
                        &slots[finished],
                        &mut tex_stats,
                        &texture_env,
                        &mut log,
                        materials_lit,
                        Some(track_stats.as_mut()),
                        tdb_track.as_deref().map(|r| &r.ctx),
                    );
                    *tile_i += 1;
                    *obj_i = 0;
                    if *tile_i >= n_tiles {
                        progress.set("Finalizando…", 0.98);
                        log.push("→ Luces y cámara");
                        let initial_tile_coords = slots
                            .iter()
                            .map(|s| (s.geometry.tile_x, s.geometry.tile_z))
                            .collect();
                        *stage = LoadStage::Finished {
                            index: index.clone(),
                            obj_ctx: obj_ctx.clone(),
                            initial_tile_coords,
                        };
                        continue 'progress;
                    }
                    *filtered = build_filtered_objects(slots, *tile_i);
                    log.push(format!(
                        "→ Objetos T[{}/{}]: {} shapes",
                        *tile_i + 1,
                        n_tiles,
                        filtered.len()
                    ));
                }

                if filtered.is_empty() {
                    // Tile sin shapes → igual puede tener bosque/agua.
                    spawn_scenery_for_slot(
                        &mut commands,
                        &mut load_assets.meshes,
                        &mut load_assets.materials,
                        &mut load_assets.images,
                        index,
                        obj_ctx,
                        &route.0,
                        &msts_root.0,
                        &slots[*tile_i],
                        &mut tex_stats,
                        &texture_env,
                        &mut log,
                        materials_lit,
                        Some(track_stats.as_mut()),
                        tdb_track.as_deref().map(|r| &r.ctx),
                    );
                    *tile_i += 1;
                    *obj_i = 0;
                    if *tile_i >= n_tiles {
                        progress.set("Finalizando…", 0.98);
                        log.push("→ Luces y cámara");
                        let initial_tile_coords = slots
                            .iter()
                            .map(|s| (s.geometry.tile_x, s.geometry.tile_z))
                            .collect();
                        *stage = LoadStage::Finished {
                            index: index.clone(),
                            obj_ctx: obj_ctx.clone(),
                            initial_tile_coords,
                        };
                        continue 'progress;
                    }
                    *filtered = build_filtered_objects(slots, *tile_i);
                    break 'progress;
                }

                let start = *obj_i;
                let end = (start + OBJECTS_PER_FRAME).min(filtered.len());
                let batch = &filtered[start..end];
                let tile_offset = slots[*tile_i].world_offset;

                let batch_t = Instant::now();
                spawn_objects_batch(
                    &mut commands,
                    &mut load_assets.meshes,
                    &mut load_assets.materials,
                    &mut load_assets.or_materials,
                    &mut load_assets.images,
                    index,
                    obj_ctx,
                    &route.0,
                    &msts_root.0,
                    batch,
                    &mut tex_stats,
                    tile_offset,
                    &texture_env,
                    perf.viewer_pos,
                    slots[*tile_i].geometry.tile_x,
                    slots[*tile_i].geometry.tile_z,
                    materials_lit,
                    Some(track_stats.as_mut()),
                    tdb_track.as_deref().map(|r| &r.ctx),
                );
                let batch_ms = batch_t.elapsed().as_secs_f64() * 1000.0;
                perf.object_batches += 1;

                if perf_debug() {
                    eprintln!(
                        "[PERF] objects    T[{}/{}] {start}–{end}/{} shapes  {batch_ms:.2}ms",
                        *tile_i + 1,
                        n_tiles,
                        filtered.len()
                    );
                } else if batch_ms > SLOW_BATCH_MS {
                    let names = unique_shape_names(batch);
                    eprintln!(
                        "[PERF] SLOW objs  T[{}/{}] {start}–{end}: {batch_ms:.1}ms  {:?}",
                        *tile_i + 1,
                        n_tiles,
                        format_name_list(&names, 3)
                    );
                }

                log.push(format!(
                    "→ Objs T[{}/{}] {start}–{end}/{}",
                    *tile_i + 1,
                    n_tiles,
                    filtered.len()
                ));
                let names = unique_shape_names(batch);
                if !names.is_empty() {
                    log.push(format!(
                        "  · {}",
                        format_name_list(&names, LOG_NAME_PREVIEW)
                    ));
                }
                *obj_i = end;

                // Progreso global de objetos.
                let done_objs: usize = slots[..*tile_i]
                    .iter()
                    .map(|s| count_filtered_objects(&s.objects))
                    .sum::<usize>()
                    + end;
                progress.set(
                    format!("Objetos… (tile {}/{n_tiles})", *tile_i + 1),
                    TRACK_END_FRACTION
                        + (1.0 - TRACK_END_FRACTION)
                            * (done_objs as f32 / total_objs_all.max(1) as f32),
                );
                break 'progress;
            }

            // ── 5. Done ──────────────────────────────────────────────────
            LoadStage::Finished { .. } => {
                let elapsed_objects = perf.elapsed_phase();
                perf.secs_objects = elapsed_objects;
                break 'progress;
            }
        }
    }
}

/// Tras `LoadStage::Finished`: activa streaming y pasa a `Playing`.
#[derive(SystemParam)]
pub struct WorldLoadFinish<'w> {
    stream_config: Res<'w, TileStreamConfig>,
    catalog: Res<'w, TileCatalog>,
    terrain_saved: Option<Res<'w, SavedTerrainCtx>>,
    consist_plan: Option<Res<'w, StaticConsistPlan>>,
    player_start: Res<'w, PlayerStartPoseResource>,
    route: Res<'w, crate::RouteDir>,
    msts_root: Res<'w, crate::MstsRootDir>,
    cycle: ResMut<'w, openrailsrs_bevy_scenery::ScenerySpawnCycle>,
    progress_msg: MessageWriter<'w, openrailsrs_bevy_scenery::ScenerySpawnProgress>,
}

#[allow(clippy::too_many_arguments)]
pub fn finish_world_load(
    mut commands: Commands,
    stage: Res<LoadStage>,
    mut next_state: ResMut<NextState<AppState>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut or_materials: ResMut<Assets<OrSceneryMaterial>>,
    mut images: ResMut<Assets<Image>>,
    screen: Res<LoadingScreen>,
    loading_cams: Query<Entity, With<Camera2d>>,
    perf: Res<LoadPerfState>,
    mut finish: WorldLoadFinish,
    texture_env: Res<crate::textures::TextureEnvironment>,
    mut tex_stats: ResMut<TextureLoadStats>,
    track_stats: Res<TrackSpawnStats>,
    mut load_diag: ResMut<MstsLoadDiagnostics>,
) {
    let WorldLoadFinish {
        stream_config,
        catalog,
        terrain_saved,
        consist_plan,
        player_start,
        route,
        msts_root,
        ref mut cycle,
        ref mut progress_msg,
    } = finish;
    let LoadStage::Finished {
        index,
        obj_ctx,
        initial_tile_coords,
    } = stage.as_ref()
    else {
        return;
    };

    // Guard: LoadStage::Finished can be observed only once per cycle (#52).
    if !cycle.active {
        return;
    }
    cycle.set_phase(openrailsrs_bevy_scenery::ScenerySpawnPhase::Ready);
    progress_msg.write(openrailsrs_bevy_scenery::ScenerySpawnProgress::new(
        cycle,
        1.0,
        "ready",
    ));
    cycle.note_spawn_work();
    cycle.finish();

    if let Some(terrain) = terrain_saved.as_ref() {
        let mut obj_ctx = obj_ctx.clone();

        if let (Some(plan), Some(pose)) = (consist_plan.as_ref(), player_start.0) {
            spawn_static_consist(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut or_materials,
                &mut images,
                index,
                &mut obj_ctx,
                &route.0,
                &msts_root.0,
                plan,
                pose,
                &texture_env,
                &mut tex_stats,
            );
        }

        load_diag.merge_from(&obj_ctx.load_diag);
        commands.insert_resource(StreamWorldAssets {
            index: index.clone(),
            terrain_ctx: terrain.0.clone(),
            obj_ctx,
            materials_lit: perf.materials_lit,
        });
        if stream_config.streaming_enabled() {
            commands.insert_resource(TileStreamState {
                loaded: initial_loaded_tiles(&stream_config, initial_tile_coords),
                last_camera_tile: Some(stream_config.center_tile),
            });
            info!(
                "streaming activo: {} tiles iniciales, {} en catálogo",
                initial_tile_coords.len(),
                catalog.entries.len()
            );
        }
        commands.remove_resource::<SavedTerrainCtx>();
    } else if stream_config.streaming_enabled() {
        warn!("streaming: SavedTerrainCtx ausente — tiles extra no se cargarán");
        load_diag.merge_from(&obj_ctx.load_diag);
    } else {
        load_diag.merge_from(&obj_ctx.load_diag);
    }
    load_diag.ingest_texture_stats(
        tex_stats.resolved,
        tex_stats.unresolved,
        tex_stats.decode_failed,
        &tex_stats.unresolved_samples,
        &tex_stats.decode_failed_samples,
    );
    finish_loading(
        &mut commands,
        &mut meshes,
        &mut materials,
        &screen,
        &loading_cams,
        &mut next_state,
        &tex_stats,
        &track_stats,
        &load_diag,
        perf.scene_side_m,
        &perf,
        !texture_env.night,
    );
}

/// Fracción de progreso al inicio de la fase de vía (después del terreno).
const TRACK_END_FRACTION: f32 = 0.35;

fn terrain_fraction(tile_i: usize, n_tiles: usize) -> f32 {
    0.05 + (TRACK_END_FRACTION - 0.05) * (tile_i as f32 / n_tiles.max(1) as f32)
}

fn update_tile_counter(
    screen: &LoadingScreen,
    texts: &mut Query<&mut Text>,
    current: usize,
    total: usize,
) {
    if let Ok(mut t) = texts.get_mut(screen.tile_counter) {
        *t = Text::new(if total > 1 {
            format!("Tile {}/{total}", current.min(total))
        } else {
            String::new()
        });
    }
}

// ─── Helpers de filtrado de objetos ──────────────────────────────────────────

fn build_filtered_objects(slots: &[TileSlot], tile_i: usize) -> Vec<ObjectMarker> {
    if tile_i >= slots.len() {
        return Vec::new();
    }
    slots[tile_i]
        .objects
        .iter()
        .filter(|o| crate::objects::object_wants_shape_mesh(o))
        .cloned()
        .collect()
}

fn count_filtered_objects(objs: &[ObjectMarker]) -> usize {
    objs.iter()
        .filter(|o| crate::objects::object_wants_shape_mesh(o))
        .count()
}

#[allow(clippy::too_many_arguments)]
fn spawn_scenery_for_slot(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    index: &AssetIndex,
    obj_ctx: &mut ObjectSpawnCtx,
    route_dir: &std::path::Path,
    msts_root: &std::path::Path,
    slot: &TileSlot,
    tex_stats: &mut TextureLoadStats,
    texture_env: &crate::textures::TextureEnvironment,
    log: &mut LoadLog,
    materials_lit: bool,
    track_stats: Option<&mut TrackSpawnStats>,
    tdb: Option<&crate::tdb_track::TdbContext>,
) {
    let trackobj = crate::world_spawn::spawn_trackobj_procedural_for_objects(
        commands,
        meshes,
        materials,
        &slot.objects,
        slot.world_offset,
        &index.tsection,
        tdb,
        index,
        route_dir,
        msts_root,
        materials_lit,
        slot.geometry.tile_x,
        slot.geometry.tile_z,
        track_stats,
    );
    let (forests, waters) = crate::scenery::spawn_tile_scenery(
        commands,
        meshes,
        materials,
        images,
        index,
        obj_ctx,
        route_dir,
        msts_root,
        &slot.objects,
        &slot.geometry.height,
        slot.geometry.tile_x,
        slot.geometry.tile_z,
        slot.world_offset,
        tex_stats,
        texture_env,
        materials_lit,
    );
    let dyntrack = crate::dyntrack::spawn_tile_dyntrack(
        commands,
        meshes,
        materials,
        &slot.objects,
        slot.world_offset,
        materials_lit,
        slot.geometry.tile_x,
        slot.geometry.tile_z,
    );
    let transfers = crate::transfer::spawn_tile_transfers(
        commands,
        meshes,
        materials,
        images,
        index,
        route_dir,
        msts_root,
        &slot.objects,
        &slot.geometry.height,
        slot.geometry.tile_x,
        slot.geometry.tile_z,
        slot.world_offset,
        tex_stats,
        texture_env,
        materials_lit,
    );
    if forests > 0 || waters > 0 || dyntrack > 0 || transfers > 0 || trackobj > 0 {
        log.push(format!(
            "→ Escena: {forests} bosque(s), {waters} agua, {dyntrack} dyntrack, {transfers} transfer, {trackobj} TrackObj proc."
        ));
    }
}

// ─── Finalización ────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn finish_loading(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    screen: &LoadingScreen,
    loading_cams: &Query<Entity, With<Camera2d>>,
    next_state: &mut NextState<AppState>,
    tex_stats: &TextureLoadStats,
    track_stats: &TrackSpawnStats,
    load_diag: &MstsLoadDiagnostics,
    side_m: f32,
    perf: &LoadPerfState,
    night: bool,
) {
    let total = perf.elapsed_total();
    let misc = total - perf.secs_indexing - perf.secs_terrain - perf.secs_track - perf.secs_objects;
    eprintln!("[PERF] ══ Carga completa ══  total {total:.3}s");
    eprintln!(
        "[PERF]   indexing : {:7.3}s  ({:.1}%)",
        perf.secs_indexing,
        perf.secs_indexing / total * 100.0
    );
    eprintln!(
        "[PERF]   terrain  : {:7.3}s  ({:.1}%)  {} batches",
        perf.secs_terrain,
        perf.secs_terrain / total * 100.0,
        perf.terrain_batches
    );
    eprintln!(
        "[PERF]   track    : {:7.3}s  ({:.1}%)",
        perf.secs_track,
        perf.secs_track / total * 100.0
    );
    eprintln!(
        "[PERF]   objects  : {:7.3}s  ({:.1}%)  {} batches",
        perf.secs_objects,
        perf.secs_objects / total * 100.0,
        perf.object_batches
    );
    if misc > 0.01 {
        eprintln!(
            "[PERF]   misc/wait: {:7.3}s  ({:.1}%)",
            misc,
            misc / total * 100.0
        );
    }
    tex_stats.report();
    track_stats.report();
    load_diag.report();
    load_diag.maybe_write_audit_env();
    let extent = crate::SceneExtent { side_m };
    crate::sky::spawn_scene_sky(commands, meshes, materials, &extent, perf.tile_count, night);
    commands.insert_resource(ClearColor(crate::sky::sky_clear_color(night)));
    spawn_play_camera(
        commands,
        side_m,
        perf.player_start,
        &extent,
        perf.tile_count,
        night,
    );
    spawn_ui_overlay_camera(commands);
    spawn_debug_hud(commands, perf.hud_enabled);
    commands.entity(screen.root).despawn();
    for e in loading_cams {
        commands.entity(e).despawn();
    }
    commands.remove_resource::<LoadingScreen>();
    commands.remove_resource::<LoadStage>();
    commands.remove_resource::<LoadLog>();
    commands.remove_resource::<LoadProgress>();
    commands.remove_resource::<TextureLoadStats>();
    commands.remove_resource::<TrackSpawnStats>();
    next_state.set(AppState::Playing);
    info!("carga completa — escena 3D lista");
}

fn spawn_play_camera(
    commands: &mut Commands,
    side_m: f32,
    start: Option<PlayerStartPose>,
    extent: &crate::SceneExtent,
    tile_count: usize,
    night: bool,
) {
    let side = side_m.max(256.0);
    let far = side * 16.0;
    let fog = crate::sky::scene_distance_fog(extent, tile_count, night);

    if let Some(pose) = start {
        // Mirada ligeramente hacia abajo (cabina sobre el raíl).
        let rotation = Quat::from_euler(EulerRot::YXZ, pose.yaw_rad, -0.07, 0.0);
        commands.spawn((
            Camera3d::default(),
            FlyCamera,
            Projection::Perspective(PerspectiveProjection { far, ..default() }),
            Transform::from_translation(pose.position).with_rotation(rotation),
            fog,
            Name::new("fly_camera"),
        ));
        return;
    }

    let eye = Vec3::new(0.0, side * 0.45, side * 0.75);
    commands.spawn((
        Camera3d::default(),
        FlyCamera,
        Projection::Perspective(PerspectiveProjection { far, ..default() }),
        Transform::from_translation(eye).looking_at(Vec3::ZERO, Vec3::Y),
        fog,
        Name::new("fly_camera"),
    ));
}

// ─── Utilidades de nombres ────────────────────────────────────────────────────

fn terrain_texture_names(tile: &TileGeometry, from: usize, to: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for patch in tile.patches.iter().take(to).skip(from) {
        if let Some(name) = &patch.texture {
            if seen.insert(name.clone()) {
                names.push(name.clone());
            }
        }
    }
    names.sort_unstable();
    names
}

fn unique_shape_names(batch: &[ObjectMarker]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for obj in batch {
        if let Some(file) = &obj.file_name {
            let base = basename(file);
            if seen.insert(base.to_string()) {
                names.push(base.to_string());
            }
        }
    }
    names.sort_unstable();
    names
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn format_name_list(names: &[String], max: usize) -> String {
    if names.is_empty() {
        return String::new();
    }
    let mut out = names
        .iter()
        .take(max)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > max {
        out.push_str(&format!(" … (+{} más)", names.len() - max));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_log_keeps_tail() {
        let mut log = LoadLog::new(3);
        log.push("a");
        log.push("b");
        log.push("c");
        log.push("d");
        assert_eq!(log.body(), "b\nc\nd");
    }

    #[test]
    fn format_name_list_truncates() {
        let names = vec!["a.s".into(), "b.s".into(), "c.s".into()];
        assert_eq!(format_name_list(&names, 2), "a.s, b.s … (+1 más)");
    }
}
