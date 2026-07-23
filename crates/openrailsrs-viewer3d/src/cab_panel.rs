//! Driver cab instrumentation (Open Rails cabview analogue).
//!
//! Open Rails renders MSTS `CABVIEW3D` meshes plus `.cvf` control sprites. Until CVF
//! animation is ported, this module draws a HUD instrument board and loads the 3D cab
//! shell via [`crate::cab_view`] when `CABVIEW3D/*.s` exists on disk.

use bevy::prelude::*;
use openrailsrs_sim::CabTelemetry;

use crate::camera::CameraFollowMode;
use crate::live::LiveDrive;

const PANEL_WIDTH_PX: f32 = 520.0;
const PANEL_HEIGHT_PX: f32 = 210.0;
const FONT_SPEED: f32 = 42.0;
const FONT_LABEL: f32 = 13.0;
const FONT_BADGE: f32 = 15.0;

const COL_PANEL_BG: Color = Color::srgba(0.04, 0.05, 0.08, 0.94);
const COL_PANEL_BORDER: Color = Color::srgb(0.35, 0.45, 0.55);
const COL_TEXT: Color = Color::srgb(0.85, 0.88, 0.92);
const COL_MUTED: Color = Color::srgb(0.45, 0.52, 0.58);
const COL_SPEED: Color = Color::srgb(1.0, 0.82, 0.35);
const COL_WARN: Color = Color::srgb(1.0, 0.35, 0.35);
const COL_BADGE: Color = Color::srgb(0.55, 0.95, 0.65);
const COL_BADGE_BG: Color = Color::srgba(0.02, 0.12, 0.06, 0.88);
const COL_THROTTLE: Color = Color::srgb(0.25, 0.85, 0.45);
const COL_BRAKE: Color = Color::srgb(0.95, 0.3, 0.3);
const COL_BAR_TRACK: Color = Color::srgb(0.12, 0.16, 0.22);

/// Whether the cab panel is shown (toggle with `C` in live mode).
#[derive(Resource, Clone, Copy, Debug)]
pub struct CabPanelVisible {
    pub open: bool,
}

