//! Pantalla de carga y spawn progresivo del mundo (evita congelar la ventana).

use std::collections::HashSet;

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
}

/// Estado interno del spawn por fases.
#[derive(Resource)]
pub enum LoadStage {
    /// Indexando SHAPES/TEXTURES (hilo en background).
    Indexing {
        task: Task<AssetIndex>,
    },
    /// Terreno texturizado.
    Terrain {
        index: AssetIndex,
        ctx: TerrainSpawnCtx,
        patch_i: usize,
    },
    /// Cinta de vía.
    Track {
        index: AssetIndex,
    },
    /// Objetos `.s` en lotes.
    Objects {
        index: AssetIndex,
        obj_ctx: ObjectSpawnCtx,
        markers: Vec<ObjectMarker>,
        cursor: usize,
    },
    Done,
}

const TERRAIN_PATCHES_PER_FRAME: usize = 32;
const OBJECTS_PER_FRAME: usize = 40;
const LOG_MAX_LINES: usize = 14;
const LOG_NAME_PREVIEW: usize = 6;

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
                row_gap: Val::Px(16.0),
                padding: UiRect::all(Val::Px(24.0)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.12, 0.14, 0.18)),
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

    let status = commands
        .spawn((
            Text::new("Cargando…"),
            TextFont {
                font_size: 18.0,
                ..default()
            },
            TextColor(Color::srgb(0.75, 0.80, 0.88)),
        ))
        .id();

    let bar_bg = commands
        .spawn((
            Node {
                width: Val::Px(480.0),
                height: Val::Px(12.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.22, 0.24, 0.28)),
        ))
        .id();

    let bar_fill = commands
        .spawn((
            Node {
                width: Val::Percent(0.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.35, 0.62, 0.92)),
        ))
        .id();

    let log_panel = commands
        .spawn((
            Node {
                width: Val::Px(480.0),
                height: Val::Px(220.0),
                margin: UiRect::top(Val::Px(8.0)),
                padding: UiRect::all(Val::Px(12.0)),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.08, 0.09, 0.11, 0.92)),
        ))
        .id();

    let log = commands
        .spawn((
            Text::new(""),
            TextFont {
                font_size: 13.0,
                ..default()
            },
            TextColor(Color::srgb(0.62, 0.68, 0.76)),
        ))
        .id();

    commands.entity(log_panel).add_child(log);
    commands.entity(bar_bg).add_child(bar_fill);
    commands
        .entity(root)
        .add_children(&[title, status, bar_bg, log_panel]);

    commands.spawn((
        Camera2d,
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.12, 0.14, 0.18)),
            ..default()
        },
    ));

    commands.insert_resource(LoadingScreen {
        root,
        bar_fill,
        status,
        log,
    });
}

pub fn begin_load_stage(
    mut commands: Commands,
    route: Res<crate::RouteDir>,
    mut progress: ResMut<LoadProgress>,
    mut log: ResMut<LoadLog>,
) {
    progress.set("Indexando shapes y texturas…", 0.02);
    log.push("→ Indexando SHAPES y TEXTURES…");
    let route_dir = route.0.clone();
    commands.insert_resource(TextureLoadStats::default());
    let task = AsyncComputeTaskPool::get().spawn(async move { AssetIndex::build(&route_dir) });
    commands.insert_resource(LoadStage::Indexing { task });
}

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

