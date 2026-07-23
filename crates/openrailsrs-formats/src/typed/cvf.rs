//! Typed adapter for MSTS cab view files (`.cvf`).
//!
//! Reference: OpenBVE `Train.MsTs/Panel/CvfParser.cs`, Open Rails `CabViewFile.cs`.

use crate::ast::{Ast, Atom};
use crate::error::FormatError;

use super::{atom_to_number, atom_to_string, walk_lists_find};

/// Parsed MSTS cab view definition (`Tr_CabViewFile`).
#[derive(Clone, Debug, PartialEq)]
pub struct CabViewFile {
    pub cab_view_type: Option<u32>,
    pub views: Vec<CabView>,
    pub controls: Vec<CabControl>,
}

/// A 2D cab panel view (texture + head position).
#[derive(Clone, Debug, PartialEq)]
pub struct CabView {
    pub texture_ace: String,
    pub window: ScreenRect,
    pub position_m: [f64; 3],
    pub direction_deg: [f64; 3],
}

/// Screen rectangle in cab panel pixels (x, y, width, height).
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Simulation variable driving a cab control (from `Type (...)` tokens).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ControlType {
    Throttle,
    TrainBrake,
    Ammeter,
    ThrottleDisplay,
    DirectionDisplay,
    DynamicBrakeDisplay,
    Speedometer,
    MainRes,
    BrakePipe,
    BrakeCyl,
    EqRes,
    LoadMeter,
    PenaltyApp,
    /// OR `ORTS_ETCS` screen / DMI.
    OrtsEtcs,
    Generic(String),
}

impl ControlType {
    pub fn from_type_tokens(tokens: &[String]) -> Self {
        let joined = tokens
            .iter()
            .map(|t| t.to_ascii_uppercase())
            .collect::<Vec<_>>()
            .join(" ");
        match joined.as_str() {
            "THROTTLE LEVER" | "THROTTLE" => return Self::Throttle,
            "TRAIN BRAKE LEVER" | "TRAIN BRAKE" => return Self::TrainBrake,
            "AMMETER" | "AMMETER ABS" => return Self::Ammeter,
            _ => {}
        }
        for token in tokens {
            match token.to_ascii_uppercase().as_str() {
                "THROTTLE" => return Self::Throttle,
                "TRAIN_BRAKE" | "TRAIN BRAKE" => return Self::TrainBrake,
                "THROTTLE_DISPLAY" => return Self::ThrottleDisplay,
                "DIRECTION_DISPLAY" => return Self::DirectionDisplay,
                "DYNAMIC_BRAKE_DISPLAY" => return Self::DynamicBrakeDisplay,
                "SPEEDOMETER" => return Self::Speedometer,
                "MAIN_RES" => return Self::MainRes,
                "BRAKE_PIPE" => return Self::BrakePipe,
                "BRAKE_CYL" => return Self::BrakeCyl,
                "EQ_RES" => return Self::EqRes,
                "LOAD_METER" => return Self::LoadMeter,
                "PENALTY_APP" => return Self::PenaltyApp,
                "ORTS_ETCS" => return Self::OrtsEtcs,
                _ => {}
            }
        }
        Self::Generic(tokens.first().cloned().unwrap_or_default())
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Throttle => "THROTTLE",
            Self::TrainBrake => "TRAIN_BRAKE",
            Self::Ammeter => "AMMETER",
            Self::ThrottleDisplay => "THROTTLE_DISPLAY",
            Self::DirectionDisplay => "DIRECTION_DISPLAY",
            Self::DynamicBrakeDisplay => "DYNAMIC_BRAKE_DISPLAY",
            Self::Speedometer => "SPEEDOMETER",
            Self::MainRes => "MAIN_RES",
            Self::BrakePipe => "BRAKE_PIPE",
            Self::BrakeCyl => "BRAKE_CYL",
            Self::EqRes => "EQ_RES",
            Self::LoadMeter => "LOAD_METER",
            Self::PenaltyApp => "PENALTY_APP",
            Self::OrtsEtcs => "ORTS_ETCS",
            Self::Generic(s) => s.as_str(),
        }
    }
}

/// One discrete state for multi-state displays.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlState {
    pub style: u32,
    pub switch_val: f64,
}

/// Discrete ACE frames for a CVF `Lever` (Open Rails `CVCWithFrames` subset).
#[derive(Clone, Debug, PartialEq)]
pub struct CabLeverFrames {
    pub frames_count: u32,
    pub frames_x: u32,
    pub frames_y: u32,
    /// 0 = horizontal, 1 = vertical (MSTS/OR `Orientation`).
    pub orientation: i32,
    /// When false, mouse/input direction is inverted (`DirIncrease 0`).
    pub dir_increase: bool,
    /// Normalized control values per frame (ascending). Empty until normalized.
    pub values: Vec<f64>,
    pub min_value: f64,
    pub max_value: f64,
}

impl Default for CabLeverFrames {
    fn default() -> Self {
        Self {
            frames_count: 0,
            frames_x: 0,
            frames_y: 0,
            orientation: 0,
            dir_increase: true,
            values: Vec::new(),
            min_value: 0.0,
            max_value: 1.0,
        }
    }
}

impl CabLeverFrames {
    /// Open Rails `PercentToIndex` for lever/discrete ACE frames.
    pub fn percent_to_index(&self, percent: f64) -> usize {
        let mut percent = percent;
        if percent > 1.0 {
            percent /= 100.0;
        }
        if self.min_value != self.max_value {
            percent = percent.clamp(self.min_value, self.max_value);
        }
        let frames = self.frames_count.max(1) as usize;
        if self.values.len() > 1 {
            let mut best = 0usize;
            let mut best_dist = f64::INFINITY;
            for (i, v) in self.values.iter().enumerate() {
                let dist = (v - percent).abs();
                if dist < best_dist {
                    best_dist = dist;
                    best = i;
                }
            }
            return best.min(frames.saturating_sub(1));
        }
        if self.max_value != self.min_value {
            let span = self.max_value - self.min_value;
            let idx = ((percent - self.min_value) / span * self.frames_count as f64).floor() as isize;
            return idx.clamp(0, frames as isize - 1) as usize;
        }
        0
    }

    /// Pixel rect of frame `index` inside a packed ACE sheet (row-major, OR order).
    pub fn frame_rect(&self, image_w: f32, image_h: f32, index: usize) -> (f32, f32, f32, f32) {
        let fx = self.frames_x.max(1) as f32;
        let fy = self.frames_y.max(1) as f32;
        let cell_w = image_w / fx;
        let cell_h = image_h / fy;
        let max_index = (self.frames_count.max(1) as usize).saturating_sub(1);
        let idx = index.min(max_index);
        let x = (idx as u32 % self.frames_x.max(1)) as f32;
        let y = (idx as u32 / self.frames_x.max(1)) as f32;
        (x * cell_w, y * cell_h, cell_w, cell_h)
    }
}

/// Dial needle metadata (`ScaleRange` / `ScalePos` / `Pivot` / `DirIncrease`).
///
/// Open Rails maps `ScalePos (from to)` → `FromDegree` / `ToDegree` (not a separate
/// `FromDegree` token). Degrees: 0° = 12 o'clock, 90° = 3 o'clock.
#[derive(Clone, Debug, PartialEq)]
pub struct CabDialParams {
    pub scale_min: f64,
    pub scale_max: f64,
    pub from_degree: f64,
    pub to_degree: f64,
    /// Pivot Y in unscaled ACE pixels; `None` → half texture height at draw time.
    pub pivot: Option<f64>,
    /// When false (`DirIncrease 0`), needle sweeps the opposite way.
    pub dir_increase: bool,
    pub units: Option<String>,
}

impl Default for CabDialParams {
    fn default() -> Self {
        Self {
            scale_min: 0.0,
            scale_max: 1.0,
            from_degree: 0.0,
            to_degree: 0.0,
            pivot: None,
            dir_increase: true,
            units: None,
        }
    }
}

