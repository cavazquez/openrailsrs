use std::path::Path;

use crate::ast::Ast;
use crate::encoding::read_msts_file_to_string;
use crate::error::FormatError;
use crate::parser::parse_from_first_paren;
use crate::typed::{ConsistFile, EngineFile, RouteFile, WagonFile};

#[derive(Clone, Debug, PartialEq)]
pub enum MstsFile {
    Engine(Box<EngineFile>),
    Wagon(WagonFile),
    Consist(ConsistFile),
    Route(RouteFile),
    Unknown(Ast),
}

pub fn parse_msts_file(path: impl AsRef<Path>) -> Result<MstsFile, FormatError> {
    let path = path.as_ref();
    let source = read_msts_file_to_string(path)?;
    let ast = parse_from_first_paren(&source)?;
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);

    match ext.as_deref() {
        Some("eng") => Ok(MstsFile::Engine(Box::new(EngineFile::from_ast(&ast)?))),
        Some("wag") => Ok(MstsFile::Wagon(WagonFile::from_ast(&ast)?)),
        Some("con") => Ok(MstsFile::Consist(ConsistFile::from_ast(&ast)?)),
        Some("trk") => Ok(MstsFile::Route(RouteFile::from_ast(&ast)?)),
        _ => Ok(MstsFile::Unknown(ast)),
    }
}
