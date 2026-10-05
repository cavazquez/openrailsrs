//! Bounded STF Include expansion within a vehicle's content installation.
use crate::lexer::{Lexer, Token};
use crate::{
    Ast, FormatError, parse_vehicle_text, read_msts_file_to_string, resolve_path_case_insensitive,
};
use std::path::{Path, PathBuf};
const MAX_DEPTH: usize = 32;
const MAX_BYTES: usize = 32 * 1024 * 1024;
fn error(path: &Path, message: impl std::fmt::Display) -> FormatError {
    FormatError::UnexpectedToken {
        offset: 0,
        message: format!("{}: {message}", path.display()),
    }
}
/// Includes resolve relative to their containing file, with Windows case and
/// separators. Common.Include siblings inside TRAINS are allowed; paths out
/// of that installation, cycles and unbounded expansion are rejected.
pub fn read_msts_text_with_includes(path: impl AsRef<Path>) -> Result<String, FormatError> {
    let path = path.as_ref();
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| error(path, e))?
            .join(path)
    };
    let path = resolve_path_case_insensitive(&absolute)
        .ok_or_else(|| error(path, "Archivo ausente"))?
        .canonicalize()
        .map_err(|e| error(path, e))?;
    let named = |p: &&Path, name: &str| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name));
    let root = path
        .ancestors()
        .find(|p| named(p, "TRAINS"))
        .or_else(|| path.ancestors().find(|p| named(p, "TRAINSET")))
        .or_else(|| path.parent())
        .ok_or_else(|| error(&path, "Sin directorio de contenido"))?;
    expand(&path, root, &mut vec![], &mut 0)
}
pub fn read_vehicle_ast(path: impl AsRef<Path>) -> Result<Ast, FormatError> {
    parse_vehicle_text(&read_msts_text_with_includes(path)?)
}
fn expand(
    path: &Path,
    root: &Path,
    stack: &mut Vec<PathBuf>,
    bytes: &mut usize,
) -> Result<String, FormatError> {
    if stack.len() >= MAX_DEPTH || stack.iter().any(|p| p == path) {
        return Err(error(path, "Include circular o demasiado profundo"));
    }
    let length = std::fs::metadata(path).map_err(|e| error(path, e))?.len();
    if length > MAX_BYTES as u64 || *bytes > MAX_BYTES - length as usize {
        return Err(error(path, "Include supera el límite de 32 MiB"));
    }
    let source = read_msts_file_to_string(path)?;
    *bytes += source.len();
    if *bytes > MAX_BYTES {
        return Err(error(path, "Include supera el límite de 32 MiB"));
    }
    stack.push(path.to_owned());
    let body = source.trim_start_matches('\u{feff}').trim_start();
    let body = if body.starts_with("SIMISA") {
        body.split_once('\n').map_or("", |(_, s)| s).trim_start()
    } else {
        body
    };
    let mut lexer = if body.starts_with('(') {
        Lexer::new(&source)
    } else {
        Lexer::new_stf(&source)
    };
    let mut output = String::new();
    let mut copied = 0;
    let mut preceding_paren = None;
    loop {
        lexer.skip_ws_and_comments();
        let start = lexer.position();
        let Some(token) = lexer.next_token()? else {
            break;
        };
        match token {
            Token::Symbol(name)
                if name.eq_ignore_ascii_case("comment") || name.eq_ignore_ascii_case("skip") =>
            {
                if lexer.next_token()? == Some(Token::LParen) || preceding_paren.is_some() {
                    let mut depth = 1;
                    while depth > 0 {
                        match lexer.next_token()? {
                            Some(Token::LParen) => depth += 1,
                            Some(Token::RParen) => depth -= 1,
                            None => return Err(error(path, "Comentario sin cerrar")),
                            _ => (),
                        }
                    }
                }
                preceding_paren = None;
            }
            Token::Symbol(name) if name.eq_ignore_ascii_case("include") => {
                let next = lexer.next_token()?;
                let (replace_start, argument) = if next == Some(Token::LParen) {
                    (start, lexer.next_token()?)
                } else if let Some(paren) = preceding_paren {
                    (paren, next)
                } else {
                    return Err(error(path, "Include requiere un nombre entre paréntesis"));
                };
                let relative = match argument {
                    Some(Token::Symbol(name) | Token::String(name)) => name.replace('\\', "/"),
                    _ => return Err(error(path, "Include sin nombre de archivo")),
                };
                if lexer.next_token()? != Some(Token::RParen) {
                    return Err(error(path, "Include debe contener un solo archivo"));
                }
                let relative_path = Path::new(&relative);
                if relative_path.is_absolute() || relative.contains(':') {
                    return Err(error(path, "Include absoluto no permitido"));
                }
                let requested = path.parent().unwrap().join(relative_path);
                let included = resolve_path_case_insensitive(&requested)
                    .ok_or_else(|| error(path, format!("Falta Include {relative}")))?
                    .canonicalize()
                    .map_err(|e| error(path, e))?;
                if !included.starts_with(root) {
                    return Err(error(path, "Include fuera de la instalación"));
                }
                let fragment = expand(&included, root, stack, bytes)?;
                let fragment = fragment.trim_start_matches('\u{feff}');
                let fragment = if fragment.starts_with("SIMISA") {
                    fragment.split_once('\n').map_or("", |(_, s)| s)
                } else {
                    fragment
                };
                output.push_str(&source[copied..replace_start]);
                output.push('\n');
                output.push_str(fragment);
                output.push('\n');
                copied = lexer.position();
                preceding_paren = None;
            }
            Token::LParen => preceding_paren = Some(start),
            _ => preceding_paren = None,
        }
    }
    output.push_str(&source[copied..]);
    stack.pop();
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("TRAINS/TRAINSET");
        std::fs::create_dir_all(root.join("Stock")).unwrap();
        std::fs::create_dir_all(root.join("Common.Include")).unwrap();
        (temp, root)
    }
    #[test]
    fn native_fields_expand_with_units_case_nested_and_unicode() {
        let (_temp, root) = fixture();
        std::fs::write(
            root.join("Common.Include/mass.inc"),
            "Mass ( 42t ) Include ( dimensions.inc )",
        )
        .unwrap();
        std::fs::write(
            root.join("Common.Include/dimensions.inc"),
            "Size ( 3m 4m 20m )",
        )
        .unwrap();
        std::fs::write(
            root.join("Common.Include/engine.inc"),
            "MaxPower ( 500kW ) CabView ( frente.cvf )",
        )
        .unwrap();
        let eng = root.join("Stock/engine.eng");
        let source = "SIMISA@@@@@@@@@@JINX0D0t______\nWagon ( motor Name ( \"Locomotora eléctrica\" ) Include ( ..\\common.include\\MASS.INC ) ) Engine ( motor Include ( ../Common.Include/engine.inc ) )";
        let utf16: Vec<u8> = [0xff, 0xfe]
            .into_iter()
            .chain(source.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        std::fs::write(&eng, utf16).unwrap();
        let engine = crate::EngineFile::from_ast(&read_vehicle_ast(&eng).unwrap()).unwrap();
        assert_eq!(engine.mass_kg, 42_000.);
        assert_eq!(engine.max_power_w, 500_000.);
        assert_eq!(engine.length_m, 20.);
    }
    #[test]
    fn fixture_include_and_comment_text_do_not_open_comment_files() {
        let (_temp, root) = fixture();
        std::fs::write(root.join("Stock/mass.inc"), "(Mass 40t) (Size 3m 4m 20m)").unwrap();
        let eng = root.join("Stock/car.wag");
        std::fs::write(
            &eng,
            "(Wagon car (Comment \"Include\" (Include missing.inc)) (Include \"mass.inc\"))",
        )
        .unwrap();
        assert_eq!(
            crate::WagonFile::from_ast(&read_vehicle_ast(&eng).unwrap())
                .unwrap()
                .mass_kg,
            40_000.
        );
    }
    #[test]
    fn missing_and_circular_includes_have_actionable_errors() {
        let (_temp, root) = fixture();
        let eng = root.join("Stock/car.wag");
        std::fs::write(&eng, "Wagon ( car Include ( missing.inc ) )").unwrap();
        assert!(
            read_vehicle_ast(&eng)
                .unwrap_err()
                .to_string()
                .contains("Falta Include")
        );
        std::fs::write(&eng, "Wagon ( car Include ( car.wag ) )").unwrap();
        assert!(
            read_vehicle_ast(&eng)
                .unwrap_err()
                .to_string()
                .contains("circular")
        );
    }
    #[test]
    fn parent_escape_is_rejected_before_reading_external_files() {
        let (temp, root) = fixture();
        let eng = root.join("Stock/car.wag");
        std::fs::write(temp.path().join("external.inc"), "Mass ( 40t )").unwrap();
        std::fs::write(&eng, "Wagon ( car Include ( ../../../external.inc ) )").unwrap();
        assert!(
            read_vehicle_ast(&eng)
                .unwrap_err()
                .to_string()
                .contains("fuera de la instalación")
        );
    }
    #[test]
    fn fixture_line_comments_do_not_expand_includes() {
        let (_temp, root) = fixture();
        let wag = root.join("Stock/car.wag");
        std::fs::write(
            &wag,
            "(Wagon car ; Include ( missing.inc )\n(Mass 40t) (Size 3m 4m 20m))",
        )
        .unwrap();
        assert_eq!(
            crate::WagonFile::from_ast(&read_vehicle_ast(&wag).unwrap())
                .unwrap()
                .mass_kg,
            40_000.
        );
    }
    #[test]
    fn depth_and_source_size_limits_are_enforced() {
        let (_temp, root) = fixture();
        for i in 0..MAX_DEPTH {
            std::fs::write(
                root.join(format!("Stock/{i}.inc")),
                format!("Include ( {}.inc )", i + 1),
            )
            .unwrap();
        }
        std::fs::write(root.join(format!("Stock/{MAX_DEPTH}.inc")), "Mass ( 40t )").unwrap();
        assert!(
            read_msts_text_with_includes(root.join("Stock/0.inc"))
                .unwrap_err()
                .to_string()
                .contains("profundo")
        );
        let oversized = root.join("Stock/large.inc");
        std::fs::File::create(&oversized)
            .unwrap()
            .set_len(MAX_BYTES as u64 + 1)
            .unwrap();
        assert!(
            read_msts_text_with_includes(oversized)
                .unwrap_err()
                .to_string()
                .contains("32 MiB")
        );
    }
    #[cfg(unix)]
    #[test]
    fn symlink_include_cannot_read_outside_the_installation() {
        let (temp, root) = fixture();
        let external = temp.path().join("external.inc");
        std::fs::write(&external, "Mass ( 40t )").unwrap();
        std::os::unix::fs::symlink(&external, root.join("Stock/linked.inc")).unwrap();
        let wag = root.join("Stock/car.wag");
        std::fs::write(&wag, "Wagon ( car Include ( linked.inc ) )").unwrap();
        assert!(
            read_vehicle_ast(wag)
                .unwrap_err()
                .to_string()
                .contains("fuera de la instalación")
        );
    }
}