impl CabDialParams {
    /// Open Rails `GetRangeFraction` against `ScaleRange`.
    pub fn range_fraction(&self, value: f64) -> f64 {
        if (self.scale_max - self.scale_min).abs() < f64::EPSILON {
            return 0.0;
        }
        ((value - self.scale_min) / (self.scale_max - self.scale_min)).clamp(0.0, 1.0)
    }

    /// Needle rotation in radians (OR `CabViewDialRenderer`).
    pub fn rotation_radians(&self, value: f64) -> f32 {
        let fraction = self.range_fraction(value);
        let direction = if self.dir_increase { 1.0 } else { -1.0 };
        let mut range_degrees = direction * (self.to_degree - self.from_degree);
        while range_degrees <= 0.0 {
            range_degrees += 360.0;
        }
        let deg = self.from_degree + direction * range_degrees * fraction;
        // Wrap to [-π, π] like MathHelper.WrapAngle.
        let mut rad = (deg as f32).to_radians();
        const PI: f32 = std::f32::consts::PI;
        const TAU: f32 = std::f32::consts::TAU;
        rad = rad.rem_euclid(TAU);
        if rad > PI {
            rad -= TAU;
        }
        rad
    }
}

/// Digital readout metadata (`Accuracy` / `LeadingZeros` / `Justification` / …).
///
/// Open Rails: `CabViewDigitalRenderer` / `CVCDigital`.
#[derive(Clone, Debug, PartialEq)]
pub struct CabDigitalParams {
    pub scale_min: f64,
    pub scale_max: f64,
    /// Decimal places (`Accuracy`).
    pub accuracy: i32,
    pub leading_zeros: u32,
    /// 1 = center, 2 = left, 3 = right (OR `Justification`).
    pub justification: u32,
    pub units: Option<String>,
}

impl Default for CabDigitalParams {
    fn default() -> Self {
        Self {
            scale_min: 0.0,
            scale_max: 999.0,
            accuracy: 0,
            leading_zeros: 0,
            justification: 1,
            units: None,
        }
    }
}

/// Gauge bar metadata (`Orientation` / `DirIncrease` / colours / `Style`).
///
/// Open Rails: `CVCGauge` / `CabViewGaugeRenderer` — used by `ThreeDimCabGaugeNative`.
#[derive(Clone, Debug, PartialEq)]
pub struct CabGaugeParams {
    pub scale_min: f64,
    pub scale_max: f64,
    /// 0 = horizontal, 1 = vertical.
    pub orientation: i32,
    /// `DirIncrease` (0/1); XOR with negative fraction flips growth axis.
    pub direction: i32,
    /// `POINTER` → shape MultiState; otherwise solid/liquid native quad.
    pub style: Option<String>,
    pub units: Option<String>,
    /// RGBA 0–1 (`PositiveColour` / `ControlColour`).
    pub positive_colour: Option<[f32; 4]>,
    pub negative_colour: Option<[f32; 4]>,
}

impl Default for CabGaugeParams {
    fn default() -> Self {
        Self {
            scale_min: 0.0,
            scale_max: 1.0,
            orientation: 0,
            direction: 1,
            style: None,
            units: None,
            positive_colour: Some([1.0, 1.0, 0.0, 1.0]),
            negative_colour: Some([1.0, 0.0, 0.0, 1.0]),
        }
    }
}

impl CabGaugeParams {
    /// OR `GetRangeFraction(offsetFromZero)`.
    pub fn range_fraction(&self, value: f64, offset_from_zero: bool) -> f64 {
        if value < self.scale_min {
            return 0.0;
        }
        if value > self.scale_max {
            return 1.0;
        }
        if (self.scale_max - self.scale_min).abs() < f64::EPSILON {
            return 0.0;
        }
        let base = if offset_from_zero && self.scale_min < 0.0 {
            0.0
        } else {
            self.scale_min
        };
        (value - base) / (self.scale_max - self.scale_min)
    }

    pub fn is_pointer(&self) -> bool {
        self.style
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case("POINTER"))
    }
}

impl CabDigitalParams {
    /// Format a numeric reading like Open Rails digital cab displays.
    pub fn format_value(&self, value: f64) -> String {
        let v = value.clamp(self.scale_min, self.scale_max);
        let decimals = self.accuracy.max(0) as usize;
        let body = if decimals == 0 {
            format!("{}", v.round() as i64)
        } else {
            format!("{v:.decimals$}")
        };
        if self.leading_zeros == 0 {
            return body;
        }
        let (sign, digits) = if let Some(rest) = body.strip_prefix('-') {
            ("-", rest)
        } else {
            ("", body.as_str())
        };
        let (int_part, frac) = match digits.split_once('.') {
            Some((i, f)) => (i, Some(f)),
            None => (digits, None),
        };
        let width = self.leading_zeros as usize;
        let padded = format!("{int_part:0>width$}");
        match frac {
            Some(f) => format!("{sign}{padded}.{f}"),
            None => format!("{sign}{padded}"),
        }
    }
}

/// Cab control element (display, dial, digital readout, …).
#[derive(Clone, Debug, PartialEq)]
pub enum CabControl {
    MultiStateDisplay {
        control_type: ControlType,
        position: ScreenRect,
        graphic: String,
        states: Vec<ControlState>,
    },
    Dial {
        control_type: ControlType,
        position: ScreenRect,
        graphic: String,
        dial: CabDialParams,
    },
    Digital {
        control_type: ControlType,
        position: ScreenRect,
        digital: CabDigitalParams,
    },
    Gauge {
        control_type: ControlType,
        position: ScreenRect,
        graphic: String,
        gauge: CabGaugeParams,
    },
    /// OR `ScreenDisplay` (ETCS DMI / animated cab screen).
    Screen {
        control_type: ControlType,
        position: ScreenRect,
        graphic: String,
        /// `Parameters ( key value … )` lowercased.
        parameters: std::collections::HashMap<String, String>,
        hide_if_disabled: bool,
    },
    TwoStateDisplay {
        control_type: ControlType,
        position: ScreenRect,
        graphic: String,
        frames: CabLeverFrames,
        /// CVF `MouseControl ( 1 )`.
        mouse_control: bool,
        /// CVF `Style` token (`ONOFF`, `WHILE_PRESSED`, …).
        style: Option<String>,
    },
    TriStateDisplay {
        control_type: ControlType,
        position: ScreenRect,
        graphic: String,
        frames: CabLeverFrames,
        mouse_control: bool,
        style: Option<String>,
    },
    /// Discrete lever (CVF ACE frames): throttle, train brake, …
    Lever {
        control_type: ControlType,
        position: Option<ScreenRect>,
        graphic: String,
        frames: CabLeverFrames,
        mouse_control: bool,
        style: Option<String>,
    },
    Unknown {
        kind: String,
    },
}

impl CabViewFile {
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        if let Some(root) = find_cabview_root(ast) {
            return parse_cabview_block(root);
        }
        if looks_like_cabview_content(ast) {
            return parse_cabview_block(ast);
        }
        Err(FormatError::MissingField {
            key: "Tr_CabViewFile".to_string(),
            context: "cvf".to_string(),
        })
    }

    /// Collect distinct control type names (for inspect / debugging).
    pub fn control_type_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .controls
            .iter()
            .filter_map(|c| control_type(c).map(ControlType::as_str))
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }
}

fn control_type(control: &CabControl) -> Option<&ControlType> {
    control.control_type()
}

impl CabControl {
    /// Control type token when this entry is a typed instrument/lever.
    pub fn control_type(&self) -> Option<&ControlType> {
        match self {
            CabControl::MultiStateDisplay { control_type, .. }
            | CabControl::Dial { control_type, .. }
            | CabControl::Digital { control_type, .. }
            | CabControl::Gauge { control_type, .. }
            | CabControl::Screen { control_type, .. }
            | CabControl::TwoStateDisplay { control_type, .. }
            | CabControl::TriStateDisplay { control_type, .. }
            | CabControl::Lever { control_type, .. } => Some(control_type),
            CabControl::Unknown { .. } => None,
        }
    }
}

fn find_cabview_root(ast: &Ast) -> Option<&Ast> {
    if is_cabview_block(ast) {
        return Some(ast);
    }
    if let Ast::List(items) = ast {
        for item in items {
            if let Some(found) = find_cabview_root(item) {
                return Some(found);
            }
        }
    }
    None
}

