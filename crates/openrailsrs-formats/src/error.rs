use thiserror::Error;

#[derive(Debug, Error)]
pub enum FormatError {
    #[error("unexpected end of input")]
    UnexpectedEof,

    #[error("unexpected token at byte {offset}: {message}")]
    UnexpectedToken { offset: usize, message: String },

    #[error("unclosed string starting at byte {0}")]
    UnclosedString(usize),

    #[error("invalid number at byte {offset}: {text}")]
    InvalidNumber { offset: usize, text: String },

    #[error("extra input after complete expression at byte {0}")]
    TrailingInput(usize),

    #[error("missing required field `{key}` in {context}")]
    MissingField { key: String, context: String },

    #[error("unexpected atom for `{key}` in {context}: {expected}")]
    UnexpectedAtom {
        key: String,
        context: String,
        expected: String,
    },

    #[error("unknown unit `{0}`")]
    UnknownUnit(String),
}
