//! Driver cab panel (Fase C3): Bevy UI gauges fed by [`LiveDriveSession`].

use bevy::prelude::*;
use openrailsrs_sim::CabTelemetry;

use crate::live::LiveDrive;

const PANEL_WIDTH_PX: f32 = 260.0;
const PANEL_HEIGHT_PX: f32 = 168.0;
const FONT_SPEED: f32 = 28.0;
const FONT_LABEL: f32 = 12.0;

const COL_PANEL_BG: Color = Color::srgba(0.04, 0.05, 0.08, 0.92);
const COL_PANEL_BORDER: Color = Color::srgb(0.35, 0.45, 0.55);
const COL_TEXT: Color = Color::srgb(0.85, 0.88, 0.92);
const COL_MUTED: Color = Color::srgb(0.45, 0.52, 0.58);
const COL_SPEED: Color = Color::srgb(1.0, 0.82, 0.35);
const COL_WARN: Color = Color::srgb(1.0, 0.35, 0.35);
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
                right: Val::Px(12.0),
                bottom: Val::Px(88.0),
                width: Val::Px(PANEL_WIDTH_PX),
                height: Val::Px(PANEL_HEIGHT_PX),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(6.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(COL_PANEL_BG),
            BorderColor::all(COL_PANEL_BORDER),
            ZIndex(110),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("CAB"),
                TextFont {
                    font_size: FONT_LABEL,
                    ..default()
                },
                TextColor(COL_MUTED),
            ));
            panel.spawn((
                CabSpeedText,
                Text::new("0 km/h"),
                TextFont {
                    font_size: FONT_SPEED,
                    ..default()
                },
                TextColor(COL_SPEED),
            ));
            panel.spawn((
                CabLimitText,
                Text::new("LIM —"),
                TextFont {
                    font_size: FONT_LABEL,
                    ..default()
                },
                TextColor(COL_MUTED),
            ));
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(14.0),
                    column_gap: Val::Px(8.0),
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|bars| {
                    bars.spawn(Node {
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    })
                    .with_children(|col| {
                        col.spawn((
                            Text::new("THR"),
                            TextFont {
                                font_size: 10.0,
                                ..default()
                            },
                            TextColor(COL_MUTED),
                        ));
                        col.spawn((
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(8.0),
                                ..default()
                            },
                            BackgroundColor(COL_BAR_TRACK),
                        ))
                        .with_children(|track| {
                            track.spawn((
                                CabThrottleFill,
                                Node {
                                    width: Val::Percent(0.0),
                                    height: Val::Percent(100.0),
                                    ..default()
                                },
                                BackgroundColor(COL_THROTTLE),
                            ));
                        });
                    });
                    bars.spawn(Node {
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    })
                    .with_children(|col| {
                        col.spawn((
                            Text::new("BRK"),
                            TextFont {
                                font_size: 10.0,
                                ..default()
                            },
                            TextColor(COL_MUTED),
                        ));
                        col.spawn((
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(8.0),
                                ..default()
                            },
                            BackgroundColor(COL_BAR_TRACK),
                        ))
                        .with_children(|track| {
                            track.spawn((
                                CabBrakeFill,
                                Node {
                                    width: Val::Percent(0.0),
                                    height: Val::Percent(100.0),
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
                    font_size: FONT_LABEL,
                    ..default()
                },
                TextColor(COL_TEXT),
            ));
        });
}

pub(crate) fn toggle_cab_panel(
    keys: Res<ButtonInput<KeyCode>>,
    mut visible: ResMut<CabPanelVisible>,
) {
    if keys.just_pressed(KeyCode::KeyC) {
        visible.open = !visible.open;
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn update_cab_panel(
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
    mut throttle: Query<&mut Node, (With<CabThrottleFill>, Without<CabBrakeFill>)>,
    mut brake: Query<&mut Node, (With<CabBrakeFill>, Without<CabThrottleFill>)>,
) {
    let Ok(mut vis) = root.single_mut() else {
        return;
    };
    let Some(live) = live else {
        *vis = Visibility::Hidden;
        return;
    };
    if !visible.open {
        *vis = Visibility::Hidden;
        return;
    }
    *vis = Visibility::Visible;

    let content = build_cab_panel_content(&live.session.cab_telemetry());
    if let Ok((mut text, mut color)) = speed.single_mut() {
        **text = format!("{} km/h", content.speed_line);
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
        node.width = Val::Percent(content.throttle_frac * 100.0);
    }
    if let Ok(mut node) = brake.single_mut() {
        node.width = Val::Percent(content.brake_frac * 100.0);
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
            brake_force_kn: 120.0,
            diesel_rpm: Some(900.0),
            boiler_bar: None,
            overspeed: false,
        };
        let c = build_cab_panel_content(&tel);
        assert_eq!(c.speed_line, "72");
        assert!(c.detail_line.contains("RPM 900"));
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
