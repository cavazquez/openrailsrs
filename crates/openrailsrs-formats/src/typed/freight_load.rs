//! Continuous authored wagon loads. Alternative material animations are one payload.
use super::{named_blocks, scalar_text};
use crate::{Ast, FormatError};
pub use openrailsrs_core::activity::FreightProfile;
fn text(ast: &Ast, key: &str) -> Option<String> {
    let block = named_blocks(ast, key).first().copied()?;
    let Ast::List(items) = block else { return None };
    items
        .iter()
        .filter_map(scalar_text)
        .find(|s| !s.eq_ignore_ascii_case(key))
}
pub fn parse_freight_profile(ast: &Ast) -> Result<Option<FreightProfile>, FormatError> {
    let Some(anims) = named_blocks(ast, "ORTSFreightAnims").first().copied() else {
        return Ok(None);
    };
    // Native continuous animations also describe tender fuel mass and
    // resistance without a transferable wagon intake. Existing steam fuel
    // state owns those consumables; do not create a second cargo reservoir.
    let continuous = named_blocks(anims, "FreightAnimContinuous")
        .into_iter()
        .filter(|animation| !named_blocks(animation, "IntakePoint").is_empty())
        .collect::<Vec<_>>();
    if continuous.is_empty() {
        return Ok(None);
    }
    let error = || FormatError::UnexpectedAtom {
        key: "FreightAnimContinuous".into(),
        context: "native freight".into(),
        expected: "finite compatible payload alternatives and WagonEmptyWeight".into(),
    };
    let empty_mass_kg = text(anims, "WagonEmptyWeight")
        .and_then(|s| crate::parse_mass_kg(&s))
        .ok_or_else(error)?;
    let delay = text(anims, "UnloadingStartDelay")
        .map(|s| s.parse::<f64>())
        .transpose()
        .map_err(|_| error())?
        .unwrap_or(0.);
    let mut profile: Option<FreightProfile> = None;
    for animation in continuous {
        let capacity = text(animation, "FreightWeightWhenFull")
            .and_then(|s| crate::parse_mass_kg(&s))
            .ok_or_else(error)?;
        let intakes = crate::parse_intake_points(animation);
        if intakes.is_empty() {
            return Err(error());
        }
        // OR initializes FullAtStart=false; an authored empty bool block
        // instead invokes ReadBoolBlock(true). Absence is not an empty block.
        let full = if let Some(block) = named_blocks(animation, "FullAtStart").last() {
            match text(block, "FullAtStart")
                .as_deref()
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                None | Some("true") => true,
                Some("false") => false,
                Some(value) => value.parse::<i32>().map(|v| v != 0).map_err(|_| error())?,
            }
        } else {
            false
        };
        let p = profile.get_or_insert(FreightProfile {
            empty_mass_kg,
            capacity_kg: capacity,
            initial_kg: if full { capacity } else { 0. },
            pickup_types: vec![],
            unloading_delay_s: delay,
        });
        if (p.capacity_kg - capacity).abs() > 0.001
            || (p.initial_kg - if full { capacity } else { 0. }).abs() > 0.001
        {
            return Err(error());
        }
        for intake in intakes {
            if !p.pickup_types.contains(&intake.pickup_type) {
                p.pickup_types.push(intake.pickup_type);
            }
        }
    }
    if profile.as_ref().is_none_or(|p| !p.valid()) {
        return Err(error());
    }
    Ok(profile)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_freight_alternatives_share_capacity_and_keep_empty_mass() {
        let ast=crate::parse_vehicle_text("Wagon ( ORTSFreightAnims ( WagonEmptyWeight ( 10t-uk ) UnloadingStartDelay ( 5 ) FreightAnimContinuous ( IntakePoint ( 0 6 FreightCoal ) FreightWeightWhenFull ( 16t-uk ) FullAtStart ( 0 ) ) FreightAnimContinuous ( IntakePoint ( 0 6 FuelCoal ) FreightWeightWhenFull ( 16t-uk ) FullAtStart ( 0 ) ) ) )").unwrap();
        let p = parse_freight_profile(&ast).unwrap().unwrap();
        assert!((p.capacity_kg - 16256.8).abs() < 0.01);
        assert_eq!(p.initial_kg, 0.);
        assert_eq!(p.pickup_types, [2, 6]);
        assert_eq!(p.unloading_delay_s, 5.);
    }

    #[test]
    fn continuous_tender_mass_without_cargo_intake_is_not_a_second_payload() {
        let ast = crate::parse_vehicle_text("Wagon ( Type ( Tender ) IntakePoint ( 0 6 FuelCoal ) ORTSFreightAnims ( WagonEmptyWeight ( 20.525t-uk ) FreightAnimContinuous ( FreightWeightWhenFull ( 21.625t-uk ) ) ) )").unwrap();
        assert!(parse_freight_profile(&ast).unwrap().is_none());
    }

    #[test]
    fn native_full_at_start_absence_differs_from_an_empty_bool_block() {
        for (token, full) in [
            ("", false),
            ("FullAtStart ()", true),
            ("FullAtStart (0)", false),
            ("FullAtStart (true)", true),
            ("FullAtStart (false)", false),
            ("FullAtStart (2)", true),
        ] {
            let ast=crate::parse_vehicle_text(&format!("Wagon ( ORTSFreightAnims ( WagonEmptyWeight (1t) FreightAnimContinuous ( IntakePoint (0 6 FreightCoal) FreightWeightWhenFull (2t) {token} ) ) )")).unwrap();
            let profile = parse_freight_profile(&ast).unwrap().unwrap();
            assert_eq!(profile.initial_kg, if full { 2000. } else { 0. }, "{token}");
        }
    }
}
