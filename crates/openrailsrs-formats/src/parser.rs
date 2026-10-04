use crate::ast::{Ast, Atom};
use crate::error::FormatError;
use crate::lexer::{Lexer, Token};

/// Parse native keyword-before-parenthesis files while retaining root names.
/// Sound programs distinguish Wagon/Engine/Inside Sound blocks, unlike legacy
/// vehicle field scans which only need their normalized bodies.
pub fn parse_named_stf(source: &str) -> Result<Ast, FormatError> {
    let source = source.trim_start_matches('\u{feff}').trim_start();
    let source = if source.starts_with("SIMISA") {
        source.split_once('\n').map_or("", |(_, s)| s).trim_start()
    } else {
        source
    };
    if source.starts_with('(') {
        return parse_vehicle_text(source);
    }
    let mut lexer = Lexer::new_stf(source);
    let mut roots = vec![];
    while let Some(token) = lexer.next_token()? {
        lexer.skip_ws_and_comments();
        if lexer.peek_byte() != Some(b'(') {
            continue;
        }
        let Ast::List(mut body) = normalize_stf_body(parse_expr(&mut lexer)?) else {
            unreachable!()
        };
        if let Token::Symbol(name) = token {
            body.insert(0, Ast::Atom(Atom::Symbol(name)));
        }
        roots.push(Ast::List(body));
    }
    match roots.len() {
        0 => Err(FormatError::UnexpectedEof),
        1 => Ok(roots.remove(0)),
        _ => Ok(Ast::List(roots)),
    }
}

/// OR's STFReader accepts EOF after the complete contents of a CVF whose outer
/// closing parenthesis is omitted. Tolerate only that case, not an unfinished
/// control, quoted filename or an extra closing parenthesis.
pub fn parse_cab_view_text(source: &str) -> Result<Ast, FormatError> {
    let parsed = parse_named_stf(source);
    if !matches!(parsed, Err(FormatError::UnexpectedEof)) {
        return parsed;
    }
    let body = source.trim_start_matches('\u{feff}').trim_start();
    let body = if body.starts_with("SIMISA") {
        body.split_once('\n').map_or("", |(_, s)| s).trim_start()
    } else {
        body
    };
    if !body.to_ascii_lowercase().starts_with("tr_cabviewfile") {
        return parsed;
    }
    let mut lexer = Lexer::new_stf(body);
    let mut depth = 0;
    while let Some(token) = lexer.next_token()? {
        match token {
            Token::LParen => depth += 1,
            Token::RParen => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return parsed;
        }
    }
    if depth != 1 {
        return parsed;
    }
    parse_named_stf(&format!("{source}\n)"))
}

/// Read rolling-stock files in either native STF (`Wagon ( ... )`) or the
/// parenthesized fixture notation (`(Wagon ...)`). Native ENG files commonly
/// have separate Wagon and Engine roots; both are required for mass and power.
pub fn parse_vehicle_text(source: &str) -> Result<Ast, FormatError> {
    let source = source.trim_start_matches('\u{feff}').trim_start();
    let source = if source.starts_with("SIMISA") {
        source
            .split_once('\n')
            .map_or("", |(_, rest)| rest)
            .trim_start()
    } else {
        source
    };
    if source.starts_with('(') {
        let mut roots = parse_all_top_level(source)?;
        return match roots.len() {
            0 => Err(FormatError::UnexpectedEof),
            1 => Ok(roots.remove(0)),
            _ => Ok(Ast::List(roots)),
        };
    }
    let mut lexer = Lexer::new_stf(source);
    let mut roots = Vec::new();
    while lexer.next_token()?.is_some() {
        // Parse native blocks through their body with the existing lexer, then
        // normalize keyword/body pairs. Unlike S-expressions their keyword is
        // before the opening parenthesis, including nested fields.
        lexer.skip_ws_and_comments();
        if lexer.peek_byte() == Some(b'(') {
            roots.push(normalize_stf_body(parse_expr(&mut lexer)?));
        }
    }
    if roots.is_empty() {
        Err(FormatError::UnexpectedEof)
    } else {
        Ok(Ast::List(roots))
    }
}

fn normalize_stf_body(ast: Ast) -> Ast {
    let Ast::List(items) = ast else { return ast };
    let mut items = items.into_iter().peekable();
    let mut out = Vec::new();
    while let Some(item) = items.next() {
        if matches!(item, Ast::Atom(Atom::Symbol(_))) && matches!(items.peek(), Some(Ast::List(_)))
        {
            let Some(Ast::List(body)) = items.next().map(normalize_stf_body) else {
                unreachable!()
            };
            let mut block = Vec::with_capacity(body.len() + 1);
            block.push(item);
            block.extend(body);
            out.push(Ast::List(block));
        } else {
            out.push(normalize_stf_body(item));
        }
    }
    Ast::List(out)
}

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

