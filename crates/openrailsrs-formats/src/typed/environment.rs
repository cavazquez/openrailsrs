//! Lighting metadata from MSTS `.env` files. Other sky/water fields remain in the AST.

use std::path::Path;

use super::activity::find_string_field;
use super::atom_to_number;
use crate::{Ast, FormatError, parse_all_top_level_lenient, read_msts_file_to_string};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EnvironmentSun {
    pub rise_time_s: u32,
    pub set_time_s: u32,
}

impl EnvironmentSun {
    pub fn from_path(path: &Path) -> Result<Option<Self>, FormatError> {
        let text = read_msts_file_to_string(path)?;
        Ok(Self::from_ast(&Ast::List(parse_all_top_level_lenient(
            &text,
        ))))
    }

    pub fn from_ast(ast: &Ast) -> Option<Self> {
        for block in super::named_blocks(ast, "world_sky_satellite") {
            let is_sun = super::named_blocks(block, "world_sky_satellite_light")
                .iter()
                .any(|value| {
                    let Ast::List(values) = value else {
                        return false;
                    };
                    values
                        .iter()
                        .filter_map(|v| match v {
                            Ast::Atom(a) => atom_to_number(a),
                            _ => None,
                        })
                        .any(|v| v == 1.0)
                });
            if is_sun {
                let time = |key| {
                    find_string_field(block, &[key])
                        .and_then(|s| parse_time(&s))
                        .unwrap_or(0)
                };
                return Some(Self {
                    rise_time_s: time("world_sky_satellite_rise_time"),
                    set_time_s: time("world_sky_satellite_set_time"),
                });
            }
        }
        None
    }
}

fn parse_time(time: &str) -> Option<u32> {
    let parts: Vec<_> = time.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    // The native parser accepts byte-valued fields (including 24:00:00).
    let hour = u32::from(parts[0].parse::<u8>().ok()?);
    let minute = u32::from(parts[1].parse::<u8>().ok()?);
    let second = u32::from(parts[2].parse::<u8>().ok()?);
    Some(hour * 3600 + minute * 60 + second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_sun_instead_of_first_moon_satellite() {
        let ast = Ast::List(parse_all_top_level_lenient(
            r#"world ( world_sky ( world_sky_satellites ( 2
            world_sky_satellite ( world_sky_satellite_light ( 0 ) world_sky_satellite_rise_time ( "18:00:00" ) )
            world_sky_satellite ( world_sky_satellite_light ( 1 ) world_sky_satellite_rise_time ( "06:15:00" ) world_sky_satellite_set_time ( "20:45:00" ) )
        ) ) )"#,
        ));
        assert_eq!(
            EnvironmentSun::from_ast(&ast),
            Some(EnvironmentSun {
                rise_time_s: 22500,
                set_time_s: 74700
            })
        );
    }

    #[test]
    fn missing_or_invalid_times_use_native_zero_fallback() {
        let ast = Ast::List(parse_all_top_level_lenient(
            r#"(world_sky_satellite world_sky_satellite_light ( 1 ) world_sky_satellite_rise_time ( "bad" ) )"#,
        ));
        assert_eq!(
            EnvironmentSun::from_ast(&ast),
            Some(EnvironmentSun::default())
        );
    }
}
