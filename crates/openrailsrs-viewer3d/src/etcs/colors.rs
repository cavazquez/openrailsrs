//! ERA / OR DMI colours (approx. from DriverMachineInterface.cs).

pub type Rgba = [u8; 4];

pub const BG: Rgba = [3, 17, 34, 255];
pub const PASP_DARK: Rgba = [1, 65, 99, 255];
pub const PASP_LIGHT: Rgba = [3, 110, 160, 255];
pub const GREY: Rgba = [195, 195, 195, 255];
pub const YELLOW: Rgba = [223, 223, 0, 255];
pub const ORANGE: Rgba = [234, 145, 0, 255];
pub const RED: Rgba = [191, 0, 2, 255];
pub const WHITE: Rgba = [255, 255, 255, 255];
pub const BLACK: Rgba = [0, 0, 0, 255];
pub const PANEL: Rgba = [8, 28, 48, 255];
pub const FRAME: Rgba = [80, 100, 120, 255];
/// OR `ColorDarkGrey` — TTI CSM background.
pub const DARK_GREY: Rgba = [47, 47, 47, 255];