/// Parse every top-level S-expression in `source` (MSTS `tsection.dat`, route overlays, etc.).
pub fn parse_all_top_level(source: &str) -> Result<Vec<Ast>, FormatError> {
    let mut out = Vec::new();
    let mut rest = source;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let Some(start) = rest.find('(') else {
            break;
        };
        let from_paren = &rest[start..];
        let mut lexer = Lexer::new(from_paren);
        let ast = parse_expr(&mut lexer)?;
        let consumed = lexer.position();
        out.push(ast);
        rest = &from_paren[consumed..];
    }
    Ok(out)
}

/// Like [`parse_all_top_level`], but skips malformed blocks instead of failing.
pub fn parse_all_top_level_lenient(source: &str) -> Vec<Ast> {
    match parse_all_top_level(source) {
        Ok(blocks) if !blocks.is_empty() => return blocks,
        Ok(_) | Err(_) => {}
    }
    let mut out = Vec::new();
    let mut rest = source;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let Some(start) = rest.find('(') else {
            break;
        };
        let from_paren = &rest[start..];
        let mut lexer = Lexer::new(from_paren);
        match parse_expr(&mut lexer) {
            Ok(ast) => {
                let consumed = lexer.position().max(1);
                out.push(ast);
                rest = &from_paren[consumed..];
            }
            Err(_) => {
                rest = &from_paren[1..];
            }
        }
    }
    out
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
    fn named_native_roots_preserve_exterior_cab_and_passenger_scopes() {
        let ast = parse_named_stf("Wagon ( coach Sound ( exterior.sms ) Inside ( Sound ( passenger.sms ) ) ) Engine ( coach Sound ( cab.sms ) )").unwrap();
        let Ast::List(roots) = ast else {
            panic!("root list")
        };
        assert!(
            matches!(&roots[0], Ast::List(items) if items[0] == Ast::Atom(Atom::Symbol("Wagon".into())))
        );
        assert!(
            matches!(&roots[1], Ast::List(items) if items[0] == Ast::Atom(Atom::Symbol("Engine".into())))
        );
    }

    #[test]
    fn cab_eof_tolerates_only_the_missing_outer_close() {
        let body =
            "Tr_CabViewFile ( CabViewType ( 1 ) CabViewFile ( front.ace ) CabViewControls ( 0 )";
        assert_eq!(
            parse_cab_view_text(body).unwrap(),
            parse_cab_view_text(&format!("{body})")).unwrap()
        );
        assert!(parse_cab_view_text("Tr_CabViewFile ( CabViewControls ( 1 Dial (").is_err());
        assert!(parse_cab_view_text("Tr_CabViewFile ( CabViewFile ( \"unfinished").is_err());
    }

    #[test]
    fn native_vehicle_roots_and_consist_keyword_blocks() {
        let numeric = parse_vehicle_text(
            "Train ( TrainCfg ( test Engine ( EngineData ( 7030 \"7030Cranbrook Castle\" ) ) ) )",
        )
        .unwrap();
        assert_eq!(
            crate::ConsistFile::from_ast(&numeric).unwrap().entries[0].path(),
            "trains/7030Cranbrook Castle/7030.eng"
        );
        let comments = parse_vehicle_text("Wagon ( test Comment ( Native semicolons; stay inside this block ) Mass ( 40t ) Size ( 3m 4m 20m ) )").unwrap();
        assert_eq!(
            crate::WagonFile::from_ast(&comments).unwrap().mass_kg,
            40_000.0
        );
        let eng = parse_vehicle_text("SIMISA@@@@@@@@@@JINX0D0t______\nWagon ( power Mass ( 40t ) Size ( 3m 4m 20m ) ) Engine ( power MaxPower ( 500kW ) CabView ( front.cvf ) )").unwrap();
        let engine = crate::EngineFile::from_ast(&eng).unwrap();
        assert_eq!(engine.mass_kg, 40_000.0);
        assert_eq!(engine.max_power_w, 500_000.0);
        assert_eq!(engine.length_m, 20.0);
        assert_eq!(engine.cab.cab_view_file.as_deref(), Some("front.cvf"));
        let con = parse_vehicle_text("Train ( TrainCfg ( express Serial ( 1 ) Engine ( UiD ( 12 ) EngineData ( Power DMU ) Flip ( ) ) Wagon ( WagonData ( Coach DMU ) ) ) )").unwrap();
        let consist = crate::ConsistFile::from_ast(&con).unwrap();
        assert_eq!(consist.entries.len(), 2);
        assert_eq!(consist.entries[0].uid(), Some(12));
        assert!(consist.entries[0].flipped());
        assert_eq!(consist.entries[1].path(), "trains/DMU/Coach.wag");
    }

    #[test]
    fn parse_all_top_level_reads_multiple_blocks() {
        let src = "(A 1) junk (B 2) (C 3)";
        let asts = parse_all_top_level(src).expect("parse");
        assert_eq!(asts.len(), 3);
    }

    #[test]
    fn parse_first_from_first_paren_ignores_trailing_bytes() {
        let src = r#"(Shape (a 1) (b 2)) trailing junk"#;
        let ast = parse_first_from_first_paren(src).expect("parse");
        assert!(matches!(ast, Ast::List(_)));
        assert!(parse_from_first_paren(src).is_err());
    }
}
