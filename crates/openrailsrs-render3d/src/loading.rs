//! Pantalla de carga y spawn progresivo del mundo (evita congelar la ventana).

use std::collections::HashSet;
use std::time::Instant;

use bevy::ecs::query::Without;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};

use crate::objects::ObjectMarker;
use crate::terrain::TileGeometry;
use crate::world_spawn::{
    AssetIndex, ObjectSpawnCtx, TerrainSpawnCtx, TextureLoadStats, spawn_objects_batch,
    spawn_terrain_patches, spawn_track, spawn_world_lights,
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
pub(crate) struct TileSlot {
    geometry: crate::terrain::TileGeometry,
    world_offset: Vec3,
    track: crate::track::TrackRibbon,
    objects: Vec<ObjectMarker>,
}

/// Estado interno del spawn por fases.
#[derive(Resource)]
pub enum LoadStage {
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
        slots: Vec<TileSlot>,
        tile_i: usize,
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
    Done,
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
}

impl LoadPerfState {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            total_start: now,
            phase_start: now,
            secs_indexing: 0.0,
            secs_terrain: 0.0,
            secs_track: 0.0,
            secs_objects: 0.0,
            terrain_batches: 0,
            object_batches: 0,
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
                font_size: 28.0,
                ..default()
            },
            TextColor(Color::srgb(0.92, 0.94, 0.98)),
        ))
        .id();

    let tile_counter = commands
        .spawn((
            Text::new(""),
            TextFont {
                font_size: 14.0,
                ..default()
            },
            TextColor(Color::srgb(0.55, 0.65, 0.85)),
        ))
        .id();

    let status = commands
        .spawn((
            Text::new("Cargando…"),
            TextFont {
                font_size: 17.0,
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
                font_size: 12.0,
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
        bevy::render::camera::CameraRenderGraph::new(bevy::core_pipeline::core_2d::graph::Core2d),
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

pub fn begin_load_stage(
    mut commands: Commands,
    tiles_res: Res<crate::TilesToRender>,
    route: Res<crate::RouteDir>,
    msts_root: Res<crate::MstsRootDir>,
    mut progress: ResMut<LoadProgress>,
    mut log: ResMut<LoadLog>,
) {
    let n = tiles_res.0.len();
    progress.set(format!("Indexando shapes y texturas… ({n} tiles)"), 0.02);
    log.push(format!("→ Indexando SHAPES y TEXTURES… ({n} tiles)"));

    // Clonar los datos de cada tile en TileSlots propios para esta máquina de estados.
    let mut slots: Vec<TileSlot> = Vec::with_capacity(n);
    for entry in tiles_res.0.iter() {
        slots.push(TileSlot {
            geometry: entry.geometry.clone(),
            world_offset: entry.world_offset,
            track: entry.track.clone(),
            objects: entry.objects.clone(),
        });
    }

    commands.insert_resource(TextureLoadStats::default());
    commands.insert_resource(LoadPerfState::new());
    let route_dir = route.0.clone();
    let msts_dir = msts_root.0.clone();
    let task =
        AsyncComputeTaskPool::get().spawn(async move { AssetIndex::build(&route_dir, &msts_dir) });
    commands.insert_resource(LoadStage::Indexing { task, slots });
    if perf_debug() {
        eprintln!("[PERF] begin_load_stage: {n} tiles → indexando shapes/texturas…");
    }
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

#[allow(clippy::too_many_arguments)]
pub fn progressive_world_load(
    mut commands: Commands,
    mut stage: ResMut<LoadStage>,
    mut progress: ResMut<LoadProgress>,
    mut log: ResMut<LoadLog>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    route: Res<crate::RouteDir>,
    msts_root: Res<crate::MstsRootDir>,
    screen: Res<LoadingScreen>,
    loading_cams: Query<Entity, With<Camera2d>>,
    mut tex_stats: ResMut<TextureLoadStats>,
    mut next_state: ResMut<NextState<AppState>>,
    extent: Res<crate::SceneExtent>,
    mut texts: Query<&mut Text>,
    mut perf: ResMut<LoadPerfState>,
) {
    'progress: loop {
        match &mut *stage {
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
                    let ctx = TerrainSpawnCtx::new(&mut materials);
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
                    *stage = LoadStage::Track {
                        index: index.clone(),
                        slots,
                        tile_i: 0,
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
                    &mut meshes,
                    ctx,
                    &mut materials,
                    &mut images,
                    &route.0,
                    &slot.geometry,
                    start,
                    end,
                    slot.world_offset,
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
                slots,
                tile_i,
            } => {
                let n_tiles = slots.len();
                if *tile_i >= n_tiles {
                    let elapsed = perf.elapsed_phase();
                    perf.secs_track = elapsed;
                    perf.reset_phase();
                    if perf_debug() {
                        perf.print_phase("Track", &format!("{n_tiles} tiles"), elapsed);
                    }
                    // Preparar Objects.
                    let slots = std::mem::take(slots);
                    let obj_ctx = ObjectSpawnCtx::new(&mut materials);
                    let filtered = build_filtered_objects(&slots, 0);
                    *stage = LoadStage::Objects {
                        index: index.clone(),
                        obj_ctx,
                        slots,
                        tile_i: 0,
                        obj_i: 0,
                        filtered,
                    };
                    progress.set("Objetos…", TRACK_END_FRACTION);
                    continue 'progress;
                }

                let slot = &slots[*tile_i];
                let segs = slot.track.segment_count();
                let batch_t = Instant::now();
                spawn_track(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &slot.track,
                    slot.world_offset,
                );
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

                // Fin de tile actual.
                if *obj_i >= filtered.len() {
                    *tile_i += 1;
                    *obj_i = 0;
                    if *tile_i >= n_tiles {
                        progress.set("Finalizando…", 0.98);
                        log.push("→ Luces y cámara");
                        *stage = LoadStage::Done;
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
                    // Tile sin objetos → avanzar.
                    *tile_i += 1;
                    *obj_i = 0;
                    if *tile_i >= n_tiles {
                        progress.set("Finalizando…", 0.98);
                        log.push("→ Luces y cámara");
                        *stage = LoadStage::Done;
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
                    &mut meshes,
                    &mut materials,
                    &mut images,
                    index,
                    obj_ctx,
                    &route.0,
                    &msts_root.0,
                    batch,
                    &mut tex_stats,
                    tile_offset,
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
            LoadStage::Done => {
                let elapsed_objects = perf.elapsed_phase();
                perf.secs_objects = elapsed_objects;
                finish_loading(
                    &mut commands,
                    &screen,
                    &loading_cams,
                    &mut next_state,
                    &tex_stats,
                    extent.side_m,
                    &perf,
                );
                return;
            }
        }
    }
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
        .filter(|o| crate::objects::wants_shape_mesh(o.kind, o.file_name.as_deref()))
        .cloned()
        .collect()
}

fn count_filtered_objects(objs: &[ObjectMarker]) -> usize {
    objs.iter()
        .filter(|o| crate::objects::wants_shape_mesh(o.kind, o.file_name.as_deref()))
        .count()
}

// ─── Finalización ────────────────────────────────────────────────────────────

fn finish_loading(
    commands: &mut Commands,
    screen: &LoadingScreen,
    loading_cams: &Query<Entity, With<Camera2d>>,
    next_state: &mut NextState<AppState>,
    tex_stats: &TextureLoadStats,
    side_m: f32,
    perf: &LoadPerfState,
) {
    // Resumen de performance.
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
    spawn_world_lights(commands);
    spawn_play_camera(commands, side_m);
    commands.entity(screen.root).despawn();
    for e in loading_cams {
        commands.entity(e).despawn();
    }
    commands.remove_resource::<LoadingScreen>();
    commands.remove_resource::<LoadStage>();
    commands.remove_resource::<LoadLog>();
    commands.remove_resource::<LoadProgress>();
    commands.remove_resource::<TextureLoadStats>();
    next_state.set(AppState::Playing);
    info!("carga completa — escena 3D lista");
}

fn spawn_play_camera(commands: &mut Commands, side_m: f32) {
    let side = side_m.max(256.0);
    let eye = Vec3::new(0.0, side * 0.45, side * 0.75);
    commands.spawn((
        Camera3d::default(),
        bevy::render::camera::CameraRenderGraph::new(bevy::core_pipeline::core_3d::graph::Core3d),
        Projection::Perspective(PerspectiveProjection {
            far: side * 16.0, // ver más lejos cuando hay muchos tiles
            ..default()
        }),
        Transform::from_translation(eye).looking_at(Vec3::ZERO, Vec3::Y),
        AmbientLight {
            color: Color::srgb(0.85, 0.90, 1.0),
            brightness: 350.0,
            ..default()
        },
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
