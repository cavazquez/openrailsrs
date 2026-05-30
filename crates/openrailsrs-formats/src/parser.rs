use crate::ast::{Ast, Atom};
use crate::error::FormatError;
use crate::lexer::{Lexer, Token};

/// Parse the first complete S-expression, ignoring any trailing text.
pub fn parse_first(source: &str) -> Result<Ast, FormatError> {
    let mut lexer = Lexer::new(source);
    parse_expr(&mut lexer)
}

/// Skip preamble, find the first `(`, then parse one expression (ignore trailing bytes).
pub fn parse_first_from_first_paren(source: &str) -> Result<Ast, FormatError> {
    let trimmed = source.trim_start();
    let from_paren = trimmed
        .find('(')
        .map(|i| &trimmed[i..])
        .ok_or(FormatError::UnexpectedEof)?;
    parse_first(from_paren)
}

/// Skip preamble, find the first `(`, then parse one expression until balanced closing.
pub fn parse_from_first_paren(source: &str) -> Result<Ast, FormatError> {
    let trimmed = source.trim_start();
    let from_paren = trimmed
        .find('(')
        .map(|i| &trimmed[i..])
        .ok_or(FormatError::UnexpectedEof)?;
    parse(from_paren)
}

/// Parse a single top-level S-expression; the entire `source` must be one expression (after trim).
pub fn parse(source: &str) -> Result<Ast, FormatError> {
    let mut lexer = Lexer::new(source);
    let ast = parse_expr(&mut lexer)?;
    lexer.skip_ws_and_comments();
    if lexer.position() < source.len() {
        return Err(FormatError::TrailingInput(lexer.position()));
    }
    Ok(ast)
}

fn parse_expr(lexer: &mut Lexer<'_>) -> Result<Ast, FormatError> {
    match lexer.next_token()? {
        None => Err(FormatError::UnexpectedEof),
        Some(Token::LParen) => {
            let mut items = Vec::new();
            loop {
                lexer.skip_ws_and_comments();
                match lexer.peek_byte() {
                    None => return Err(FormatError::UnexpectedEof),
                    Some(b')') => {
                        lexer.skip_ws_and_comments();
                        // consume ')'
                        let _ = lexer.next_token()?;
                        break;
                    }
                    _ => items.push(parse_expr(lexer)?),
                }
            }
            Ok(Ast::List(items))
        }
        Some(Token::RParen) => Err(FormatError::UnexpectedToken {
            offset: lexer.position().saturating_sub(1),
            message: "unexpected ')'".into(),
        }),
        Some(Token::Symbol(s)) => Ok(Ast::Atom(Atom::Symbol(s))),
        Some(Token::String(s)) => Ok(Ast::Atom(Atom::String(s))),
        Some(Token::Number(n)) => Ok(Ast::Atom(Atom::Number(n))),
        Some(Token::Integer(i)) => Ok(Ast::Atom(Atom::Integer(i))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_first_from_first_paren_ignores_trailing_bytes() {
        let src = r#"(Shape (a 1) (b 2)) trailing junk"#;
        let ast = parse_first_from_first_paren(src).expect("parse");
        assert!(matches!(ast, Ast::List(_)));
        assert!(parse_from_first_paren(src).is_err());
    }
}
