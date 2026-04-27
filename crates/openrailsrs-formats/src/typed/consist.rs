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
            if head.eq_ignore_ascii_case("Engine") || head.eq_ignore_ascii_case("Wagon") {
                let value = items.get(1).ok_or_else(|| FormatError::MissingField {
                    key: "path".to_string(),
                    context: head.clone(),
                })?;
                let path = match value {
                    Ast::Atom(atom) => atom_to_string(atom).ok_or_else(|| FormatError::UnexpectedAtom {
                        key: "path".to_string(),
                        context: head.clone(),
                        expected: "string or symbol atom".to_string(),
                    })?,
                    Ast::List(_) => {
                        return Err(FormatError::UnexpectedAtom {
                            key: "path".to_string(),
                            context: head.clone(),
                            expected: "string or symbol atom".to_string(),
                        });
                    }
                };

                if head.eq_ignore_ascii_case("Engine") {
                    out.push(ConsistEntry::Engine { path });
                } else {
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