impl Default for CabPanelVisible {
    fn default() -> Self {
        Self { open: true }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CabPanelContent {
    pub speed_line: String,
    pub limit_line: String,
    pub detail_line: String,
    pub throttle_frac: f32,
    pub brake_frac: f32,
    pub overspeed: bool,
}

pub fn build_cab_panel_content(tel: &CabTelemetry) -> CabPanelContent {
    let speed_line = format!("{:.0}", tel.speed_kmh.round());
    let limit_line = if tel.limit_kmh.is_finite() {
        format!("LIM {:.0} km/h", tel.limit_kmh)
    } else {
        "LIM —".into()
    };
    let mut detail = format!(
        "THR {:.0}%   BRK {:.0}%   {:.0} kN",
        tel.throttle_pct, tel.brake_pct, tel.brake_force_kn
    );
    let dir_label = if tel.direction <= 0.25 {
        "REV"
    } else if tel.direction >= 0.75 {
        "FWD"
    } else {
        "NEU"
    };
    detail.push_str(&format!("   INV {dir_label}"));
    if let Some(rpm) = tel.diesel_rpm {
        detail.push_str(&format!("   RPM {:.0}", rpm));
    }
    if let Some(bar) = tel.boiler_bar {
        detail.push_str(&format!("   P {bar:.1} bar"));
    }
    CabPanelContent {
        speed_line,
        limit_line,
        detail_line: detail,
        throttle_frac: (tel.throttle_pct / 100.0).clamp(0.0, 1.0) as f32,
        brake_frac: (tel.brake_pct / 100.0).clamp(0.0, 1.0) as f32,
        overspeed: tel.overspeed,
    }
}

#[derive(Component)]
pub(crate) struct CabPanelRoot;

#[derive(Component)]
pub(crate) struct CabModeBadge;

#[derive(Component)]
pub(crate) struct CabSpeedText;

#[derive(Component)]
pub(crate) struct CabLimitText;

#[derive(Component)]
pub(crate) struct CabDetailText;

#[derive(Component)]
pub(crate) struct CabThrottleFill;

#[derive(Component)]
pub(crate) struct CabBrakeFill;

pub(crate) fn spawn_cab_panel(mut commands: Commands) {
    commands
        .spawn((
            CabPanelRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            ZIndex(95),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    CabModeBadge,
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(8.0),
                        left: Val::Percent(50.0),
                        margin: UiRect::left(Val::Px(-110.0)),
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(5.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(COL_BADGE_BG),
                    BorderColor::all(COL_BADGE),
                ))
                .with_children(|badge| {
                    badge.spawn((
                        Text::new("MODO CABINA"),
                        TextFont {
                            font_size: FontSize::Px(FONT_BADGE),
                            ..default()
                        },
                        TextColor(COL_BADGE),
                    ));
                });

            overlay
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(92.0),
                        left: Val::Percent(50.0),
                        margin: UiRect::left(Val::Px(-PANEL_WIDTH_PX * 0.5)),
                        width: Val::Px(PANEL_WIDTH_PX),
                        height: Val::Px(PANEL_HEIGHT_PX),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(12.0)),
                        row_gap: Val::Px(8.0),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(COL_PANEL_BG),
                    BorderColor::all(COL_PANEL_BORDER),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new("INSTRUMENTAL"),
                        TextFont {
                            font_size: FontSize::Px(FONT_LABEL),
                            ..default()
                        },
                        TextColor(COL_MUTED),
                    ));
                    panel
                        .spawn((Node {
                            width: Val::Percent(100.0),
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(16.0),
                            align_items: AlignItems::Center,
                            ..default()
                        },))
                        .with_children(|row| {
                            row.spawn((Node {
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(4.0),
                                ..default()
                            },))
                                .with_children(|col| {
                                    col.spawn((
                                        Text::new("MARCHA"),
                                        TextFont {
                                            font_size: FontSize::Px(10.0),
                                            ..default()
                                        },
                                        TextColor(COL_MUTED),
                                    ));
                                    col.spawn((
                                        Node {
                                            width: Val::Percent(100.0),
                                            height: Val::Px(72.0),
                                            justify_content: JustifyContent::FlexEnd,
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        BackgroundColor(COL_BAR_TRACK),
                                    ))
                                    .with_children(|track| {
                                        track.spawn((
                                            CabThrottleFill,
                                            Node {
                                                width: Val::Percent(55.0),
                                                height: Val::Percent(0.0),
                                                ..default()
                                            },
                                            BackgroundColor(COL_THROTTLE),
                                        ));
                                    });
                                });
                            row.spawn((Node {
                                flex_grow: 1.2,
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                row_gap: Val::Px(2.0),
                                ..default()
                            },))
                                .with_children(|col| {
                                    col.spawn((
                                        CabSpeedText,
                                        Text::new("0"),
                                        TextFont {
                                            font_size: FontSize::Px(FONT_SPEED),
                                            ..default()
                                        },
                                        TextColor(COL_SPEED),
                                    ));
                                    col.spawn((
                                        Text::new("km/h"),
                                        TextFont {
                                            font_size: FontSize::Px(FONT_LABEL),
                                            ..default()
                                        },
                                        TextColor(COL_MUTED),
                                    ));
                                    col.spawn((
                                        CabLimitText,
                                        Text::new("LIM —"),
                                        TextFont {
                                            font_size: FontSize::Px(FONT_LABEL),
                                            ..default()
                                        },
                                        TextColor(COL_MUTED),
                                    ));
                                });
                            row.spawn((Node {
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(4.0),
                                ..default()
                            },))
                                .with_children(|col| {
                                    col.spawn((
                                        Text::new("FRENO"),
                                        TextFont {
                                            font_size: FontSize::Px(10.0),
                                            ..default()
                                        },
                                        TextColor(COL_MUTED),
                                    ));
                                    col.spawn((
                                        Node {
                                            width: Val::Percent(100.0),
                                            height: Val::Px(72.0),
                                            justify_content: JustifyContent::FlexEnd,
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        BackgroundColor(COL_BAR_TRACK),
                                    ))
                                    .with_children(|track| {
                                        track.spawn((
                                            CabBrakeFill,
                                            Node {
                                                width: Val::Percent(55.0),
                                                height: Val::Percent(0.0),
                                                ..default()
                                            },
                                            BackgroundColor(COL_BRAKE),
                                        ));
                                    });
                                });
                        });
                    panel.spawn((
                        CabDetailText,
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(FONT_LABEL),
                            ..default()
                        },
                        TextColor(COL_TEXT),
                    ));
                });
        });
}