fn looks_like_cabview_content(ast: &Ast) -> bool {
    walk_lists_find(ast, &mut |items| {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            match head.to_ascii_uppercase().as_str() {
                "CABVIEWTYPE" | "CABVIEWCONTROLS" | "CABVIEWFILE" => return Some(()),
                _ => {}
            }
        }
        None
    })
    .is_some()
}

fn cabview_block_start_index(items: &[Ast]) -> usize {
    if items.first().is_some_and(|item| {
        matches!(item, Ast::Atom(Atom::Symbol(head)) if head.eq_ignore_ascii_case("Tr_CabViewFile"))
    }) {
        1
    } else {
        0
    }
}

fn is_cabview_block(ast: &Ast) -> bool {
    matches!(
        ast,
        Ast::List(items)
            if items.first().is_some_and(|item| {
                matches!(item, Ast::Atom(Atom::Symbol(head)) if head.eq_ignore_ascii_case("Tr_CabViewFile"))
            })
    )
}

fn parse_cabview_block(ast: &Ast) -> Result<CabViewFile, FormatError> {
    let Ast::List(items) = ast else {
        return Err(FormatError::UnexpectedAtom {
            key: "Tr_CabViewFile".to_string(),
            context: "cvf".to_string(),
            expected: "list".to_string(),
        });
    };

    let mut cab_view_type = None;
    let mut views = Vec::new();
    let mut controls = Vec::new();

    let mut pending_texture: Option<String> = None;
    let mut pending_window = ScreenRect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };
    let mut pending_position = [0.0; 3];
    let mut pending_direction = [0.0; 3];
    let mut has_position = false;
    let mut has_direction = false;

    let start = cabview_block_start_index(items);
    let mut i = start;
    while i < items.len() {
        match &items[i] {
            Ast::List(entry) => {
                if let Some(Ast::Atom(Atom::Symbol(key))) = entry.first() {
                    apply_cabview_field(
                        key,
                        Some(entry.as_slice()),
                        None,
                        &mut cab_view_type,
                        &mut views,
                        &mut controls,
                        &mut pending_texture,
                        &mut pending_window,
                        &mut pending_position,
                        &mut pending_direction,
                        &mut has_position,
                        &mut has_direction,
                    )?;
                }
                i += 1;
            }
            Ast::Atom(Atom::Symbol(key)) => {
                i += 1;
                let value = items.get(i);
                apply_cabview_field(
                    key,
                    None,
                    value,
                    &mut cab_view_type,
                    &mut views,
                    &mut controls,
                    &mut pending_texture,
                    &mut pending_window,
                    &mut pending_position,
                    &mut pending_direction,
                    &mut has_position,
                    &mut has_direction,
                )?;
                if value.is_some() {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }

    flush_pending_view(
        &mut views,
        &mut pending_texture,
        &pending_window,
        pending_position,
        pending_direction,
        has_position,
        has_direction,
    );

    Ok(CabViewFile {
        cab_view_type,
        views,
        controls,
    })
}

#[allow(clippy::too_many_arguments)]
fn apply_cabview_field(
    key: &str,
    entry: Option<&[Ast]>,
    flat_value: Option<&Ast>,
    cab_view_type: &mut Option<u32>,
    views: &mut Vec<CabView>,
    controls: &mut Vec<CabControl>,
    pending_texture: &mut Option<String>,
    pending_window: &mut ScreenRect,
    pending_position: &mut [f64; 3],
    pending_direction: &mut [f64; 3],
    has_position: &mut bool,
    has_direction: &mut bool,
) -> Result<(), FormatError> {
    match key.to_ascii_uppercase().as_str() {
        "CABVIEWTYPE" => {
            *cab_view_type = Some(parse_u32_value(entry, flat_value)?);
        }
        "CABVIEWFILE" => {
            flush_pending_view(
                views,
                pending_texture,
                pending_window,
                *pending_position,
                *pending_direction,
                *has_position,
                *has_direction,
            );
            *pending_texture = Some(parse_string_value(entry, flat_value)?);
            *has_position = false;
            *has_direction = false;
        }
        "CABVIEWWINDOW" => {
            *pending_window = parse_screen_rect_value(entry, flat_value)?;
        }
        "POSITION" => {
            *pending_position = parse_vec3_value(entry, flat_value)?;
            *has_position = true;
        }
        "DIRECTION" => {
            *pending_direction = parse_vec3_value(entry, flat_value)?;
            *has_direction = true;
        }
        "CABVIEWCONTROLS" => {
            flush_pending_view(
                views,
                pending_texture,
                pending_window,
                *pending_position,
                *pending_direction,
                *has_position,
                *has_direction,
            );
            if let Some(entry) = entry {
                parse_controls(entry, controls)?;
            } else if let Some(Ast::List(vals)) = flat_value {
                parse_controls_flat(vals, controls)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn parse_controls_flat(items: &[Ast], out: &mut Vec<CabControl>) -> Result<(), FormatError> {
    let mut i = items
        .first()
        .filter(|a| matches!(a, Ast::Atom(Atom::Number(_) | Atom::Integer(_))))
        .map(|_| 1)
        .unwrap_or(0);
    while i < items.len() {
        match &items[i] {
            Ast::List(entry) => {
                if let Some(control) = parse_control_entry(entry)? {
                    out.push(control);
                }
                i += 1;
            }
            Ast::Atom(Atom::Symbol(kind)) => {
                i += 1;
                let Some(Ast::List(values)) = items.get(i) else {
                    continue;
                };
                let mut entry = vec![Ast::Atom(Atom::Symbol(kind.clone()))];
                entry.extend(values.clone());
                if let Some(control) = parse_control_entry(&entry)? {
                    out.push(control);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    Ok(())
}

fn parse_control_entry(entry: &[Ast]) -> Result<Option<CabControl>, FormatError> {
    let Some(Ast::Atom(Atom::Symbol(kind))) = entry.first() else {
        return Ok(None);
    };
    Ok(Some(match kind.to_ascii_uppercase().as_str() {
        "MULTISTATEDISPLAY" => parse_multi_state(entry)?,
        "DIAL" => parse_dial(entry)?,
        "DIGITAL" => parse_digital(entry)?,
        "GAUGE" => parse_gauge(entry)?,
        "SCREENDISPLAY" | "SCREEN" => parse_screen(entry)?,
        "TWOSTATEDISPLAY" | "TWOSTATE" => parse_two_state(entry)?,
        "TRISTATEDISPLAY" | "TRISTATE" => parse_tri_state(entry)?,
        "LEVER" => parse_lever(entry)?,
        other => CabControl::Unknown {
            kind: other.to_string(),
        },
    }))
}

fn parse_u32_value(entry: Option<&[Ast]>, flat_value: Option<&Ast>) -> Result<u32, FormatError> {
    if let Some(entry) = entry {
        return parse_u32_field(entry, "CabViewType")?.ok_or_else(|| FormatError::MissingField {
            key: "CabViewType".to_string(),
            context: "cvf".to_string(),
        });
    }
    parse_u32_from_ast(flat_value.ok_or_else(|| FormatError::MissingField {
        key: "CabViewType".to_string(),
        context: "cvf".to_string(),
    })?)
}

fn parse_u32_from_ast(value: &Ast) -> Result<u32, FormatError> {
    match value {
        Ast::Atom(atom) => {
            atom_to_number(atom)
                .map(|v| v as u32)
                .ok_or_else(|| FormatError::UnexpectedAtom {
                    key: "CabViewType".to_string(),
                    context: "cvf".to_string(),
                    expected: "number".to_string(),
                })
        }
        Ast::List(items) => {
            if items.len() == 1 {
                if let Ast::Atom(atom) = &items[0] {
                    if let Some(v) = atom_to_number(atom) {
                        return Ok(v as u32);
                    }
                }
            }
            parse_coordinate_numbers(items)?
                .first()
                .copied()
                .map(|v| v as u32)
                .ok_or_else(|| FormatError::MissingField {
                    key: "CabViewType".to_string(),
                    context: "cvf".to_string(),
                })
        }
    }
}

fn parse_string_value(
    entry: Option<&[Ast]>,
    flat_value: Option<&Ast>,
) -> Result<String, FormatError> {
    if let Some(entry) = entry {
        return parse_string_field(entry, "CabViewFile")?.ok_or_else(|| {
            FormatError::MissingField {
                key: "CabViewFile".to_string(),
                context: "cvf".to_string(),
            }
        });
    }
    match flat_value {
        Some(Ast::Atom(atom)) => atom_to_string(atom).ok_or_else(|| FormatError::UnexpectedAtom {
            key: "CabViewFile".to_string(),
            context: "cvf".to_string(),
            expected: "string".to_string(),
        }),
        Some(Ast::List(items)) => {
            if let Some(Ast::Atom(atom)) = items.first() {
                atom_to_string(atom).ok_or_else(|| FormatError::UnexpectedAtom {
                    key: "CabViewFile".to_string(),
                    context: "cvf".to_string(),
                    expected: "string".to_string(),
                })
            } else {
                Err(FormatError::MissingField {
                    key: "CabViewFile".to_string(),
                    context: "cvf".to_string(),
                })
            }
        }
        None => Err(FormatError::MissingField {
            key: "CabViewFile".to_string(),
            context: "cvf".to_string(),
        }),
    }
}

fn parse_screen_rect_value(
    entry: Option<&[Ast]>,
    flat_value: Option<&Ast>,
) -> Result<ScreenRect, FormatError> {
    if let Some(entry) = entry {
        return parse_screen_rect(entry, "CabViewWindow");
    }
    match flat_value {
        Some(Ast::List(items)) => parse_screen_rect(items, "CabViewWindow"),
        Some(value) => {
            let fake = Ast::List(vec![
                Ast::Atom(Atom::Symbol("CabViewWindow".into())),
                value.clone(),
            ]);
            if let Ast::List(items) = fake {
                parse_screen_rect(&items, "CabViewWindow")
            } else {
                unreachable!()
            }
        }
        None => Err(FormatError::MissingField {
            key: "CabViewWindow".to_string(),
            context: "cvf".to_string(),
        }),
    }
}

fn parse_vec3_value(
    entry: Option<&[Ast]>,
    flat_value: Option<&Ast>,
) -> Result<[f64; 3], FormatError> {
    if let Some(entry) = entry {
        return parse_vec3(entry, "Position");
    }
    match flat_value {
        Some(Ast::List(items)) => parse_vec3(items, "Position"),
        Some(value) => {
            let fake = Ast::List(vec![
                Ast::Atom(Atom::Symbol("Position".into())),
                value.clone(),
            ]);
            if let Ast::List(items) = fake {
                parse_vec3(&items, "Position")
            } else {
                unreachable!()
            }
        }
        None => Err(FormatError::MissingField {
            key: "Position".to_string(),
            context: "cvf".to_string(),
        }),
    }
}

fn flush_pending_view(
    views: &mut Vec<CabView>,
    pending_texture: &mut Option<String>,
    window: &ScreenRect,
    position_m: [f64; 3],
    direction_deg: [f64; 3],
    has_position: bool,
    has_direction: bool,
) {
    if let Some(texture_ace) = pending_texture.take() {
        if has_position || has_direction || !texture_ace.is_empty() {
            views.push(CabView {
                texture_ace,
                window: window.clone(),
                position_m,
                direction_deg,
            });
        }
    }
}

fn parse_controls(items: &[Ast], out: &mut Vec<CabControl>) -> Result<(), FormatError> {
    for item in items.iter().skip(1) {
        let Ast::List(entry) = item else {
            continue;
        };
        if let Some(control) = parse_control_entry(entry)? {
            out.push(control);
        }
    }
    Ok(())
}

fn parse_multi_state(items: &[Ast]) -> Result<CabControl, FormatError> {
    let control_type = parse_control_type(items)?;
    let position = find_screen_rect(items, "MultiStateDisplay")?;
    let graphic = find_string_in_list(items, "Graphic").unwrap_or_default();
    let states = parse_states(items)?;
    Ok(CabControl::MultiStateDisplay {
        control_type,
        position,
        graphic,
        states,
    })
}

fn parse_dial(items: &[Ast]) -> Result<CabControl, FormatError> {
    let control_type = parse_control_type(items)?;
    let position = find_screen_rect(items, "Dial")?;
    let graphic = find_string_in_list(items, "Graphic").unwrap_or_default();
    Ok(CabControl::Dial {
        control_type,
        position,
        graphic,
        dial: parse_dial_params(items),
    })
}

fn parse_dial_params(items: &[Ast]) -> CabDialParams {
    let mut dial = CabDialParams::default();
    if let Some(nums) = find_named_numbers(items, "ScaleRange") {
        if nums.len() >= 2 {
            dial.scale_min = nums[0];
            dial.scale_max = nums[1];
        }
    }
    // Open Rails: ScalePos (from to) → FromDegree / ToDegree.
    if let Some(nums) = find_named_numbers(items, "ScalePos") {
        if nums.len() >= 2 {
            dial.from_degree = nums[0];
            dial.to_degree = nums[1];
        }
    }
    if let Some(nums) = find_named_numbers(items, "Pivot") {
        if let Some(v) = nums.first() {
            dial.pivot = Some(*v);
        }
    }
    if let Some(nums) = find_named_numbers(items, "DirIncrease") {
        if let Some(v) = nums.first() {
            dial.dir_increase = *v != 0.0;
        }
    }
    dial.units = find_string_in_list(items, "Units");
    dial
}

fn parse_digital(items: &[Ast]) -> Result<CabControl, FormatError> {
    let control_type = parse_control_type(items)?;
    let position = find_screen_rect(items, "Digital")?;
    Ok(CabControl::Digital {
        control_type,
        position,
        digital: parse_digital_params(items),
    })
}

fn parse_gauge(items: &[Ast]) -> Result<CabControl, FormatError> {
    let control_type = parse_control_type(items)?;
    let position = find_screen_rect(items, "Gauge").unwrap_or(ScreenRect {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    });
    let graphic = find_string_in_list(items, "Graphic").unwrap_or_default();
    Ok(CabControl::Gauge {
        control_type,
        position,
        graphic,
        gauge: parse_gauge_params(items),
    })
}

fn parse_screen(items: &[Ast]) -> Result<CabControl, FormatError> {
    let control_type = parse_control_type(items)?;
    let position = find_screen_rect(items, "ScreenDisplay").unwrap_or(ScreenRect {
        x: 0.0,
        y: 0.0,
        width: 640.0,
        height: 480.0,
    });
    let graphic = find_string_in_list(items, "Graphic").unwrap_or_default();
    Ok(CabControl::Screen {
        control_type,
        position,
        graphic,
        parameters: parse_screen_parameters(items),
        hide_if_disabled: parse_hide_if_disabled(items),
    })
}

fn parse_hide_if_disabled(items: &[Ast]) -> bool {
    find_named_numbers(items, "HideIfDisabled")
        .and_then(|n| n.first().copied())
        .is_some_and(|v| v != 0.0)
}

fn parse_screen_parameters(items: &[Ast]) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let _ = walk_lists_find(&Ast::List(items.to_vec()), &mut |list| {
        if list.len() < 2 {
            return None::<()>;
        }
        let Ast::Atom(Atom::Symbol(head)) = &list[0] else {
            return None;
        };
        if !head.eq_ignore_ascii_case("Parameters") {
            return None;
        }
        // `(Parameters ( Mode Full Size 640 ))` or flat key/value pairs / `(Key ( Value ))`.
        let body: &[Ast] = if list.len() == 2 {
            if let Ast::List(inner) = &list[1] {
                inner.as_slice()
            } else {
                &list[1..]
            }
        } else {
            &list[1..]
        };
        let tokens = flatten_param_tokens(body);
        for pair in tokens.chunks(2) {
            if pair.len() == 2 {
                out.insert(pair[0].clone(), pair[1].clone());
            }
        }
        Some(())
    });
    out
}

fn flatten_param_tokens(items: &[Ast]) -> Vec<String> {
    let mut tokens = Vec::new();
    for item in items {
        match item {
            Ast::Atom(a) => {
                if let Some(s) = atom_to_string(a) {
                    tokens.push(s.to_ascii_lowercase());
                } else if let Some(n) = atom_to_number(a) {
                    tokens.push(format!("{n}"));
                }
            }
            Ast::List(sub) => tokens.extend(flatten_param_tokens(sub)),
        }
    }
    tokens
}

fn parse_gauge_params(items: &[Ast]) -> CabGaugeParams {
    let mut gauge = CabGaugeParams::default();
    if let Some(nums) = find_named_numbers(items, "ScaleRange") {
        if nums.len() >= 2 {
            gauge.scale_min = nums[0];
            gauge.scale_max = nums[1];
        }
    }
    if let Some(nums) = find_named_numbers(items, "Orientation") {
        if let Some(v) = nums.first() {
            gauge.orientation = *v as i32;
        }
    }
    if let Some(nums) = find_named_numbers(items, "DirIncrease") {
        if let Some(v) = nums.first() {
            gauge.direction = *v as i32;
        }
    }
    gauge.style = parse_control_style(items);
    gauge.units = find_string_in_list(items, "Units");
    gauge.positive_colour = parse_named_control_colour(items, "PositiveColour")
        .or(gauge.positive_colour);
    gauge.negative_colour = parse_named_control_colour(items, "NegativeColour")
        .or(gauge.negative_colour);
    gauge
}

/// `PositiveColour ( n (ControlColour ( r g b )) … )` → RGBA 0–1 (A=1).
fn parse_named_control_colour(items: &[Ast], key: &str) -> Option<[f32; 4]> {
    walk_lists_find(&Ast::List(items.to_vec()), &mut |list| {
        if list.len() < 2 {
            return None;
        }
        let Ast::Atom(Atom::Symbol(head)) = &list[0] else {
            return None;
        };
        if !head.eq_ignore_ascii_case(key) {
            return None;
        }
        walk_lists_find(&Ast::List(list[1..].to_vec()), &mut |inner| {
            if inner.len() < 2 {
                return None;
            }
            let Ast::Atom(Atom::Symbol(h)) = &inner[0] else {
                return None;
            };
            if !h.eq_ignore_ascii_case("ControlColour") {
                return None;
            }
            let nums = flatten_numbers(&inner[1..]);
            if nums.len() >= 3 {
                Some([
                    1.0,
                    (nums[0] as f32 / 255.0).clamp(0.0, 1.0),
                    (nums[1] as f32 / 255.0).clamp(0.0, 1.0),
                    (nums[2] as f32 / 255.0).clamp(0.0, 1.0),
                ])
            } else {
                None
            }
        })
    })
}

fn parse_digital_params(items: &[Ast]) -> CabDigitalParams {
    let mut digital = CabDigitalParams {
        units: find_string_in_list(items, "Units"),
        ..Default::default()
    };
    if let Some(nums) = find_named_numbers(items, "ScaleRange") {
        if nums.len() >= 2 {
            digital.scale_min = nums[0];
            digital.scale_max = nums[1];
        }
    }
    if let Some(nums) = find_named_numbers(items, "Accuracy") {
        if let Some(v) = nums.first() {
            digital.accuracy = *v as i32;
        }
    }
    if let Some(nums) = find_named_numbers(items, "LeadingZeros") {
        if let Some(v) = nums.first() {
            digital.leading_zeros = (*v).max(0.0) as u32;
        }
    }
    if let Some(nums) = find_named_numbers(items, "Justification") {
        if let Some(v) = nums.first() {
            digital.justification = (*v).max(0.0) as u32;
        }
    }
    digital
}

fn parse_mouse_control(items: &[Ast]) -> bool {
    find_named_numbers(items, "MouseControl")
        .and_then(|n| n.first().copied())
        .is_some_and(|v| v != 0.0)
}

fn parse_control_style(items: &[Ast]) -> Option<String> {
    find_string_in_list(items, "Style").map(|s| s.to_ascii_uppercase())
}

fn parse_two_state(items: &[Ast]) -> Result<CabControl, FormatError> {
    let control_type = parse_control_type(items)?;
    let position = find_screen_rect(items, "TwoStateDisplay")?;
    let graphic = find_string_in_list(items, "Graphic").unwrap_or_default();
    Ok(CabControl::TwoStateDisplay {
        control_type,
        position,
        graphic,
        frames: normalize_lever_frames(parse_lever_frames(items)),
        mouse_control: parse_mouse_control(items),
        style: parse_control_style(items),
    })
}

fn parse_tri_state(items: &[Ast]) -> Result<CabControl, FormatError> {
    let control_type = parse_control_type(items)?;
    let position = find_screen_rect(items, "TriStateDisplay").ok();
    let graphic = find_string_in_list(items, "Graphic").unwrap_or_default();
    Ok(CabControl::TriStateDisplay {
        control_type,
        position: position.unwrap_or(ScreenRect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        }),
        graphic,
        frames: normalize_lever_frames(parse_lever_frames(items)),
        mouse_control: parse_mouse_control(items),
        style: parse_control_style(items),
    })
}

fn parse_lever(items: &[Ast]) -> Result<CabControl, FormatError> {
    let control_type = parse_control_type(items)?;
    let position = find_screen_rect(items, "Lever").ok();
    let graphic = find_string_in_list(items, "Graphic").unwrap_or_default();
    let frames = normalize_lever_frames(parse_lever_frames(items));
    Ok(CabControl::Lever {
        control_type,
        position,
        graphic,
        frames,
        mouse_control: parse_mouse_control(items),
        style: parse_control_style(items),
    })
}

fn parse_lever_frames(items: &[Ast]) -> CabLeverFrames {
    let mut frames = CabLeverFrames::default();
    if let Some(nums) = find_named_numbers(items, "NumFrames") {
        if !nums.is_empty() {
            frames.frames_count = nums[0].max(0.0) as u32;
        }
        if nums.len() >= 2 {
            frames.frames_x = nums[1].max(0.0) as u32;
        }
        if nums.len() >= 3 {
            frames.frames_y = nums[2].max(0.0) as u32;
        }
    }
    if let Some(nums) = find_named_numbers(items, "NumPositions") {
        // Count is nums[0]; optional explicit position indices follow.
        let _count = nums.first().copied().unwrap_or(0.0);
        let _ = _count;
    }
    if let Some(nums) = find_named_numbers(items, "NumValues") {
        if nums.len() > 1 {
            frames.values = nums[1..].to_vec();
        }
    }
    if let Some(nums) = find_named_numbers(items, "Orientation") {
        if let Some(v) = nums.first() {
            frames.orientation = *v as i32;
        }
    }
    if let Some(nums) = find_named_numbers(items, "DirIncrease") {
        if let Some(v) = nums.first() {
            frames.dir_increase = *v != 0.0;
        }
    }
    if let Some(nums) = find_named_numbers(items, "ScaleRange") {
        if nums.len() >= 2 {
            frames.min_value = nums[0];
            frames.max_value = nums[1];
        }
    }
    frames
}

/// Fill missing frame metadata the way Open Rails does for abbreviated levers.
fn normalize_lever_frames(mut frames: CabLeverFrames) -> CabLeverFrames {
    if frames.frames_count == 0 && frames.frames_x > 0 && frames.frames_y > 0 {
        frames.frames_count = frames.frames_x.saturating_mul(frames.frames_y);
    }
    if frames.frames_x == 0 && frames.frames_count > 0 {
        frames.frames_x = frames.frames_count;
        frames.frames_y = 1;
    }
    if frames.values.len() <= 1 && frames.frames_count > 1 {
        let n = frames.frames_count as usize;
        let span = frames.max_value - frames.min_value;
        frames.values = (0..n)
            .map(|i| frames.min_value + span * (i as f64) / ((n - 1) as f64))
            .collect();
    } else if frames.values.is_empty() && frames.frames_count == 1 {
        frames.values.push(frames.min_value);
    }
    if frames.values.len() >= 2 && frames.values[0] > *frames.values.last().unwrap_or(&0.0) {
        frames.values.reverse();
    }
    frames
}

fn find_named_numbers(items: &[Ast], key: &str) -> Option<Vec<f64>> {
    if let Some(nums) = walk_lists_find(&Ast::List(items.to_vec()), &mut |list| {
        if list.len() >= 2 {
            if let Ast::Atom(Atom::Symbol(head)) = &list[0] {
                if head.eq_ignore_ascii_case(key) {
                    let nums = flatten_numbers(&list[1..]);
                    if !nums.is_empty() {
                        return Some(nums);
                    }
                }
            }
        }
        None
    }) {
        return Some(nums);
    }
    for (i, item) in items.iter().enumerate() {
        if let Ast::Atom(Atom::Symbol(head)) = item {
            if head.eq_ignore_ascii_case(key) {
                if let Some(rest) = items.get(i + 1..) {
                    let nums = flatten_numbers(rest);
                    if !nums.is_empty() {
                        return Some(nums);
                    }
                }
            }
        }
    }
    None
}

fn flatten_numbers(items: &[Ast]) -> Vec<f64> {
    let mut nums = Vec::new();
    for item in items {
        match item {
            Ast::Atom(atom) => {
                if let Some(n) = atom_to_number(atom) {
                    nums.push(n);
                }
            }
            Ast::List(sub) => nums.extend(flatten_numbers(sub)),
        }
    }
    nums
}

fn parse_control_type(items: &[Ast]) -> Result<ControlType, FormatError> {
    let tokens = find_type_tokens(items)?;
    Ok(ControlType::from_type_tokens(&tokens))
}

fn find_type_tokens(items: &[Ast]) -> Result<Vec<String>, FormatError> {
    if let Some(tokens) = walk_lists_find(&Ast::List(items.to_vec()), &mut |list| {
        if list.len() >= 2 {
            if let Ast::Atom(Atom::Symbol(head)) = &list[0] {
                if head.eq_ignore_ascii_case("Type") {
                    return Some(collect_type_token_strings(&list[1..]));
                }
            }
        }
        None
    }) {
        return Ok(tokens);
    }

    for (i, item) in items.iter().enumerate() {
        if let Ast::Atom(Atom::Symbol(head)) = item {
            if head.eq_ignore_ascii_case("Type") {
                if let Some(next) = items.get(i + 1) {
                    return Ok(match next {
                        Ast::List(sub) => collect_type_token_strings(sub),
                        Ast::Atom(atom) => atom_to_string(atom).into_iter().collect(),
                    });
                }
            }
        }
    }

    Err(FormatError::MissingField {
        key: "Type".to_string(),
        context: "CabControl".to_string(),
    })
}

fn collect_type_token_strings(items: &[Ast]) -> Vec<String> {
    let mut tokens = Vec::new();
    for item in items {
        match item {
            Ast::Atom(atom) => {
                if let Some(s) = atom_to_string(atom) {
                    tokens.push(s);
                }
            }
            Ast::List(sub) => tokens.extend(collect_type_token_strings(sub)),
        }
    }
    tokens
}

fn find_screen_rect(items: &[Ast], context: &str) -> Result<ScreenRect, FormatError> {
    if let Some(rect) = walk_lists_find(&Ast::List(items.to_vec()), &mut |list| {
        if list.len() >= 2 {
            if let Ast::Atom(Atom::Symbol(head)) = &list[0] {
                if head.eq_ignore_ascii_case("Position") {
                    return parse_screen_rect(list, "Position").ok();
                }
            }
        }
        None
    }) {
        return Ok(rect);
    }

    for (i, item) in items.iter().enumerate() {
        if let Ast::Atom(Atom::Symbol(head)) = item {
            if head.eq_ignore_ascii_case("Position") {
                if let Some(next) = items.get(i + 1) {
                    return parse_screen_rect_value(None, Some(next));
                }
            }
        }
    }

    Err(FormatError::MissingField {
        key: "Position".to_string(),
        context: context.to_string(),
    })
}

fn parse_screen_rect(items: &[Ast], context: &str) -> Result<ScreenRect, FormatError> {
    let nums = parse_coordinate_numbers(items)?;
    if nums.len() >= 4 {
        return Ok(ScreenRect {
            x: nums[0],
            y: nums[1],
            width: nums[2],
            height: nums[3],
        });
    }
    Err(FormatError::UnexpectedAtom {
        key: context.to_string(),
        context: "cvf".to_string(),
        expected: "four numbers (x y width height)".to_string(),
    })
}

fn parse_vec3(items: &[Ast], context: &str) -> Result<[f64; 3], FormatError> {
    let nums = parse_coordinate_numbers(items)?;
    if nums.len() >= 3 {
        return Ok([nums[0], nums[1], nums[2]]);
    }
    Err(FormatError::UnexpectedAtom {
        key: context.to_string(),
        context: "cvf".to_string(),
        expected: "three numbers".to_string(),
    })
}

/// MSTS cab files use flat `(Key n n n)`, nested `(Key (n n n))`, or value-only `(n n n)`.
fn parse_coordinate_numbers(items: &[Ast]) -> Result<Vec<f64>, FormatError> {
    if items
        .first()
        .is_some_and(|a| matches!(a, Ast::Atom(Atom::Number(_) | Atom::Integer(_))))
    {
        let nums: Vec<f64> = items
            .iter()
            .filter_map(|a| match a {
                Ast::Atom(atom) => atom_to_number(atom),
                _ => None,
            })
            .collect();
        if !nums.is_empty() {
            return Ok(nums);
        }
    }
    if items.len() >= 2 {
        if let Ast::List(coords) = &items[1] {
            let nums: Vec<f64> = coords
                .iter()
                .filter_map(|a| match a {
                    Ast::Atom(atom) => atom_to_number(atom),
                    _ => None,
                })
                .collect();
            if !nums.is_empty() {
                return Ok(nums);
            }
        } else {
            let nums: Vec<f64> = items[1..]
                .iter()
                .filter_map(|a| match a {
                    Ast::Atom(atom) => atom_to_number(atom),
                    _ => None,
                })
                .collect();
            if !nums.is_empty() {
                return Ok(nums);
            }
        }
    }
    Ok(collect_numbers(items))
}

fn parse_states(items: &[Ast]) -> Result<Vec<ControlState>, FormatError> {
    let mut states = Vec::new();
    collect_states(&Ast::List(items.to_vec()), &mut states);
    Ok(states)
}

fn collect_states(ast: &Ast, out: &mut Vec<ControlState>) {
    if let Ast::List(list) = ast {
        if list.len() >= 2 {
            if let Ast::Atom(Atom::Symbol(head)) = &list[0] {
                if head.eq_ignore_ascii_case("State") {
                    if let Ok(state) = parse_state(list) {
                        out.push(state);
                    }
                }
            }
        }
        for item in list {
            collect_states(item, out);
        }
    }
}

fn parse_state(items: &[Ast]) -> Result<ControlState, FormatError> {
    let mut style = 0u32;
    let mut switch_val = 0.0;
    for item in items.iter().skip(1) {
        let Ast::List(entry) = item else {
            continue;
        };
        let Some(Ast::Atom(Atom::Symbol(key))) = entry.first() else {
            continue;
        };
        match key.to_ascii_uppercase().as_str() {
            "STYLE" => {
                if let Ok(nums) = parse_coordinate_numbers(entry) {
                    if let Some(v) = nums.first() {
                        style = *v as u32;
                    }
                }
            }
            "SWITCHVAL" => {
                if let Ok(nums) = parse_coordinate_numbers(entry) {
                    if let Some(v) = nums.first() {
                        switch_val = *v;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(ControlState { style, switch_val })
}

fn parse_u32_field(items: &[Ast], context: &str) -> Result<Option<u32>, FormatError> {
    parse_coordinate_numbers(items)?
        .first()
        .map(|v| *v as u32)
        .map(Some)
        .ok_or_else(|| FormatError::MissingField {
            key: context.to_string(),
            context: "cvf".to_string(),
        })
}

fn parse_string_field(items: &[Ast], context: &str) -> Result<Option<String>, FormatError> {
    if items.len() >= 2 {
        if let Ast::Atom(atom) = &items[1] {
            return atom_to_string(atom)
                .map(Some)
                .ok_or_else(|| FormatError::UnexpectedAtom {
                    key: context.to_string(),
                    context: "cvf".to_string(),
                    expected: "string".to_string(),
                });
        }
    }
    Ok(None)
}

fn find_string_in_list(items: &[Ast], key: &str) -> Option<String> {
    if let Some(value) = walk_lists_find(&Ast::List(items.to_vec()), &mut |list| {
        if list.len() >= 2 {
            if let Ast::Atom(Atom::Symbol(head)) = &list[0] {
                if head.eq_ignore_ascii_case(key) {
                    match &list[1] {
                        Ast::Atom(atom) => return atom_to_string(atom),
                        Ast::List(sub) => {
                            if let Some(Ast::Atom(atom)) = sub.first() {
                                return atom_to_string(atom);
                            }
                        }
                    }
                }
            }
        }
        None
    }) {
        return Some(value);
    }

    for (i, item) in items.iter().enumerate() {
        if let Ast::Atom(Atom::Symbol(head)) = item {
            if head.eq_ignore_ascii_case(key) {
                if let Some(Ast::Atom(atom)) = items.get(i + 1) {
                    return atom_to_string(atom);
                }
                if let Some(Ast::List(sub)) = items.get(i + 1) {
                    if let Some(Ast::Atom(atom)) = sub.first() {
                        return atom_to_string(atom);
                    }
                }
            }
        }
    }
    None
}

fn collect_numbers(items: &[Ast]) -> Vec<f64> {
    let mut nums = Vec::new();
    for item in items.iter().skip(1) {
        match item {
            Ast::Atom(atom) => {
                if let Some(n) = atom_to_number(atom) {
                    nums.push(n);
                }
            }
            Ast::List(sub) => {
                nums.extend(collect_numbers(sub));
            }
        }
    }
    nums
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_from_first_paren;

    #[test]
    fn control_type_from_tokens() {
        assert_eq!(
            ControlType::from_type_tokens(&[
                "DIRECTION_DISPLAY".into(),
                "MULTI_STATE_DISPLAY".into()
            ]),
            ControlType::DirectionDisplay
        );
        assert_eq!(
            ControlType::from_type_tokens(&["CUSTOM_GAUGE".into()]),
            ControlType::Generic("CUSTOM_GAUGE".into())
        );
    }

    #[test]
    fn parse_minimal_fixture_shape() {
        let src = r#"
(Tr_CabViewFile
  (CabViewType 2)
  (CabViewFile "panel.ace")
  (CabViewWindow (0 0 800 600))
  (Position (1.0 2.0 3.0))
  (Direction (0 0 0))
  (CabViewControls
    (MultiStateDisplay
      (Type (DIRECTION_DISPLAY MULTI_STATE_DISPLAY))
      (Position (10 20 18 11))
      (Graphic "reverser.ace")
      (States (3 3 1
        (State (Style (0)) (SwitchVal (-1)))
        (State (Style (0)) (SwitchVal (0)))
        (State (Style (0)) (SwitchVal (1)))
      ))
    )
    (MultiStateDisplay
      (Type (THROTTLE_DISPLAY MULTI_STATE_DISPLAY))
      (Position (10 40 18 11))
      (Graphic "throttle.ace")
      (States (2 2 1
        (State (Style (0)) (SwitchVal (0)))
        (State (Style (0)) (SwitchVal (1)))
      ))
    )
  )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let cvf = CabViewFile::from_ast(&ast).expect("typed");
        assert_eq!(cvf.cab_view_type, Some(2));
        assert_eq!(cvf.views.len(), 1);
        assert_eq!(cvf.views[0].texture_ace, "panel.ace");
        assert_eq!(cvf.controls.len(), 2);
        match &cvf.controls[0] {
            CabControl::MultiStateDisplay { states, .. } => {
                assert_eq!(states.len(), 3);
                assert!((states[2].switch_val - 1.0).abs() < 1e-9);
            }
            other => panic!("expected MultiStateDisplay, got {other:?}"),
        }
    }

    #[test]
    fn parse_lever_num_frames_and_positions() {
        let src = r#"
(Tr_CabViewFile
  (CabViewType 2)
  (CabViewFile "panel.ace")
  (CabViewWindow (0 0 640 480))
  (CabViewControls
    (Lever
      (Type ( THROTTLE LEVER ))
      (Position (471 374 169 106))
      (Graphic "Throttle.ace")
      (NumFrames ( 10 5 2 ))
      (NumPositions ( 10 ))
      (NumValues ( 10 ))
      (Orientation ( 1 ))
      (DirIncrease ( 0 ))
      (ScaleRange ( 0 1 ))
    )
    (Lever
      (Type ( TRAIN_BRAKE LEVER ))
      (Position (53 344 150 98))
      (Graphic "BrakeHandle.ace")
      (NumFrames ( 22 11 2 ))
      (NumPositions ( 22 ))
      (Orientation ( 0 ))
      (DirIncrease ( 0 ))
      (ScaleRange ( 0 1 ))
    )
  )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let cvf = CabViewFile::from_ast(&ast).expect("typed");
        assert_eq!(cvf.controls.len(), 2);
        match &cvf.controls[0] {
            CabControl::Lever { frames, graphic, .. } => {
                assert_eq!(graphic, "Throttle.ace");
                assert_eq!(frames.frames_count, 10);
                assert_eq!(frames.frames_x, 5);
                assert_eq!(frames.frames_y, 2);
                assert_eq!(frames.orientation, 1);
                assert!(!frames.dir_increase);
                assert_eq!(frames.values.len(), 10);
                assert_eq!(frames.percent_to_index(0.0), 0);
                assert_eq!(frames.percent_to_index(1.0), 9);
                assert_eq!(frames.percent_to_index(0.35), 3);
            }
            other => panic!("expected Lever, got {other:?}"),
        }
        match &cvf.controls[1] {
            CabControl::Lever { frames, .. } => {
                assert_eq!(frames.frames_count, 22);
                assert_eq!(frames.frames_x, 11);
                assert_eq!(frames.frames_y, 2);
                assert_eq!(frames.values.len(), 22);
                assert_eq!(frames.percent_to_index(0.0), 0);
                assert_eq!(frames.percent_to_index(1.0), 21);
            }
            other => panic!("expected Lever, got {other:?}"),
        }
    }

    #[test]
    fn lever_frame_rect_is_row_major() {
        let frames = CabLeverFrames {
            frames_count: 10,
            frames_x: 5,
            frames_y: 2,
            ..Default::default()
        };
        let (x, y, w, h) = frames.frame_rect(500.0, 200.0, 0);
        assert!((w - 100.0).abs() < 1e-3 && (h - 100.0).abs() < 1e-3);
        assert!((x - 0.0).abs() < 1e-3 && (y - 0.0).abs() < 1e-3);
        let (x, y, _, _) = frames.frame_rect(500.0, 200.0, 5);
        assert!((x - 0.0).abs() < 1e-3 && (y - 100.0).abs() < 1e-3);
        let (x, y, _, _) = frames.frame_rect(500.0, 200.0, 6);
        assert!((x - 100.0).abs() < 1e-3 && (y - 100.0).abs() < 1e-3);
    }

    #[test]
    fn parse_screen_display_etcs() {
        let src = r#"
(Tr_CabViewFile
  (CabViewType 2)
  (CabViewFile "panel.ace")
  (CabViewWindow (0 0 640 480))
  (CabViewControls
    (ScreenDisplay
      (Type ( ORTS_ETCS SCREEN_DISPLAY ))
      (Position (0 0 640 480))
      (Graphic ( statictexture.ace ))
      (Parameters ( Mode Full Size 640 ))
      (HideIfDisabled ( 1 ))
    )
  )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let cvf = CabViewFile::from_ast(&ast).expect("typed");
        match &cvf.controls[0] {
            CabControl::Screen {
                control_type,
                graphic,
                parameters,
                hide_if_disabled,
                ..
            } => {
                assert_eq!(*control_type, ControlType::OrtsEtcs);
                assert!(graphic.to_ascii_lowercase().contains("statictexture"));
                assert!(*hide_if_disabled);
                assert_eq!(parameters.get("mode").map(String::as_str), Some("full"));
            }
            other => panic!("expected Screen, got {other:?}"),
        }
    }

    #[test]
    fn parse_gauge_orientation_colour_and_style() {
        let src = r#"
(Tr_CabViewFile
  (CabViewType 2)
  (CabViewFile "panel.ace")
  (CabViewWindow (0 0 640 480))
  (CabViewControls
    (Gauge
      (Type ( AMMETER ))
      (Position (10 10 20 80))
      (Style ( SOLID ))
      (ScaleRange ( -100 100 ))
      (Orientation ( 1 ))
      (DirIncrease ( 1 ))
      (Units ( AMPS ))
      (PositiveColour ( 1 (ControlColour ( 0 255 0 )) ))
      (NegativeColour ( 1 (ControlColour ( 255 0 0 )) ))
    )
    (Gauge
      (Type ( BRAKE_PIPE ))
      (Position (0 0 10 10))
      (Style ( POINTER ))
      (ScaleRange ( 0 5 ))
    )
  )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let cvf = CabViewFile::from_ast(&ast).expect("typed");
        match &cvf.controls[0] {
            CabControl::Gauge { gauge, .. } => {
                assert_eq!(gauge.orientation, 1);
                assert_eq!(gauge.direction, 1);
                assert!(!gauge.is_pointer());
                assert_eq!(gauge.units.as_deref(), Some("AMPS"));
                let pos = gauge.positive_colour.expect("pos");
                assert!((pos[2] - 1.0).abs() < 1e-3); // G
                assert!((gauge.range_fraction(0.0, true) - 0.0).abs() < 1e-6);
                assert!((gauge.range_fraction(100.0, true) - 0.5).abs() < 1e-6);
                // offsetFromZero: numerador = data − 0 → −100/200 = −0.5
                assert!((gauge.range_fraction(-100.0, true) + 0.5).abs() < 1e-6);
                assert!((gauge.range_fraction(-101.0, true) - 0.0).abs() < 1e-6);
            }
            other => panic!("expected Gauge, got {other:?}"),
        }
        match &cvf.controls[1] {
            CabControl::Gauge { gauge, .. } => assert!(gauge.is_pointer()),
            other => panic!("expected Gauge POINTER, got {other:?}"),
        }
    }

    #[test]
    fn parse_dial_scale_pos_and_pivot() {
        let src = r#"
(Tr_CabViewFile
  (CabViewType 2)
  (CabViewFile "panel.ace")
  (CabViewWindow (0 0 640 480))
  (CabViewControls
    (Dial
      (Type ( SPEEDOMETER DIAL ))
      (Position (427 303 19 30))
      (Graphic "../../KIHA31/CabView/KMHNeedle.ace")
      (Style ( NEEDLE ))
      (ScaleRange ( 0 100 ))
      (ScalePos ( 190 150 ))
      (Units ( MILES_PER_HOUR ))
      (Pivot ( 21 ))
      (DirIncrease ( 0 ))
    )
  )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let cvf = CabViewFile::from_ast(&ast).expect("typed");
        match &cvf.controls[0] {
            CabControl::Dial { dial, graphic, .. } => {
                assert!(graphic.contains("KMHNeedle"));
                assert!((dial.scale_min - 0.0).abs() < 1e-9);
                assert!((dial.scale_max - 100.0).abs() < 1e-9);
                assert!((dial.from_degree - 190.0).abs() < 1e-9);
                assert!((dial.to_degree - 150.0).abs() < 1e-9);
                assert_eq!(dial.pivot, Some(21.0));
                assert!(!dial.dir_increase);
                assert_eq!(dial.units.as_deref(), Some("MILES_PER_HOUR"));
                assert!((dial.range_fraction(50.0) - 0.5).abs() < 1e-6);
                let angle0 = dial.rotation_radians(0.0);
                let angle1 = dial.rotation_radians(100.0);
                assert!(angle0.is_finite() && angle1.is_finite());
                assert!((angle0 - angle1).abs() > 0.01);
            }
            other => panic!("expected Dial, got {other:?}"),
        }
    }

    #[test]
    fn parse_tristate_num_frames() {
        let src = r#"
(Tr_CabViewFile
  (CabViewType 2)
  (CabViewFile "panel.ace")
  (CabViewWindow (0 0 640 480))
  (CabViewControls
    (TriState
      (Type ( FRONT_HLIGHT TRI_STATE ))
      (Position (339 411 42 25))
      (Graphic "Headlight.ace")
      (NumFrames ( 3 3 1 ))
      (Style ( NONE ))
      (Orientation ( 0 ))
      (DirIncrease ( 1 ))
    )
    (TwoState
      (Type ( HORN TWO_STATE ))
      (Position (193 378 36 57))
      (Graphic "hornlever.ace")
      (NumFrames ( 2 2 1 ))
    )
  )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let cvf = CabViewFile::from_ast(&ast).expect("typed");
        match &cvf.controls[0] {
            CabControl::TriStateDisplay { frames, .. } => {
                assert_eq!(frames.frames_count, 3);
                assert_eq!(frames.frames_x, 3);
                assert_eq!(frames.frames_y, 1);
            }
            other => panic!("expected TriStateDisplay, got {other:?}"),
        }
        match &cvf.controls[1] {
            CabControl::TwoStateDisplay { frames, .. } => {
                assert_eq!(frames.frames_count, 2);
                assert_eq!(frames.frames_x, 2);
                assert_eq!(frames.frames_y, 1);
            }
            other => panic!("expected TwoStateDisplay, got {other:?}"),
        }
    }

    #[test]
    fn parse_digital_accuracy_and_leading_zeros() {
        let src = r#"
(Tr_CabViewFile
  (CabViewType 2)
  (CabViewFile "panel.ace")
  (CabViewWindow (0 0 640 480))
  (CabViewControls
    (Digital
      (Type ( SPEEDOMETER DIGITAL ))
      (Position (102 282 16 16))
      (ScaleRange ( 0 99 ))
      (Accuracy ( 0 ))
      (LeadingZeros ( 2 ))
      (Justification ( 3 ))
      (Units ( MILES_PER_HOUR ))
    )
  )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let cvf = CabViewFile::from_ast(&ast).expect("typed");
        match &cvf.controls[0] {
            CabControl::Digital { digital, .. } => {
                assert_eq!(digital.scale_max, 99.0);
                assert_eq!(digital.leading_zeros, 2);
                assert_eq!(digital.justification, 3);
                assert_eq!(digital.units.as_deref(), Some("MILES_PER_HOUR"));
                assert_eq!(digital.format_value(7.0), "07");
                assert_eq!(digital.format_value(42.0), "42");
            }
            other => panic!("expected Digital, got {other:?}"),
        }
    }

    #[test]
    fn parse_mouse_control_and_style_on_lever() {
        let src = r#"
(Tr_CabViewFile
  (CabViewType 2)
  (CabViewFile "panel.ace")
  (CabViewWindow (0 0 640 480))
  (CabViewControls
    (Lever
      (Type ( THROTTLE LEVER ))
      (Position (471 374 169 106))
      (Graphic "Throttle.ace")
      (NumFrames ( 10 5 2 ))
      (MouseControl ( 1 ))
      (Style ( SPRUNG ))
    )
    (TwoState
      (Type ( HORN TWO_STATE ))
      (Position (193 378 36 57))
      (Graphic "hornlever.ace")
      (NumFrames ( 2 2 1 ))
      (Style ( ONOFF ))
      (MouseControl ( 1 ))
    )
  )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let cvf = CabViewFile::from_ast(&ast).expect("typed");
        match &cvf.controls[0] {
            CabControl::Lever {
                mouse_control,
                style,
                ..
            } => {
                assert!(*mouse_control);
                assert_eq!(style.as_deref(), Some("SPRUNG"));
            }
            other => panic!("expected Lever, got {other:?}"),
        }
        match &cvf.controls[1] {
            CabControl::TwoStateDisplay {
                mouse_control,
                style,
                ..
            } => {
                assert!(*mouse_control);
                assert_eq!(style.as_deref(), Some("ONOFF"));
            }
            other => panic!("expected TwoState, got {other:?}"),
        }
    }
}
