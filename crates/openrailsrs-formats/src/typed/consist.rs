use crate::ast::{Ast, Atom};
use crate::error::FormatError;

use super::atom_to_string;

#[derive(Clone, Debug, PartialEq)]
pub enum ConsistEntry {
    Engine { path: String },
    Wagon { path: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConsistFile {
    pub entries: Vec<ConsistEntry>,
}

impl ConsistFile {
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let mut entries = Vec::new();
        collect_entries(ast, &mut entries)?;
        if entries.is_empty() {
            return Err(FormatError::MissingField {
                key: "Engine|Wagon".to_string(),
                context: "Train".to_string(),
            });
        }
        Ok(Self { entries })
    }
}

fn collect_entries(ast: &Ast, out: &mut Vec<ConsistEntry>) -> Result<(), FormatError> {
    if let Ast::List(items) = ast {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            if head.eq_ignore_ascii_case("Engine") {
                if let Some(path) = parse_engine_entry(items)? {
                    out.push(ConsistEntry::Engine { path });
                }
                return Ok(());
            }
            if head.eq_ignore_ascii_case("Wagon") {
                if let Some(path) = parse_wagon_entry(items)? {
                    out.push(ConsistEntry::Wagon { path });
                }
                return Ok(());
            }
        }

        for item in items {
            collect_entries(item, out)?;
        }
    }
    Ok(())
}

fn parse_engine_entry(items: &[Ast]) -> Result<Option<String>, FormatError> {
    if items.len() >= 2 {
        if let Some(path) = path_from_simple_entry(&items[1]) {
            return Ok(Some(path));
        }
    }
    for item in items.iter().skip(1) {
        if let Ast::List(sub) = item {
            if let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() {
                if tag.eq_ignore_ascii_case("EngineData") {
                    return Ok(Some(trainset_path(sub)));
                }
            }
        }
    }
    Ok(None)
}

fn parse_wagon_entry(items: &[Ast]) -> Result<Option<String>, FormatError> {
    if items.len() >= 2 {
        if let Some(path) = path_from_simple_entry(&items[1]) {
            return Ok(Some(path));
        }
    }
    for item in items.iter().skip(1) {
        if let Ast::List(sub) = item {
            if let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() {
                if tag.eq_ignore_ascii_case("WagonData") {
                    return Ok(Some(trainset_path(sub)));
                }
            }
        }
    }
    Ok(None)
}

fn path_from_simple_entry(value: &Ast) -> Option<String> {
    match value {
        Ast::Atom(atom) => atom_to_string(atom),
        _ => None,
    }
}

/// `(EngineData ( RF_WP_DMBSA RF_Blue_Pullman ))` → `trains/RF_Blue_Pullman/RF_WP_DMBSA.eng`
fn trainset_path(data: &[Ast]) -> String {
    let vehicle = data
        .get(1)
        .and_then(|a| match a {
            Ast::Atom(atom) => atom_to_string(atom),
            _ => None,
        })
        .unwrap_or_default();
    let folder = data
        .get(2)
        .and_then(|a| match a {
            Ast::Atom(atom) => atom_to_string(atom),
            _ => None,
        })
        .unwrap_or_default();
    let ext = if data
        .first()
        .and_then(|a| match a {
            Ast::Atom(Atom::Symbol(s)) => Some(s.as_str()),
            _ => None,
        })
        .is_some_and(|t| t.eq_ignore_ascii_case("WagonData"))
    {
        "wag"
    } else {
        "eng"
    };
    format!("trains/{folder}/{vehicle}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_from_first_paren;

    #[test]
    fn parse_msts_traincfg_engine_data() {
        let text = r#"
( Train
    ( Engine ( UiD 0 ) ( EngineData ( RF_WP_DMBSA RF_Blue_Pullman ) ) )
    ( Wagon ( WagonData ( RF_WP_PSB RF_Blue_Pullman ) ) )
)"#;
        let ast = parse_from_first_paren(text).expect("parse");
        let con = ConsistFile::from_ast(&ast).expect("consist");
        assert!(!con.entries.is_empty());
        assert!(
            con.entries
                .iter()
                .any(|e| matches!(e, ConsistEntry::Engine { .. }))
        );
    }
}
