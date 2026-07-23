//! DMI subwindows: Menu + DataEntry (#162/#163), menus from TCS defs.

use openrailsrs_sim::etcs::MenuWindowDef;

use super::colors;
use super::paint::{blit_text, fill_rect, stroke_rect};
use super::symbols::EtcsSymbols;

/// Active overlay on top of the default DMI window.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum DmiOverlay {
    #[default]
    None,
    MainMenu,
    Settings,
    DataEntry {
        value: String,
    },
}

impl DmiOverlay {
    pub fn is_open(&self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubHit {
    Close,
    MenuItem(u8),
    KeyDigit(u8),
    KeyDot,
    KeyYes,
    KeyDel,
}

/// Subwindow rect: OR places non-fullscreen at (334, 15), 246×450.
pub const SW_X: i32 = 334;
pub const SW_Y: i32 = 15;
pub const SW_W: i32 = 246;
pub const SW_H: i32 = 450;

pub fn paint_overlay(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    overlay: &DmiOverlay,
    symbols: &EtcsSymbols,
    pressed: Option<SubHit>,
    main_menu: &MenuWindowDef,
    settings_menu: &MenuWindowDef,
) {
    if matches!(overlay, DmiOverlay::None) {
        return;
    }
    fill_rect(rgba, w, h, SW_X, SW_Y, SW_W, SW_H, colors::PANEL);
    stroke_rect(rgba, w, h, SW_X, SW_Y, SW_W, SW_H, colors::FRAME);

    let title = match overlay {
        DmiOverlay::MainMenu => main_menu.title.as_str(),
        DmiOverlay::Settings => settings_menu.title.as_str(),
        DmiOverlay::DataEntry { .. } => "Data entry",
        DmiOverlay::None => "",
    };

    fill_rect(rgba, w, h, SW_X, SW_Y, SW_W, 50, colors::DARK_GREY);
    blit_text(rgba, w, h, SW_X + 8, SW_Y + 18, 8, 12, title, colors::WHITE);
    let close_pressed = pressed == Some(SubHit::Close);
    fill_rect(
        rgba,
        w,
        h,
        SW_X + SW_W - 50,
        SW_Y,
        50,
        50,
        if close_pressed {
            colors::GREY
        } else {
            colors::PANEL
        },
    );
    stroke_rect(rgba, w, h, SW_X + SW_W - 50, SW_Y, 50, 50, colors::FRAME);
    if !symbols.blit_centered(rgba, w, h, SW_X + SW_W - 50, SW_Y, 50, 50, "NA_11.bmp") {
        blit_text(rgba, w, h, SW_X + SW_W - 36, SW_Y + 18, 8, 12, "X", colors::WHITE);
    }

    match overlay {
        DmiOverlay::None => {}
        DmiOverlay::MainMenu => paint_menu_def(rgba, w, h, main_menu, pressed),
        DmiOverlay::Settings => paint_menu_def(rgba, w, h, settings_menu, pressed),
        DmiOverlay::DataEntry { value } => paint_data_entry(rgba, w, h, value, symbols, pressed),
    }
}

fn paint_menu_def(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    def: &MenuWindowDef,
    pressed: Option<SubHit>,
) {
    for (i, btn) in def.buttons.iter().enumerate() {
        if btn.label.is_empty() {
            continue;
        }
        let col = (i % 2) as i32;
        let row = (i / 2) as i32;
        let x = SW_X + col * 123;
        let y = SW_Y + 50 + row * 50;
        let is_p = pressed == Some(SubHit::MenuItem(i as u8));
        fill_rect(
            rgba,
            w,
            h,
            x,
            y,
            123,
            48,
            if is_p { colors::DARK_GREY } else { colors::PANEL },
        );
        stroke_rect(rgba, w, h, x, y, 123, 48, colors::FRAME);
        let c = if btn.enabled {
            colors::GREY
        } else {
            colors::DARK_GREY
        };
        blit_text(rgba, w, h, x + 10, y + 18, 7, 10, &btn.label, c);
    }
}

fn paint_data_entry(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    value: &str,
    symbols: &EtcsSymbols,
    pressed: Option<SubHit>,
) {
    // Value field
    fill_rect(rgba, w, h, SW_X + 8, SW_Y + 60, SW_W - 16, 36, colors::BG);
    stroke_rect(rgba, w, h, SW_X + 8, SW_Y + 60, SW_W - 16, 36, colors::FRAME);
    blit_text(
        rgba,
        w,
        h,
        SW_X + 14,
        SW_Y + 70,
        10,
        14,
        if value.is_empty() { "_" } else { value },
        colors::YELLOW,
    );

    // Numeric pad 1-9, 0, ., Yes
    let keys: [(i32, &str, SubHit); 12] = [
        (1, "1", SubHit::KeyDigit(1)),
        (2, "2", SubHit::KeyDigit(2)),
        (3, "3", SubHit::KeyDigit(3)),
        (4, "4", SubHit::KeyDigit(4)),
        (5, "5", SubHit::KeyDigit(5)),
        (6, "6", SubHit::KeyDigit(6)),
        (7, "7", SubHit::KeyDigit(7)),
        (8, "8", SubHit::KeyDigit(8)),
        (9, "9", SubHit::KeyDigit(9)),
        (10, "0", SubHit::KeyDigit(0)),
        (11, ".", SubHit::KeyDot),
        (12, "Yes", SubHit::KeyYes),
    ];
    for (idx, label, hit) in keys {
        let i = idx - 1;
        let col = i % 3;
        let row = i / 3;
        let x = SW_X + 8 + col * 78;
        let y = SW_Y + 110 + row * 50;
        let is_p = pressed == Some(hit);
        fill_rect(
            rgba,
            w,
            h,
            x,
            y,
            74,
            46,
            if is_p { colors::DARK_GREY } else { colors::PANEL },
        );
        stroke_rect(rgba, w, h, x, y, 74, 46, colors::FRAME);
        blit_text(rgba, w, h, x + 20, y + 16, 10, 14, label, colors::GREY);
    }

    // Del
    let del_p = pressed == Some(SubHit::KeyDel);
    fill_rect(
        rgba,
        w,
        h,
        SW_X + 8,
        SW_Y + 320,
        SW_W - 16,
        40,
        if del_p { colors::DARK_GREY } else { colors::PANEL },
    );
    stroke_rect(rgba, w, h, SW_X + 8, SW_Y + 320, SW_W - 16, 40, colors::FRAME);
    if !symbols.blit_centered(rgba, w, h, SW_X + 8, SW_Y + 320, SW_W - 16, 40, "NA_21.bmp") {
        blit_text(rgba, w, h, SW_X + 90, SW_Y + 332, 8, 12, "Del", colors::GREY);
    }
}

pub fn hit_test_overlay(overlay: &DmiOverlay, x: i32, y: i32) -> Option<SubHit> {
    if matches!(overlay, DmiOverlay::None) {
        return None;
    }
    if !rect_contains(SW_X, SW_Y, SW_W, SW_H, x, y) {
        return None;
    }
    if rect_contains(SW_X + SW_W - 50, SW_Y, 50, 50, x, y) {
        return Some(SubHit::Close);
    }
    match overlay {
        DmiOverlay::MainMenu | DmiOverlay::Settings => {
            for i in 0..6i32 {
                let col = i % 2;
                let row = i / 2;
                let bx = SW_X + col * 123;
                let by = SW_Y + 50 + row * 50;
                if rect_contains(bx, by, 123, 48, x, y) {
                    return Some(SubHit::MenuItem(i as u8));
                }
            }
            None
        }
        DmiOverlay::DataEntry { .. } => {
            for i in 0..12i32 {
                let col = i % 3;
                let row = i / 3;
                let bx = SW_X + 8 + col * 78;
                let by = SW_Y + 110 + row * 50;
                if rect_contains(bx, by, 74, 46, x, y) {
                    return Some(match i {
                        0..=8 => SubHit::KeyDigit((i + 1) as u8),
                        9 => SubHit::KeyDigit(0),
                        10 => SubHit::KeyDot,
                        _ => SubHit::KeyYes,
                    });
                }
            }
            if rect_contains(SW_X + 8, SW_Y + 320, SW_W - 16, 40, x, y) {
                return Some(SubHit::KeyDel);
            }
            None
        }
        DmiOverlay::None => None,
    }
}

fn rect_contains(rx: i32, ry: i32, rw: i32, rh: i32, x: i32, y: i32) -> bool {
    x >= rx && y >= ry && x < rx + rw && y < ry + rh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_hit() {
        assert_eq!(
            hit_test_overlay(&DmiOverlay::MainMenu, SW_X + SW_W - 25, SW_Y + 25),
            Some(SubHit::Close)
        );
    }

    #[test]
    fn digit_hit() {
        assert_eq!(
            hit_test_overlay(
                &DmiOverlay::DataEntry {
                    value: String::new()
                },
                SW_X + 20,
                SW_Y + 120
            ),
            Some(SubHit::KeyDigit(1))
        );
    }
}
