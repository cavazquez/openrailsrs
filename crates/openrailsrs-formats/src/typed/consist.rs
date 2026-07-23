use crate::ast::{Ast, Atom};
use crate::error::FormatError;

use super::atom_to_string;

/// One Engine/Wagon slot from a `.con` / `TrainCfg` list.
#[derive(Clone, Debug, PartialEq)]
pub enum ConsistEntry {
    Engine {
        path: String,
        /// MSTS/OR `UiD` when present.
        uid: Option<u32>,
        /// True when the entry has a `Flip` field (any form).
        flipped: bool,
    },
    Wagon {
        path: String,
        uid: Option<u32>,
        flipped: bool,
    },
}

impl ConsistEntry {
    pub fn path(&self) -> &str {
        match self {
            Self::Engine { path, .. } | Self::Wagon { path, .. } => path,
        }
    }

    pub fn uid(&self) -> Option<u32> {
        match self {
            Self::Engine { uid, .. } | Self::Wagon { uid, .. } => *uid,
        }
    }

    pub fn flipped(&self) -> bool {
        match self {
            Self::Engine { flipped, .. } | Self::Wagon { flipped, .. } => *flipped,
        }
    }
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
                if let Some(entry) = parse_engine_entry(items)? {
                    out.push(entry);
                }
                return Ok(());
            }
            if head.eq_ignore_ascii_case("Wagon") {
                if let Some(entry) = parse_wagon_entry(items)? {
                    out.push(entry);
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

fn parse_engine_entry(items: &[Ast]) -> Result<Option<ConsistEntry>, FormatError> {
    let meta = entry_meta(items);
    if items.len() >= 2 {
        if let Some(path) = path_from_simple_entry(&items[1]) {
            return Ok(Some(ConsistEntry::Engine {
                path,
                uid: meta.uid,
                flipped: meta.flipped,
            }));
        }
    }
    for item in items.iter().skip(1) {
        if let Ast::List(sub) = item {
            if let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() {
                if tag.eq_ignore_ascii_case("EngineData") {
                    return Ok(Some(ConsistEntry::Engine {
                        path: trainset_path(sub),
                        uid: meta.uid,
                        flipped: meta.flipped,
                    }));
                }
            }
        }
    }
    Ok(None)
}

fn parse_wagon_entry(items: &[Ast]) -> Result<Option<ConsistEntry>, FormatError> {
    let meta = entry_meta(items);
    if items.len() >= 2 {
        if let Some(path) = path_from_simple_entry(&items[1]) {
            return Ok(Some(ConsistEntry::Wagon {
                path,
                uid: meta.uid,
                flipped: meta.flipped,
            }));
        }
    }
    for item in items.iter().skip(1) {
        if let Ast::List(sub) = item {
            if let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() {
                if tag.eq_ignore_ascii_case("WagonData") {
                    return Ok(Some(ConsistEntry::Wagon {
                        path: trainset_path(sub),
                        uid: meta.uid,
                        flipped: meta.flipped,
                    }));
                }
            }
        }
    }
    Ok(None)
}

#[derive(Clone, Copy, Default)]
struct EntryMeta {
    uid: Option<u32>,
    flipped: bool,
}

/// Collect `UiD` / `Flip` from an Engine/Wagon block (any nesting depth of direct children).
fn entry_meta(items: &[Ast]) -> EntryMeta {
    let mut meta = EntryMeta::default();
    for item in items.iter().skip(1) {
        scan_entry_meta(item, &mut meta);
    }
    meta
}

fn scan_entry_meta(ast: &Ast, meta: &mut EntryMeta) {
    let Ast::List(sub) = ast else {
        return;
    };
    let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
        return;
    };
    if tag.eq_ignore_ascii_case("UiD") {
        if let Some(Ast::Atom(at)) = sub.get(1) {
            if let Some(n) = super::atom_to_number(at) {
                meta.uid = Some(n as u32);
            }
        }
        return;
    }
    if tag.eq_ignore_ascii_case("Flip") {
        // MSTS/OR: `Flip ( )`, `Flip ( 1 )`, nested `Flip ( 0 )`, or bare presence.
        meta.flipped = match sub.get(1) {
            None => true,
            Some(Ast::Atom(at)) => flip_flag_from_atom(at),
            Some(Ast::List(inner)) => match inner.first() {
                None => true, // `Flip ( )`
                Some(Ast::Atom(at)) => flip_flag_from_atom(at),
                Some(_) => true,
            },
        };
        return;
    }
}

fn flip_flag_from_atom(at: &Atom) -> bool {
    if let Some(n) = super::atom_to_number(at) {
        return n != 0.0;
    }
    match atom_to_string(at).as_deref().map(str::trim) {
        Some("0") | Some("false") | Some("no") => false,
        Some(_) => true,
        None => true,
    }
}

fn path_from_simple_entry(value: &Ast) -> Option<String> {
    match value {
        Ast::Atom(atom) => atom_to_string(atom),
        _ => None,
    }
}

/// `(EngineData RF_WP_DMBSA RF_Blue_Pullman)` or nested
/// `(EngineData ( RF_WP_DMBSA RF_Blue_Pullman ))` → `trains/RF_Blue_Pullman/RF_WP_DMBSA.eng`
fn trainset_path(data: &[Ast]) -> String {
    let (vehicle, folder) = match data.get(1) {
        Some(Ast::List(inner)) => (
            inner.first().and_then(ast_atom_string).unwrap_or_default(),
            inner.get(1).and_then(ast_atom_string).unwrap_or_default(),
        ),
        _ => (
            data.get(1).and_then(ast_atom_string).unwrap_or_default(),
            data.get(2).and_then(ast_atom_string).unwrap_or_default(),
        ),
    };
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

fn ast_atom_string(ast: &Ast) -> Option<String> {
    match ast {
        Ast::Atom(atom) => atom_to_string(atom),
        _ => None,
    }
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
        assert_eq!(con.entries.len(), 2);
        match &con.entries[0] {
            ConsistEntry::Engine {
                path,
                uid,
                flipped,
            } => {
                assert!(path.contains("RF_WP_DMBSA"));
                assert_eq!(*uid, Some(0));
                assert!(!*flipped);
            }
            other => panic!("expected engine, got {other:?}"),
        }
        match &con.entries[1] {
            ConsistEntry::Wagon {
                path,
                uid,
                flipped,
            } => {
                assert!(path.contains("RF_WP_PSB"));
                assert_eq!(*uid, None);
                assert!(!*flipped);
            }
            other => panic!("expected wagon, got {other:?}"),
        }
    }

    #[test]
    fn parse_flip_empty_and_uid() {
        let text = r#"
( Train
    ( Engine
        ( UiD 12 )
        ( Flip ( ) )
        ( EngineData ( RF_WP_DMBSA RF_Blue_Pullman ) )
    )
    ( Wagon
        ( UiD 13 )
        ( Flip ( 1 ) )
        ( WagonData ( RF_WP_PSB RF_Blue_Pullman ) )
    )
    ( Wagon
        ( UiD 14 )
        ( Flip ( 0 ) )
        ( WagonData ( RF_WP_TFK RF_Blue_Pullman ) )
    )
)"#;
        let ast = parse_from_first_paren(text).expect("parse");
        let con = ConsistFile::from_ast(&ast).expect("consist");
        assert_eq!(con.entries.len(), 3);
        assert_eq!(con.entries[0].uid(), Some(12));
        assert!(con.entries[0].flipped());
        assert_eq!(con.entries[1].uid(), Some(13));
        assert!(con.entries[1].flipped());
        assert_eq!(con.entries[2].uid(), Some(14));
        assert!(!con.entries[2].flipped());
    }
}
