//! UI helpers for Bevy 0.19 Parley text.

use bevy::prelude::*;

/// Build a [`TextFont`] with pixel size (Bevy 0.19 `FontSize::Px`).
pub fn text_px(size: f32) -> TextFont {
    TextFont {
        font_size: FontSize::Px(size),
        ..default()
    }
}

/// Same as [`text_px`] with an explicit font handle.
pub fn text_px_with_font(size: f32, font: Handle<Font>) -> TextFont {
    TextFont {
        font: font.into(),
        font_size: FontSize::Px(size),
        ..default()
    }
}
