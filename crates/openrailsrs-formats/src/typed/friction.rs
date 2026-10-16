//! Open Rails rolling-resistance metadata from `.wag` / `.eng` files.

use crate::ast::{Ast, Atom};
use crate::msts_units::parse_length_m;

use super::{atom_to_string, find_list_value, find_optional_string_field};

/// OR `ORTSBearingType` (see `MSTSWagon.BearingTypes`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OrtsBearingType {
    #[default]
    Default,
    Grease,
    Friction,
    Roller,
    Low,
}

/// OR wagon/car category for Davis B estimation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OrtsWagonType {
    #[default]
    Freight,
    Passenger,
    Engine,
    Tender,
}

/// Parsed friction-related fields for auto Davis calculation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrtsFrictionFields {
    pub bearing_type: OrtsBearingType,
    pub wagon_type: OrtsWagonType,
    /// `ORTSNumberAxles` or inferred from `NumWheels`.
    pub num_axles: Option<u32>,
    /// Locomotive driven axles (`ORTSNumberDriveAxles`).
    pub num_drive_axles: Option<u32>,
    /// MSTS `NumWheels` (legacy axle hint).
    pub num_wheels: Option<u32>,
    pub frontal_area_m2: Option<f64>,
    pub drag_constant: Option<f64>,
    pub car_width_m: Option<f64>,
    pub car_height_m: Option<f64>,
    /// Third MSTS Friction value. Invalid FCalc speeds select OR's legacy
    /// resistance formula, rather than the newer auto-Davis estimation.
    pub legacy_friction_speed_mps: Option<f64>,
    /// Original MSTS `C1, E1, V2, C2, E2`. Each input is parsed to Single,
    /// scaled with STFReader's units, then stored as f64 without losing its
    /// native rounding. C1/C2 use Resistance units; V2 is metres per second.
    pub legacy_friction: Option<[f64; 5]>,
}

impl OrtsFrictionFields {
    /// Bearing type used for auto-friction when `Default` (matches OR runtime path).
    pub fn effective_bearing_type(&self) -> OrtsBearingType {
        if self.bearing_type == OrtsBearingType::Default {
            OrtsBearingType::Friction
        } else {
            self.bearing_type
        }
    }

    /// Total axles for Davis A/B (wagon axles + loco drive axles when applicable).
    pub fn total_axles(&self, is_loco: bool) -> u32 {
        let drive = self.num_drive_axles.unwrap_or(0);
        let wagon_axles = self
            .num_axles
            .or_else(|| self.num_wheels.filter(|&w| w > 0 && w < 6))
            .unwrap_or(if is_loco { 0 } else { 4 });
        let base = if is_loco {
            wagon_axles.max(drive)
        } else {
            wagon_axles
        };
        base + if is_loco { drive } else { 0 }
    }

    pub fn frontal_area_m2(&self) -> f64 {
        if let Some(a) = self.frontal_area_m2.filter(|a| *a > 0.0) {
            return a;
        }
        let w = self.car_width_m.unwrap_or(0.0);
        let h = self.car_height_m.unwrap_or(0.0);
        if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
    }

    /// `MSTSWagon.UpdateTrainBaseResistance_ORTS` starting resistance. This
    /// applies only when the caller selects legacy friction rather than an
    /// authored complete Davis triple. The mass is required for the fallback
    /// used when V2 is outside the FCalc speed range.
    pub fn legacy_start_force_n(&self, mass_kg: f64) -> Option<f64> {
        let values = self.legacy_friction?;
        if !values.iter().all(|v| v.is_finite()) || !mass_kg.is_finite() || mass_kg <= 0. {
            return None;
        }
        let [c1, e1, v2, _, _] = values.map(|v| v as f32);
        let force = if !(0. ..=4.4407_f32).contains(&v2) {
            let a = 6.3743_f32 * mass_kg as f32 / 1000. + 128.998_f32 * 4.;
            a * 2.
        } else if e1 < 0. {
            // The literals inside Math.Pow are Double in the original;
            // only its result and the final multiplication are Single.
            c1 * ((0.0025_f64 * 0.44704_f64).powf(f64::from(e1)) as f32)
        } else {
            c1
        };
        (force.is_finite() && force >= 0.).then_some(f64::from(force))
    }
}

