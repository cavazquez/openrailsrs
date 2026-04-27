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
}
