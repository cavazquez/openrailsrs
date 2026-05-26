use crate::FormatError;

#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    LParen,
    RParen,
    Symbol(String),
    String(String),
    Number(f64),
    Integer(i64),
}

/// Byte-oriented lexer for MSTS-style S-expressions.
pub struct Lexer<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
        }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub(crate) fn peek_byte(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek_byte()?;
        self.pos += 1;
        Some(b)
    }

    pub(crate) fn skip_ws_and_comments(&mut self) {
        loop {
            while matches!(self.peek_byte(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
                self.pos += 1;
            }
            // Line comments starting with ';' (common in route files)
            if self.peek_byte() == Some(b';') {
                while let Some(b) = self.peek_byte() {
                    self.pos += 1;
                    if b == b'\n' {
                        break;
                    }
                }
                continue;
            }
            break;
        }
    }

    /// Returns the next token or `None` at end of input.
    pub fn next_token(&mut self) -> Result<Option<Token>, FormatError> {
        self.skip_ws_and_comments();
        match self.peek_byte() {
            None => Ok(None),
            Some(b'(') => {
                self.pos += 1;
                Ok(Some(Token::LParen))
            }
            Some(b')') => {
                self.pos += 1;
                Ok(Some(Token::RParen))
            }
            Some(b'"') => Ok(Some(self.read_string()?)),
            Some(b'-')
                if self
                    .input
                    .get(self.pos + 1)
                    .is_some_and(|b| b.is_ascii_digit()) =>
            {
                Ok(Some(self.read_number()?))
            }
            Some(b) if b.is_ascii_digit() => Ok(Some(self.read_number()?)),
            Some(_) => Ok(Some(self.read_symbol()?)),
        }
    }

    fn read_string(&mut self) -> Result<Token, FormatError> {
        let start = self.pos;
        debug_assert_eq!(self.peek_byte(), Some(b'"'));
        self.pos += 1;
        let mut out = String::new();
        loop {
            match self.peek_byte() {
                None => return Err(FormatError::UnclosedString(start)),
                Some(b'"') => {
                    self.pos += 1;
                    break;
                }
                Some(b'\\') => {
                    self.pos += 1;
                    match self.bump() {
                        Some(b'n') => out.push('\n'),
                        Some(b'r') => out.push('\r'),
                        Some(b't') => out.push('\t'),
                        Some(b'"') => out.push('"'),
                        Some(b'\\') => out.push('\\'),
                        Some(c) => out.push(c as char),
                        None => return Err(FormatError::UnclosedString(start)),
                    }
                }
                Some(b) => {
                    self.pos += 1;
                    out.push(b as char);
                }
            }
        }
        Ok(Token::String(out))
    }

    fn read_number(&mut self) -> Result<Token, FormatError> {
        let start = self.pos;
        let mut neg = false;
        if self.peek_byte() == Some(b'-') {
            neg = true;
            self.pos += 1;
        }
        let int_start = self.pos;
        while matches!(self.peek_byte(), Some(b) if b.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == int_start {
            return Err(FormatError::InvalidNumber {
                offset: start,
                text: "expected digit".into(),
            });
        }
        let int_slice = std::str::from_utf8(&self.input[int_start..self.pos]).unwrap();
        let int_val: i64 = int_slice.parse().map_err(|_| FormatError::InvalidNumber {
            offset: start,
            text: int_slice.into(),
        })?;

        if self.peek_byte() == Some(b'.') {
            self.pos += 1;
            let frac_start = self.pos;
            while matches!(self.peek_byte(), Some(b) if b.is_ascii_digit()) {
                self.pos += 1;
            }
            if frac_start == self.pos {
                return Err(FormatError::InvalidNumber {
                    offset: start,
                    text: "expected fractional digits".into(),
                });
            }
            let s = std::str::from_utf8(&self.input[start..self.pos]).unwrap();
            let n: f64 = s.parse().map_err(|_| FormatError::InvalidNumber {
                offset: start,
                text: s.into(),
            })?;
            return Ok(Token::Number(if neg { -n.abs() } else { n }));
        }

        Ok(Token::Integer(if neg { -int_val } else { int_val }))
    }

    fn read_symbol(&mut self) -> Result<Token, FormatError> {
        let start = self.pos;
        while let Some(b) = self.peek_byte() {
            if matches!(b, b'(' | b')' | b'"' | b' ' | b'\t' | b'\r' | b'\n' | b';') {
                break;
            }
            self.pos += 1;
        }
        if self.pos == start {
            return Err(FormatError::UnexpectedToken {
                offset: start,
                message: format!("char {:?}", self.peek_byte().map(|c| c as char)),
            });
        }
        let s = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|_| FormatError::UnexpectedToken {
                offset: start,
                message: "invalid utf-8".into(),
            })?
            .to_string();
        Ok(Token::Symbol(s))
    }
}