pub fn parse_orts_friction_fields(ast: &Ast, is_loco: bool, name: &str) -> OrtsFrictionFields {
    let mut fields = OrtsFrictionFields {
        wagon_type: if is_loco {
            OrtsWagonType::Engine
        } else {
            infer_wagon_type_from_name(name)
        },
        ..Default::default()
    };

    if let Some(bt) = find_optional_string_field(ast, &["ORTSBearingType"], "ORTSBearingType")
        .ok()
        .flatten()
    {
        fields.bearing_type = parse_bearing_type(&bt);
    }

    if let Some(wt) = find_optional_string_field(ast, &["WagonType", "EngineType"], "WagonType")
        .ok()
        .flatten()
    {
        fields.wagon_type = parse_wagon_type(&wt);
    }

    fields.num_axles = find_optional_u32(ast, &["ORTSNumberAxles"]);
    fields.num_drive_axles = find_optional_u32(ast, &["ORTSNumberDriveAxles"]);
    fields.num_wheels = find_optional_u32(ast, &["NumWheels"]);

    fields.frontal_area_m2 = find_optional_f64(ast, &["ORTSWagonFrontalArea"]);
    fields.drag_constant = find_optional_f64(ast, &["ORTSDavisDragConstant"]);

    parse_size_dimensions(ast, &mut fields);
    let friction_blocks = super::named_blocks(ast, "Friction");
    fields.legacy_friction = friction_blocks.first().and_then(|values| {
        let Ast::List(values) = values else { return None };
        let offset = usize::from(matches!(values.first(), Some(Ast::Atom(Atom::Symbol(s))) if s.eq_ignore_ascii_case("Friction")));
        let mut result = [0.; 5];
        for (index, output) in result.iter_mut().enumerate() {
            let Ast::Atom(value) = values.get(offset + index)? else { return None };
            *output = native_friction_quantity(value, index)?;
        }
        Some(result)
    });
    fields.legacy_friction_speed_mps = fields.legacy_friction.map(|values| values[2]);

    fields
}

fn infer_wagon_type_from_name(name: &str) -> OrtsWagonType {
    let upper = name.to_ascii_uppercase();
    if upper.contains("MK2")
        || upper.contains("MKII")
        || upper.contains("COACH")
        || upper.contains("TSO")
        || upper.contains("BSO")
        || upper.contains("FK")
        || upper.contains("PCF")
        || upper.contains("PSG")
        || upper.contains("CARRIAGE")
    {
        OrtsWagonType::Passenger
    } else {
        OrtsWagonType::Freight
    }
}

fn parse_bearing_type(s: &str) -> OrtsBearingType {
    match s.trim().to_ascii_lowercase().as_str() {
        "grease" => OrtsBearingType::Grease,
        "friction" => OrtsBearingType::Friction,
        "roller" => OrtsBearingType::Roller,
        "low" => OrtsBearingType::Low,
        _ => OrtsBearingType::Default,
    }
}

fn parse_wagon_type(s: &str) -> OrtsWagonType {
    let normalized = s.trim().replace("Carriage", "Passenger");
    match normalized.to_ascii_lowercase().as_str() {
        "passenger" => OrtsWagonType::Passenger,
        "engine" | "locomotive" => OrtsWagonType::Engine,
        "tender" => OrtsWagonType::Tender,
        _ => OrtsWagonType::Freight,
    }
}

fn parse_size_dimensions(ast: &Ast, fields: &mut OrtsFrictionFields) {
    let blocks = super::named_blocks(ast, "Size");
    let Some(Ast::List(items)) = blocks.first() else {
        return;
    };
    let offset = usize::from(
        matches!(items.first(), Some(Ast::Atom(Atom::Symbol(s))) if s.eq_ignore_ascii_case("Size")),
    );
    if items.len() >= offset + 3 {
        fields.car_width_m = parse_length(items.get(offset));
        fields.car_height_m = parse_length(items.get(offset + 1));
    }
}

fn native_friction_quantity(atom: &Atom, index: usize) -> Option<f64> {
    let (number, suffix) = if let Some(number) = super::atom_to_number(atom) {
        (number as f32, String::new())
    } else {
        let text = atom_to_string(atom)?;
        let text = text.trim();
        let end = text
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
            .map_or(text.len(), |(i, _)| i);
        let number = text[..end].parse::<f32>().ok()?;
        (number, text[end..].trim().to_ascii_lowercase())
    };
    if !number.is_finite() {
        return None;
    }
    // STFReader.ReadFloat parses the numeric token to Single before scaling
    // in Double and rounding the quantity back to Single. Preserve the native
    // Resistance conversion, including its historical lbf/mph coefficient.
    let scale = match (index, suffix.as_str()) {
        (_, "") | (0 | 3, "n/m/s" | "ns/m") | (2, "m/s") => 1.,
        (0 | 3, "lbf/mph") => 10.0264321,
        (2, "cm/s") => 0.01,
        (2, "mm/s") => 0.001,
        (2, "mph") => 0.44704,
        (2, "ft/s") => 0.3048,
        (2, "in/s") => 0.0254,
        (2, "kph" | "km/h" | "kmph" | "kmh") => 0.27777778,
        _ => return None,
    };
    let value = (scale * f64::from(number)) as f32;
    value.is_finite().then_some(f64::from(value))
}

