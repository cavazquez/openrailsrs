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
            Some(b'-' | b'+')
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
        if matches!(self.peek_byte(), Some(b'-' | b'+')) {
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
        let mut is_float = false;
        if self.peek_byte() == Some(b'.') {
            is_float = true;
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
        }

        let exponent_is_numeric = matches!(self.peek_byte(), Some(b'e' | b'E')) && {
            let mut lookahead = self.pos + 1;
            if matches!(self.input.get(lookahead), Some(b'-' | b'+')) {
                lookahead += 1;
            }
            self.input
                .get(lookahead)
                .is_some_and(|b| b.is_ascii_digit())
        };
        if exponent_is_numeric {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek_byte(), Some(b'-' | b'+')) {
                self.pos += 1;
            }
            while matches!(self.peek_byte(), Some(b) if b.is_ascii_digit()) {
                self.pos += 1;
            }
        }

        let text = std::str::from_utf8(&self.input[start..self.pos]).unwrap();
        if is_float {
            let value: f64 = text.parse().map_err(|_| FormatError::InvalidNumber {
                offset: start,
                text: text.into(),
            })?;
            Ok(Token::Number(value))
        } else {
            let value: i64 = text.parse().map_err(|_| FormatError::InvalidNumber {
                offset: start,
                text: text.into(),
            })?;
            Ok(Token::Integer(value))
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn token(text: &str) -> Token {
        Lexer::new(text)
            .next_token()
            .expect("valid token")
            .expect("one token")
    }

    #[test]
    fn lexes_scientific_notation_as_one_number() {
        assert_eq!(token("-1.29716e-05"), Token::Number(-1.29716e-05));
        assert_eq!(token("2E+3"), Token::Number(2_000.0));
        assert_eq!(token("+4.5e1"), Token::Number(45.0));
    }

    #[test]
    fn keeps_plain_integers_and_decimals_compatible() {
        assert_eq!(token("-12"), Token::Integer(-12));
        assert_eq!(token("3.25"), Token::Number(3.25));
    }

    #[test]
    fn digit_prefixed_symbol_is_not_mistaken_for_an_exponent() {
        let mut lexer = Lexer::new("4994E");
        assert_eq!(lexer.next_token().unwrap(), Some(Token::Integer(4994)));
        assert_eq!(lexer.next_token().unwrap(), Some(Token::Symbol("E".into())));
    }
}