pub(crate) fn toggle_cab_panel(
    keys: Res<ButtonInput<KeyCode>>,
    follow: Res<CameraFollowMode>,
    mut visible: ResMut<CabPanelVisible>,
) {
    if *follow != CameraFollowMode::DriverCam {
        return;
    }
    if keys.just_pressed(KeyCode::KeyC) {
        visible.open = !visible.open;
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn update_cab_panel(
    follow: Res<CameraFollowMode>,
    visible: Res<CabPanelVisible>,
    live: Option<Res<LiveDrive>>,
    mut root: Query<&mut Visibility, With<CabPanelRoot>>,
    mut speed: Query<(&mut Text, &mut TextColor), With<CabSpeedText>>,
    mut limit: Query<&mut Text, (With<CabLimitText>, Without<CabSpeedText>)>,
    mut detail: Query<
        &mut Text,
        (
            With<CabDetailText>,
            Without<CabSpeedText>,
            Without<CabLimitText>,
        ),
    >,
    mut throttle: Query<
        &mut Node,
        (
            With<CabThrottleFill>,
            Without<CabBrakeFill>,
            Without<CabSpeedText>,
        ),
    >,
    mut brake: Query<
        &mut Node,
        (
            With<CabBrakeFill>,
            Without<CabThrottleFill>,
            Without<CabSpeedText>,
        ),
    >,
) {
    let Ok(mut vis) = root.single_mut() else {
        return;
    };
    let in_cab = *follow == CameraFollowMode::DriverCam;
    let Some(live) = live else {
        *vis = Visibility::Hidden;
        return;
    };
    if !in_cab || !visible.open {
        *vis = Visibility::Hidden;
        return;
    }
    *vis = Visibility::Visible;

    let content = build_cab_panel_content(&live.session.cab_telemetry());
    if let Ok((mut text, mut color)) = speed.single_mut() {
        **text = content.speed_line.clone();
        *color = if content.overspeed {
            TextColor(COL_WARN)
        } else {
            TextColor(COL_SPEED)
        };
    }
    if let Ok(mut text) = limit.single_mut() {
        **text = content.limit_line.clone();
    }
    if let Ok(mut text) = detail.single_mut() {
        **text = content.detail_line.clone();
    }
    if let Ok(mut node) = throttle.single_mut() {
        node.height = Val::Percent(content.throttle_frac * 100.0);
    }
    if let Ok(mut node) = brake.single_mut() {
        node.height = Val::Percent(content.brake_frac * 100.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cab_panel_formats_diesel_and_brake() {
        let tel = CabTelemetry {
            speed_kmh: 72.0,
            limit_kmh: 80.0,
            throttle_pct: 50.0,
            brake_pct: 25.0,
            direction: 1.0,
            horn_active: false,
            wiper_active: false,
            main_res_bar: 8.0,
            brake_pipe_bar: 4.5,
            brake_cyl_bar: 1.5,
            brake_force_kn: 120.0,
            diesel_rpm: Some(900.0),
            boiler_bar: None,
            overspeed: false,
        };
        let c = build_cab_panel_content(&tel);
        assert_eq!(c.speed_line, "72");
        assert!(c.detail_line.contains("RPM 900"));
        assert!(c.detail_line.contains("INV FWD"));
        assert!((c.throttle_frac - 0.5).abs() < 1e-6);
        assert!((c.brake_frac - 0.25).abs() < 1e-6);
    }

    #[test]
    fn cab_panel_marks_overspeed() {
        let tel = CabTelemetry {
            speed_kmh: 90.0,
            limit_kmh: 40.0,
            throttle_pct: 0.0,
            brake_pct: 0.0,
            direction: 0.5,
            horn_active: false,
            wiper_active: false,
            main_res_bar: 12.0,
            brake_pipe_bar: 5.0,
            brake_cyl_bar: 0.0,
            brake_force_kn: 0.0,
            diesel_rpm: None,
            boiler_bar: Some(12.0),
            overspeed: true,
        };
        let c = build_cab_panel_content(&tel);
        assert!(c.overspeed);
        assert!(c.detail_line.contains("P 12.0 bar"));
    }
}
