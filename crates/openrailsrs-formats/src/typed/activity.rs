//! Parser for MSTS Activity (`.act`) files.
//!
//! An activity file drives a simulation session: it names the player's consist
//! (`.con`), the path the player should follow (`.pat`), and the start time.
//!
//! Relevant section (simplified):
//! ```text
//! (Tr_Activity
//!     (Tr_Activity_Header
//!         (Name "Retiro to Bartolomé Mitre" )
//!         (Player_Train_Init
//!             (Player_Train_Init_TD  0  1  1 )
//!             (Player_Train_Init_Cons "PATHS\\Retiro-Victoria.con" )
//!         )
//!         (Player_Path "PATHS\\Retiro-Victoria.pat" )
//!         (Season Summer )
//!         (StartTime  8  0  0 )
//!         (Duration   1  30 )
//!     )
//! )
//! ```

use crate::ast::{Ast, Atom};
use crate::error::FormatError;
use crate::parser::parse_from_first_paren;

use super::atom_to_string;

/// Parsed representation of a `.act` file.
#[derive(Clone, Debug, Default)]
pub struct ActivityFile {
    /// Human-readable activity name.
    pub name: String,
    /// Relative path to the player consist (`.con`).
    pub player_consist: String,
    /// Relative path to the player path (`.pat`).
    pub player_path: String,
    /// Start time in seconds from midnight.
    pub start_time_s: f64,
    /// Duration in seconds.
    pub duration_s: f64,
}

impl ActivityFile {
    /// Parse from a pre-built AST.
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let name = find_string_field(ast, &["Name"]).unwrap_or_default();
        let player_consist = find_string_field(ast, &["Player_Train_Init_Cons"])
            .or_else(|| find_string_field(ast, &["Player_Consist"]))
            .unwrap_or_default();
        let player_path = find_string_field(ast, &["Player_Path"]).unwrap_or_default();
        let start_time_s = parse_start_time(ast);
        let duration_s = parse_duration(ast);

        Ok(Self {
            name,
            player_consist,
            player_path,
            start_time_s,
            duration_s,
        })
    }

    /// Convenience: read and parse a `.act` file from disk.
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, FormatError> {
        let text = crate::encoding::read_msts_file_to_string(path.as_ref())?;
        let ast = parse_from_first_paren(&text)?;
        Self::from_ast(&ast)
    }
}

/// Recursively find the first string value of a field with any of the given names.
fn find_string_field(ast: &Ast, names: &[&str]) -> Option<String> {
    let Ast::List(items) = ast else { return None };

    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        for n in names {
            if head.eq_ignore_ascii_case(n) {
                // Return first string/symbol child.
                if let Some(Ast::Atom(a)) = items.get(1) {
                    if let Some(s) = atom_to_string(a) {
                        return Some(s);
                    }
                }
            }
        }
    }

    for child in items {
        if let Some(v) = find_string_field(child, names) {
            return Some(v);
        }
    }
    None
}

/// Parse `(StartTime <h> <m> <s>)` → seconds from midnight.
fn parse_start_time(ast: &Ast) -> f64 {
    if let Some(vals) = find_numeric_tuple(ast, "StartTime", 3) {
        return vals[0] * 3600.0 + vals[1] * 60.0 + vals[2];
    }
    0.0
}

/// Parse `(Duration <h> <m>)` → seconds.
fn parse_duration(ast: &Ast) -> f64 {
    if let Some(vals) = find_numeric_tuple(ast, "Duration", 2) {
        return vals[0] * 3600.0 + vals[1] * 60.0;
    }
    3600.0
}

fn find_numeric_tuple(ast: &Ast, name: &str, count: usize) -> Option<Vec<f64>> {
    let Ast::List(items) = ast else { return None };

    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        if head.eq_ignore_ascii_case(name) {
            let vals: Vec<f64> = items
                .iter()
                .skip(1)
                .take(count)
                .filter_map(|a| match a {
                    Ast::Atom(Atom::Integer(i)) => Some(*i as f64),
                    Ast::Atom(Atom::Number(n)) => Some(*n),
                    _ => None,
                })
                .collect();
            if vals.len() == count {
                return Some(vals);
            }
        }
    }

    for child in items {
        if let Some(v) = find_numeric_tuple(child, name, count) {
            return Some(v);
        }
    }
    None
}
