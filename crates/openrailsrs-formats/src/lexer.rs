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
    semicolon_comments: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
            semicolon_comments: true,
        }
    }

    /// Native STF comments are named blocks. Semicolons inside those blocks
    /// must not swallow the closing parenthesis as fixture line comments do.
    pub fn new_stf(input: &'a str) -> Self {
        Self {
            semicolon_comments: false,
            ..Self::new(input)
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
            if self.semicolon_comments && self.peek_byte() == Some(b';') {
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
            // Open Rails `STFReader.ReadItem` is whitespace-delimited: `50x150_building.s`
            // is one token. Only emit Number/Integer when the *whole* item is numeric
            // (incl. scientific); otherwise keep digit-leading filenames as Symbol.
            Some(b'-' | b'+')
                if self
                    .input
                    .get(self.pos + 1)
                    .is_some_and(|b| b.is_ascii_digit() || *b == b'.') =>
            {
                Ok(Some(self.read_number_or_digit_symbol()?))
            }
            Some(b) if b.is_ascii_digit() => Ok(Some(self.read_number_or_digit_symbol()?)),
            Some(b'.') if self.input.get(self.pos + 1).is_some_and(u8::is_ascii_digit) => {
                Ok(Some(self.read_number_or_digit_symbol()?))
            }
            Some(_) => Ok(Some(self.read_symbol()?)),
        }
    }

    fn item_end_from(&self, start: usize) -> usize {
        let mut end = start;
        while let Some(b) = self.input.get(end).copied() {
            if matches!(b, b'(' | b')' | b'"' | b' ' | b'\t' | b'\r' | b'\n')
                || (self.semicolon_comments && b == b';')
            {
                break;
            }
            end += 1;
        }
        end
    }

    /// True when `text` is entirely an integer/float (optional `e`/`E` exponent with digits).
    fn is_numeric_item(text: &str) -> bool {
        text.bytes().any(|b| b.is_ascii_digit()) && text.parse::<f64>().is_ok_and(f64::is_finite)
    }

    fn read_number_or_digit_symbol(&mut self) -> Result<Token, FormatError> {
        let start = self.pos;
        let end = self.item_end_from(start);
        let text = std::str::from_utf8(&self.input[start..end]).map_err(|_| {
            FormatError::UnexpectedToken {
                offset: start,
                message: "invalid utf-8".into(),
            }
        })?;
        if Self::is_numeric_item(text) {
            self.read_number()
        } else {
            self.read_symbol()
        }
    }

    fn read_string(&mut self) -> Result<Token, FormatError> {
        let start = self.pos;
        debug_assert_eq!(self.peek_byte(), Some(b'"'));
        self.pos += 1;
        let mut out = Vec::new();
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
                        Some(b'n') => out.push(b'\n'),
                        Some(b'r') => out.push(b'\r'),
                        Some(b't') => out.push(b'\t'),
                        Some(b'"') => out.push(b'"'),
                        Some(b'\\') => out.push(b'\\'),
                        Some(c) => out.push(c),
                        None => return Err(FormatError::UnclosedString(start)),
                    }
                }
                Some(b) => {
                    self.pos += 1;
                    out.push(b);
                }
            }
        }
        let out = String::from_utf8(out).map_err(|_| FormatError::UnexpectedToken {
            offset: start,
            message: "invalid utf-8 string".into(),
        })?;
        Ok(Token::String(out))
    }

    fn read_number(&mut self) -> Result<Token, FormatError> {
        let start = self.pos;
        self.pos = self.item_end_from(start);
        let text = std::str::from_utf8(&self.input[start..self.pos]).unwrap();
        if let Ok(integer) = text.parse::<i64>() {
            return Ok(Token::Integer(integer));
        }
        text.parse::<f64>()
            .map(Token::Number)
            .map_err(|_| FormatError::InvalidNumber {
                offset: start,
                text: text.into(),
            })
    }

    fn read_symbol(&mut self) -> Result<Token, FormatError> {
        let start = self.pos;
        while let Some(b) = self.peek_byte() {
            if matches!(b, b'(' | b')' | b'"' | b' ' | b'\t' | b'\r' | b'\n')
                || (self.semicolon_comments && b == b';')
            {
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
        assert_eq!(token("-.2"), Token::Number(-0.2));
        assert_eq!(token(".5"), Token::Number(0.5));
        assert_eq!(token("2."), Token::Number(2.0));
        assert_eq!(
            token("\"Señal · estación\""),
            Token::String("Señal · estación".into())
        );
    }

    #[test]
    fn digit_prefixed_filename_stays_one_symbol() {
        assert_eq!(
            token("50x150_building.s"),
            Token::Symbol("50x150_building.s".into())
        );
        assert_eq!(
            token("650vcabinet.s"),
            Token::Symbol("650vcabinet.s".into())
        );
        assert_eq!(token("20mberm.s"), Token::Symbol("20mberm.s".into()));
        // Incomplete exponent is still one STF item (not Integer + Symbol).
        assert_eq!(token("4994E"), Token::Symbol("4994E".into()));
    }
}
