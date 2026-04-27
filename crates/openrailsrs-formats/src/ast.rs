use std::fmt;

/// Atomic value in an MSTS-style S-expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Atom {
    Symbol(String),
    String(String),
    Number(f64),
    Integer(i64),
}

/// Generic S-expression tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Ast {
    Atom(Atom),
    List(Vec<Ast>),
}

impl fmt::Display for Ast {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ast::Atom(a) => write!(f, "{a}"),
            Ast::List(items) => {
                write!(f, "(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, ")")
            }
        }
    }
}

impl fmt::Display for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Atom::Symbol(s) => write!(f, "{s}"),
            Atom::String(s) => {
                let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
                write!(f, "\"{escaped}\"")
            }
            Atom::Number(n) => write!(f, "{n}"),
            Atom::Integer(i) => write!(f, "{i}"),
        }
    }
}
