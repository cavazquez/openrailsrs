//! Overspeed visual feedback (pulsing CAB border + HUD accent).

use bevy::prelude::*;

use crate::cab_panel::CabPanelRoot;
use crate::hud::HudRow1;
use crate::live::LiveDrive;

const COL_HUD_NORMAL: Color = Color::srgb(0.8, 0.8, 0.8);
const COL_CAB_BORDER_NORMAL: Color = Color::srgb(0.35, 0.45, 0.55);

/// Smoothed flash intensity [0, 1] while overspeeding.
#[derive(Resource, Default)]
pub struct OverspeedFlash {
    pub intensity: f32,
}

pub(crate) fn tick_overspeed_flash(
    time: Res<Time>,
    live: Option<Res<LiveDrive>>,
    mut flash: ResMut<OverspeedFlash>,
) {
    let active = live
        .as_ref()
        .is_some_and(|l| l.session.gameplay.overspeed_active && !l.session.arrived);
    let dt = time.delta_secs();
    if active {
        flash.intensity = (flash.intensity + dt * 4.0).min(1.0);
    } else {
        flash.intensity = (flash.intensity - dt * 6.0).max(0.0);
    }
}

pub(crate) fn apply_overspeed_flash(
    flash: Res<OverspeedFlash>,
    mut hud: Query<&mut TextColor, With<HudRow1>>,
    mut cab: Query<&mut BorderColor, With<CabPanelRoot>>,
) {
    let pulse = if flash.intensity > 0.01 {
        0.55 + 0.45 * (flash.intensity * std::f32::consts::TAU * 3.0).sin().abs()
    } else {
        0.0
    };
    if pulse <= 0.01 {
        for mut color in &mut hud {
            *color = TextColor(COL_HUD_NORMAL);
        }
        for mut border in &mut cab {
            *border = BorderColor::all(COL_CAB_BORDER_NORMAL);
        }
        return;
    }
    let hud_c = Color::srgb(
        0.8 + 0.2 * pulse,
        0.8 * (1.0 - pulse * 0.75),
        0.8 * (1.0 - pulse * 0.75),
    );
    let cab_c = Color::srgb(
        0.35 + 0.65 * pulse,
        0.45 * (1.0 - pulse * 0.8),
        0.55 * (1.0 - pulse * 0.8),
    );
    for mut color in &mut hud {
        *color = TextColor(hud_c);
    }
    for mut border in &mut cab {
        *border = BorderColor::all(cab_c);
    }
}
