//! Direct binary `.s` loading: validate with [`BinaryBlockReader`], AST → [`ShapeFile`].
//!
//! Hot path for `JINX0s1b` shapes — avoids routing through generic MSTS text decode.

use crate::error::FormatError;
use crate::msts_simisa::SimisaPayload;
use crate::parser::parse_first_from_first_paren;
use crate::shape_binary::binary_shape_to_ascii;
use crate::shape_binary_reader::BinaryBlockReader;
use crate::typed::ShapeFile;

const SHAPE_TOKEN: i32 = 71;

/// True when the SIMISA payload is a binary shape (`JINX0s*b`), not text or world.
pub fn is_binary_shape_payload(payload: &SimisaPayload) -> bool {
    !payload.is_text && payload.bytes.len() >= 8 && payload.bytes.starts_with(b"JINX0s")
}

/// Convert a binary shape SIMISA payload to a parseable AST (via token dumper).
pub fn binary_shape_to_ast(payload: &SimisaPayload) -> Result<crate::ast::Ast, FormatError> {
    if payload.is_text {
        return Err(FormatError::UnexpectedToken {
            offset: 0,
            message: "binary_shape_to_ast called on text payload".into(),
        });
    }
    validate_shape_root_bbr(payload)?;
    let text = binary_shape_to_ascii(payload)?;
    parse_first_from_first_paren(&text)
}

/// Parse a SIMISA binary shape payload into a [`ShapeFile`].
///
/// Validates the root block with [`BinaryBlockReader`], then builds AST from the
/// token stream. Falls back to the legacy ASCII dumper if AST construction fails.
pub fn shape_from_binary_payload(payload: &SimisaPayload) -> Result<ShapeFile, FormatError> {
    match binary_shape_to_ast(payload) {
        Ok(ast) => ShapeFile::from_ast(&ast),
        Err(direct_err) => {
            let text = binary_shape_to_ascii(payload)?;
            let ast = parse_first_from_first_paren(&text)?;
            ShapeFile::from_ast(&ast).map_err(|_| direct_err)
        }
    }
}

/// Ensure the payload begins with a well-formed shape root block (token 71).
fn validate_shape_root_bbr(payload: &SimisaPayload) -> Result<(), FormatError> {
    if payload.data_offset > payload.bytes.len() {
        return Err(FormatError::UnexpectedToken {
            offset: payload.data_offset,
            message: "binary payload offset is past end of body".into(),
        });
    }
    let data = &payload.bytes[payload.data_offset..];
    if data.is_empty() {
        return Err(FormatError::UnexpectedToken {
            offset: payload.data_offset,
            message: "binary shape body is empty".into(),
        });
    }
    let mut root = BinaryBlockReader::new(data, payload.token_offset, payload.data_offset);
    if !root.has_more_blocks() {
        return Err(FormatError::UnexpectedToken {
            offset: payload.data_offset,
            message: "binary shape has no root block".into(),
        });
    }
    let shape = root.open_sub_block()?;
    if shape.token_id != SHAPE_TOKEN {
        return Err(FormatError::UnexpectedToken {
            offset: payload.data_offset,
            message: format!("expected shape token {SHAPE_TOKEN}, got {}", shape.token_id),
        });
    }
    Ok(())
}