#[allow(clippy::too_many_arguments)]
pub fn progressive_world_load(
    mut commands: Commands,
    mut stage: ResMut<LoadStage>,
    mut progress: ResMut<LoadProgress>,
    mut log: ResMut<LoadLog>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    tile: Res<crate::TileToRender>,
    track: Res<crate::TrackToRender>,
    objects: Res<crate::ObjectsToRender>,
    route: Res<crate::RouteDir>,
    screen: Res<LoadingScreen>,
    loading_cams: Query<Entity, With<Camera2d>>,
    mut tex_stats: ResMut<TextureLoadStats>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    'progress: loop {
        match &mut *stage {
            LoadStage::Indexing { task } => {
                if let Some(index) = check_ready(task) {
                    log.push(format!(
                        "✓ Índice: {} shapes, {} texturas",
                        index.shape_count(),
                        index.texture_count()
                    ));
                    progress.set("Preparando terreno…", 0.08);
                    let ctx = TerrainSpawnCtx::new(&mut materials);
                    *stage = LoadStage::Terrain {
                        index,
                        ctx,
                        patch_i: 0,
                    };
                }
                break 'progress;
            }
            LoadStage::Terrain {
                index,
                ctx,
                patch_i,
            } => {
                let total = tile.0.patches.len().max(1);
                let start = *patch_i;
                let end = (start + TERRAIN_PATCHES_PER_FRAME).min(total);
                spawn_terrain_patches(
                    &mut commands,
                    &mut meshes,
                    ctx,
                    &mut materials,
                    &mut images,
                    &route.0,
                    &tile.0,
                    start,
                    end,
                );
                log.push(format!("→ Terreno patches {start}–{end}/{total}"));
                for name in terrain_texture_names(&tile.0, start, end) {
                    log.push(format!("  · TERRTEX {name}"));
                }
                *patch_i = end;
                progress.set(
                    format!("Terreno… ({end}/{total} patches)"),
                    0.08 + 0.22 * (end as f32 / total as f32),
                );
                if end >= total {
                    progress.set("Vía…", 0.32);
                    *stage = LoadStage::Track {
                        index: index.clone(),
                    };
                }
                break 'progress;
            }
            LoadStage::Track { index } => {
                let segments = track.0.segment_count();
                spawn_track(&mut commands, &mut meshes, &mut materials, &track.0);
                log.push(format!("→ Vía: {segments} segmentos"));
                progress.set("Objetos…", 0.35);
                let markers: Vec<ObjectMarker> = objects
                    .0
                    .iter()
                    .filter(|o| crate::objects::wants_shape_mesh(o.kind, o.file_name.as_deref()))
                    .cloned()
                    .collect();
                let total = markers.len().max(1);
                let obj_ctx = ObjectSpawnCtx::new(&mut materials);
                *stage = LoadStage::Objects {
                    index: index.clone(),
                    obj_ctx,
                    markers,
                    cursor: 0,
                };
                progress.set(format!("Objetos… (0/{total})"), 0.35);
                log.push(format!("→ Objetos: {total} shapes por instanciar"));
                break 'progress;
            }
            LoadStage::Objects {
                index,
                obj_ctx,
                markers,
                cursor,
                ..
            } => {
                let total = markers.len();
                if total == 0 {
                    progress.set("Finalizando…", 0.98);
                    log.push("→ Luces y cámara");
                    *stage = LoadStage::Done;
                    continue 'progress;
                }
                let start = *cursor;
                let end = (start + OBJECTS_PER_FRAME).min(total);
                let batch = &markers[start..end];
                spawn_objects_batch(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut images,
                    index,
                    obj_ctx,
                    &route.0,
                    batch,
                    &mut tex_stats,
                );
                log.push(format!("→ Objetos {start}–{end}/{total}"));
                let names = unique_shape_names(batch);
                if !names.is_empty() {
                    log.push(format!(
                        "  · {}",
                        format_name_list(&names, LOG_NAME_PREVIEW)
                    ));
                }
                *cursor = end;
                progress.set(
                    format!("Objetos… ({end}/{total})"),
                    0.35 + 0.60 * (end as f32 / total as f32),
                );
                if end >= total {
                    progress.set("Finalizando…", 0.98);
                    log.push("→ Luces y cámara");
                    *stage = LoadStage::Done;
                    continue 'progress;
                }
                break 'progress;
            }
            LoadStage::Done => {
                finish_loading(
                    &mut commands,
                    &tile.0,
                    &screen,
                    &loading_cams,
                    &mut next_state,
                    &tex_stats,
                );
                return;
            }
        }
    }
}

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

fn finish_loading(
    commands: &mut Commands,
    tile: &TileGeometry,
    screen: &LoadingScreen,
    loading_cams: &Query<Entity, With<Camera2d>>,
    next_state: &mut NextState<AppState>,
    tex_stats: &TextureLoadStats,
) {
    tex_stats.report();
    spawn_world_lights(commands);
    spawn_play_camera(commands, tile);
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

fn spawn_play_camera(commands: &mut Commands, tile: &TileGeometry) {
    let side = tile.side_m.max(256.0);
    let eye = Vec3::new(0.0, side * 0.45, side * 0.75);
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            far: side * 8.0,
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