fn parse_length(node: Option<&Ast>) -> Option<f64> {
    node.and_then(|v| match v {
        Ast::Atom(atom) => super::atom_to_number(atom)
            .or_else(|| atom_to_string(atom).and_then(|s| parse_length_m(&s))),
        Ast::List(items) => items.first().and_then(|n| parse_length(Some(n))),
    })
}

fn find_optional_u32(ast: &Ast, keys: &[&str]) -> Option<u32> {
    for key in keys {
        if let Some(v) = find_list_value(ast, key)
            && let Some(n) = parse_scalar(v)
            && n >= 0.0
        {
            return Some(n.round() as u32);
        }
    }
    None
}

fn find_optional_f64(ast: &Ast, keys: &[&str]) -> Option<f64> {
    for key in keys {
        if let Some(v) = find_list_value(ast, key)
            && let Some(n) = parse_scalar(v)
        {
            return Some(n);
        }
    }
    None
}

fn parse_scalar(value: &Ast) -> Option<f64> {
    match value {
        Ast::Atom(atom) => super::atom_to_number(atom)
            .or_else(|| atom_to_string(atom).and_then(|s| s.parse().ok())),
        Ast::List(items) => items.first().and_then(parse_scalar),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_from_first_paren;

    #[test]
    fn parses_bearing_type_and_axles() {
        let ast = parse_from_first_paren(
            r#"(Wagon
  (Name "coach")
  (Mass 34000)
  (ORTSBearingType Roller)
  (ORTSNumberAxles 4)
)"#,
        )
        .unwrap();
        let f = parse_orts_friction_fields(&ast, false, "coach");
        assert_eq!(f.bearing_type, OrtsBearingType::Roller);
        assert_eq!(f.num_axles, Some(4));
        assert_eq!(f.total_axles(false), 4);
    }

    #[test]
    fn infers_passenger_mk2_name() {
        let ast = parse_from_first_paren(r#"(Wagon (Name "MK2_TSO") (Mass 34000))"#).unwrap();
        let f = parse_orts_friction_fields(&ast, false, "MK2_TSO");
        assert_eq!(f.wagon_type, OrtsWagonType::Passenger);
        assert_eq!(f.effective_bearing_type(), OrtsBearingType::Friction);
    }

    #[test]
    fn legacy_friction_preserves_all_five_native_single_inputs_and_units() {
        let ast = parse_from_first_paren(r#"(Wagon (Friction 707 0 1mph 13.76 1.8))"#).unwrap();
        let f = parse_orts_friction_fields(&ast, true, "simple tank");
        assert_eq!(
            f.legacy_friction,
            Some([
                707.,
                0.,
                f64::from(0.44704_f32),
                f64::from(13.76_f32),
                f64::from(1.8_f32),
            ])
        );
        assert_eq!(f.legacy_friction_speed_mps, Some(f64::from(0.44704_f32)));
        assert_eq!(f.legacy_start_force_n(50_000.), Some(707.));

        let ast = parse_from_first_paren(
            r#"(Wagon Friction (1.23456789lbf/mph -0.5 1.23456789km/h 2ns/m 1.8))"#,
        )
        .unwrap();
        let f = parse_orts_friction_fields(&ast, true, "units");
        let values = f.legacy_friction.unwrap();
        let input = f64::from("1.23456789".parse::<f32>().unwrap());
        assert_eq!(values[0], f64::from((input * 10.0264321) as f32));
        assert_eq!(values[2], f64::from((input * 0.27777778) as f32));
        assert_eq!(values[3], 2.);
        assert!(f.legacy_start_force_n(50_000.).unwrap() > values[0]);
    }

    #[test]
    fn missing_or_invalid_friction_does_not_publish_an_incomplete_tuple() {
        for source in [
            "(Wagon)",
            "(Wagon (Friction 707 0 1mph 13.76))",
            "(Wagon (Friction NaN 0 1mph 13.76 1.8))",
            "(Wagon (Friction 707 0 1unknown 13.76 1.8))",
            "(Wagon (Friction 707 0 1mph 1e100 1.8))",
        ] {
            let ast = parse_from_first_paren(source).unwrap();
            let f = parse_orts_friction_fields(&ast, false, "invalid");
            assert!(f.legacy_friction.is_none(), "{source}");
            assert!(f.legacy_start_force_n(50_000.).is_none());
        }
    }

    #[test]
    fn size_width_and_height_never_use_the_length_in_flat_or_nested_stf() {
        for source in [
            "(Wagon (Size 2.688m 3.915m 9.519m))",
            "(Wagon Size (2.688m 3.915m 9.519m))",
            "(Wagon (Size (2.688m 3.915m 9.519m)))",
        ] {
            let ast = parse_from_first_paren(source).unwrap();
            let f = parse_orts_friction_fields(&ast, true, "tank");
            assert_eq!(f.car_width_m, Some(2.688));
            assert_eq!(f.car_height_m, Some(3.915));
            assert_eq!(f.frontal_area_m2(), 2.688 * 3.915);
        }
    }
}
