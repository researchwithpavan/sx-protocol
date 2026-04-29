#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LParen,
    RParen,
    Colon,
    Comma,
    Eq,
    Question,
    At,
    Identifier(String),
    String(String),
    Number(String),
    Path(String),
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

pub struct Lexer<'a> {
    src: &'a [u8],
    i: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src: src.as_bytes(),
            i: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, String> {
        let mut out = Vec::new();
        loop {
            self.skip_ws_and_comments()?;
            let line = self.line;
            let col = self.col;
            let kind = match self.peek() {
                None => TokenKind::Eof,
                Some(b'{') => {
                    self.bump();
                    TokenKind::LBrace
                }
                Some(b'}') => {
                    self.bump();
                    TokenKind::RBrace
                }
                Some(b'[') => {
                    self.bump();
                    TokenKind::LBracket
                }
                Some(b']') => {
                    self.bump();
                    TokenKind::RBracket
                }
                Some(b'(') => {
                    self.bump();
                    TokenKind::LParen
                }
                Some(b')') => {
                    self.bump();
                    TokenKind::RParen
                }
                Some(b':') => {
                    self.bump();
                    TokenKind::Colon
                }
                Some(b',') => {
                    self.bump();
                    TokenKind::Comma
                }
                Some(b'=') => {
                    self.bump();
                    TokenKind::Eq
                }
                Some(b'#') if self.peek_n(1).map(|c| c.is_ascii_digit()).unwrap_or(false) => {
                    TokenKind::Number(self.lex_hash_number())
                }
                Some(b'?') => {
                    self.bump();
                    TokenKind::Question
                }
                Some(b'@') => {
                    self.bump();
                    TokenKind::At
                }
                Some(b'"') => TokenKind::String(self.lex_string()?),
                Some(b'/') => TokenKind::Path(self.lex_path()),
                Some(c) if is_ident_start(c) => TokenKind::Identifier(self.lex_ident()),
                Some(c) if is_num_start(c) => TokenKind::Number(self.lex_number()),
                Some(c) => {
                    return Err(format!(
                        "unexpected character '{}' at {}:{}",
                        c as char, line, col
                    ));
                }
            };
            out.push(Token {
                kind: kind.clone(),
                line,
                column: col,
            });
            if matches!(kind, TokenKind::Eof) {
                break;
            }
        }
        Ok(out)
    }

    fn skip_ws_and_comments(&mut self) -> Result<(), String> {
        loop {
            while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                self.bump();
            }
            let save = self.i;
            if self.peek() == Some(b'/') && self.peek_n(1) == Some(b'/') {
                while let Some(c) = self.peek() {
                    self.bump();
                    if c == b'\n' {
                        break;
                    }
                }
                continue;
            }
            if self.peek() == Some(b'#')
                && !self.peek_n(1).map(|c| c.is_ascii_digit()).unwrap_or(false)
            {
                while let Some(c) = self.peek() {
                    self.bump();
                    if c == b'\n' {
                        break;
                    }
                }
                continue;
            }
            if self.peek() == Some(b'/') && self.peek_n(1) == Some(b'*') {
                self.bump();
                self.bump();
                loop {
                    match (self.peek(), self.peek_n(1)) {
                        (Some(b'*'), Some(b'/')) => {
                            self.bump();
                            self.bump();
                            break;
                        }
                        (Some(_), _) => {
                            self.bump();
                        }
                        (None, _) => return Err("unterminated block comment".to_string()),
                    }
                }
                continue;
            }
            if self.i == save {
                break;
            }
        }
        Ok(())
    }

    fn lex_string(&mut self) -> Result<String, String> {
        self.expect_byte(b'"')?;
        let mut out = String::new();
        while let Some(ch) = self.peek() {
            self.bump();
            match ch {
                b'"' => return Ok(out),
                b'\\' => {
                    let next = self
                        .peek()
                        .ok_or_else(|| "unterminated escape".to_string())?;
                    self.bump();
                    out.push(match next {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{0008}',
                        b'f' => '\u{000C}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        _ => return Err("unsupported escape".to_string()),
                    });
                }
                _ => out.push(ch as char),
            }
        }
        Err("unterminated string".to_string())
    }

    fn lex_path(&mut self) -> String {
        let start = self.i;
        self.bump();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'/' || c == b'.' {
                self.bump();
            } else {
                break;
            }
        }
        String::from_utf8_lossy(&self.src[start..self.i]).to_string()
    }

    fn lex_ident(&mut self) -> String {
        let start = self.i;
        self.bump();
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                self.bump();
            } else {
                break;
            }
        }
        String::from_utf8_lossy(&self.src[start..self.i]).to_string()
    }

    fn lex_number(&mut self) -> String {
        let start = self.i;
        self.bump();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-') {
                self.bump();
            } else {
                break;
            }
        }
        String::from_utf8_lossy(&self.src[start..self.i]).to_string()
    }

    fn lex_hash_number(&mut self) -> String {
        self.bump();
        let start = self.i;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.bump();
            } else {
                break;
            }
        }
        String::from_utf8_lossy(&self.src[start..self.i]).to_string()
    }

    fn expect_byte(&mut self, b: u8) -> Result<(), String> {
        if self.peek() == Some(b) {
            self.bump();
            Ok(())
        } else {
            Err(format!("expected '{}', got {:?}", b as char, self.peek()))
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.i).copied()
    }

    fn peek_n(&self, n: usize) -> Option<u8> {
        self.src.get(self.i + n).copied()
    }

    fn bump(&mut self) {
        if let Some(c) = self.peek() {
            self.i += 1;
            if c == b'\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
    }
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_continue(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'.' || c == b'-'
}

fn is_num_start(c: u8) -> bool {
    c.is_ascii_digit() || c == b'-'
}
