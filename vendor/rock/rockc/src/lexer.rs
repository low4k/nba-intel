use crate::error::{Result, RockError};
use crate::token::{InterpPiece, Span, Spanned, Token};

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self { src: src.as_bytes(), pos: 0, line: 1, col: 1 }
    }

    pub fn tokenize(mut self) -> Result<Vec<Spanned>> {
        let mut out = Vec::new();
        loop {
            self.skip_trivia();
            if self.pos >= self.src.len() {
                out.push(Spanned { token: Token::Eof, span: self.span() });
                return Ok(out);
            }
            let span = self.span();
            let tok = self.next_token()?;
            out.push(Spanned { token: tok, span });
        }
    }

    fn span(&self) -> Span {
        Span { line: self.line, col: self.col }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.src.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let ch = self.peek()?;
        self.pos += 1;
        if ch == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn matches(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') => {
                    self.advance();
                }
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    while let Some(c) = self.peek() {
                        if c == b'\n' { break; }
                        self.advance();
                    }
                }
                Some(b'/') if self.peek_at(1) == Some(b'*') => {
                    self.advance(); self.advance();
                    while let Some(c) = self.peek() {
                        if c == b'*' && self.peek_at(1) == Some(b'/') {
                            self.advance(); self.advance();
                            break;
                        }
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    fn next_token(&mut self) -> Result<Token> {
        let start_line = self.line;
        let start_col = self.col;
        let c = self.advance().unwrap();

        match c {
            b'+' => Ok(if self.matches(b'=') { Token::PlusAssign } else { Token::Plus }),
            b'-' => {
                if self.matches(b'=') { Ok(Token::MinusAssign) }
                else if self.matches(b'>') { Ok(Token::Arrow) }
                else { Ok(Token::Minus) }
            }
            b'*' => Ok(if self.matches(b'=') { Token::StarAssign } else { Token::Star }),
            b'/' => Ok(if self.matches(b'=') { Token::SlashAssign } else { Token::Slash }),
            b'%' => Ok(Token::Percent),
            b'@' => Ok(Token::At),
            b'(' => Ok(Token::LParen),
            b')' => Ok(Token::RParen),
            b'{' => Ok(Token::LBrace),
            b'}' => Ok(Token::RBrace),
            b'[' => Ok(Token::LBracket),
            b']' => Ok(Token::RBracket),
            b',' => Ok(Token::Comma),
            b';' => Ok(Token::Semicolon),
            b'&' => Ok(Token::Amp),
            b'~' => {
                if self.matches(b'>') { Ok(Token::ReactiveArrow) }
                else { Err(crate::error::RockError::lex("unexpected '~'", self.line, self.col)) }
            }
            b'|' => {
                if self.matches(b'>') { Ok(Token::PipeArrow) }
                else if self.matches(b'|') { Ok(Token::Or) }
                else { Ok(Token::Pipe) }
            }
            b'.' => Ok(if self.matches(b'.') { Token::DotDot } else { Token::Dot }),
            b':' => {
                if self.matches(b'=') { Ok(Token::Walrus) }
                else { Ok(Token::Colon) }
            }
            b'=' => {
                if self.matches(b'=') { Ok(Token::Eq) }
                else if self.matches(b'>') { Ok(Token::FatArrow) }
                else { Ok(Token::Assign) }
            }
            b'!' => Ok(if self.matches(b'=') { Token::Neq } else { Token::Bang }),
            b'<' => Ok(if self.matches(b'=') { Token::Le } else { Token::Lt }),
            b'>' => Ok(if self.matches(b'=') { Token::Ge } else { Token::Gt }),
            b'?' => {
                if self.matches(b'?') { Ok(Token::DoubleQuestion) }
                else if self.matches(b'.') { Ok(Token::QuestionDot) }
                else { Ok(Token::Question) }
            }
            b'"' => self.read_string(start_line, start_col),
            b'`' => self.read_interp_string(start_line, start_col),
            c if c.is_ascii_digit() => self.read_number(c),
            c if c == b'_' || c.is_ascii_alphabetic() => self.read_ident(c),
            other => Err(RockError::lex(
                format!("unexpected character '{}'", other as char),
                start_line,
                start_col,
            )),
        }
    }

    fn read_interp_string(&mut self, line: usize, col: usize) -> Result<Token> {
        let mut pieces = Vec::new();
        let mut cur = String::new();
        loop {
            match self.advance() {
                None => return Err(RockError::lex("unterminated interpolated string", line, col)),
                Some(b'`') => {
                    if !cur.is_empty() { pieces.push(InterpPiece::Lit(cur)); }
                    return Ok(Token::InterpStr(pieces));
                }
                Some(b'\\') => match self.advance() {
                    Some(b'n') => cur.push('\n'),
                    Some(b't') => cur.push('\t'),
                    Some(b'r') => cur.push('\r'),
                    Some(b'\\') => cur.push('\\'),
                    Some(b'`') => cur.push('`'),
                    Some(b'$') => cur.push('$'),
                    Some(c) => cur.push(c as char),
                    None => return Err(RockError::lex("bad escape", line, col)),
                },
                Some(b'$') if self.peek() == Some(b'{') => {
                    self.advance();
                    if !cur.is_empty() {
                        pieces.push(InterpPiece::Lit(std::mem::take(&mut cur)));
                    }
                    let mut expr_src = String::new();
                    let mut depth = 1;
                    loop {
                        match self.advance() {
                            None => return Err(RockError::lex("unterminated interpolation", line, col)),
                            Some(b'{') => { depth += 1; expr_src.push('{'); }
                            Some(b'}') => {
                                depth -= 1;
                                if depth == 0 { break; }
                                expr_src.push('}');
                            }
                            Some(c) => expr_src.push(c as char),
                        }
                    }
                    pieces.push(InterpPiece::Expr(expr_src));
                }
                Some(c) => cur.push(c as char),
            }
        }
    }

    fn read_string(&mut self, line: usize, col: usize) -> Result<Token> {
        let mut s = String::new();
        loop {
            match self.advance() {
                None => return Err(RockError::lex("unterminated string", line, col)),
                Some(b'"') => return Ok(Token::Str(s)),
                Some(b'\\') => {
                    match self.advance() {
                        Some(b'n') => s.push('\n'),
                        Some(b't') => s.push('\t'),
                        Some(b'r') => s.push('\r'),
                        Some(b'\\') => s.push('\\'),
                        Some(b'"') => s.push('"'),
                        Some(b'0') => s.push('\0'),
                        Some(c) => s.push(c as char),
                        None => return Err(RockError::lex("bad escape", line, col)),
                    }
                }
                Some(c) => s.push(c as char),
            }
        }
    }

    fn read_number(&mut self, first: u8) -> Result<Token> {
        // Support 0x.., 0o.., 0b.. integer literals.
        if first == b'0' {
            if let Some(&p) = self.peek().as_ref() {
                let radix_info: Option<(u32, &str)> = match p {
                    b'x' | b'X' => Some((16, "hex")),
                    b'o' | b'O' => Some((8, "octal")),
                    b'b' | b'B' => Some((2, "binary")),
                    _ => None,
                };
                if let Some((radix, label)) = radix_info {
                    self.advance(); // consume the prefix char
                    let mut digits = String::new();
                    while let Some(c) = self.peek() {
                        if c == b'_' { self.advance(); continue; }
                        let valid = match radix {
                            16 => c.is_ascii_hexdigit(),
                            8  => (b'0'..=b'7').contains(&c),
                            2  => c == b'0' || c == b'1',
                            _ => false,
                        };
                        if !valid { break; }
                        self.advance();
                        digits.push(c as char);
                    }
                    if digits.is_empty() {
                        return Err(RockError::lex(format!("empty {} literal", label), self.line, self.col));
                    }
                    return u64::from_str_radix(&digits, radix)
                        .map(|n| Token::Int(n as i64))
                        .map_err(|e| RockError::lex(e.to_string(), self.line, self.col));
                }
            }
        }
        let mut s = String::new();
        s.push(first as char);
        let mut is_float = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == b'_' {
                self.advance();
                if c != b'_' { s.push(c as char); }
            } else if c == b'.' && self.peek_at(1).map_or(false, |c| c.is_ascii_digit()) {
                is_float = true;
                self.advance();
                s.push('.');
            } else {
                break;
            }
        }
        if is_float {
            s.parse::<f64>()
                .map(Token::Float)
                .map_err(|e| RockError::lex(e.to_string(), self.line, self.col))
        } else {
            s.parse::<i64>()
                .map(Token::Int)
                .map_err(|e| RockError::lex(e.to_string(), self.line, self.col))
        }
    }

    fn read_ident(&mut self, first: u8) -> Result<Token> {
        let mut s = String::new();
        s.push(first as char);
        while let Some(c) = self.peek() {
            if c == b'_' || c.is_ascii_alphanumeric() {
                self.advance();
                s.push(c as char);
            } else {
                break;
            }
        }
        Ok(match s.as_str() {
            "fn" => Token::Fn,
            "if" => Token::If,
            "else" => Token::Else,
            "loop" => Token::Loop,
            "while" => Token::While,
            "for" => Token::For,
            "in" => Token::In,
            "return" => Token::Return,
            "break" => Token::Break,
            "continue" => Token::Continue,
            "let" => Token::Let,
            "mut" => Token::Mut,
            "true" => Token::True,
            "false" => Token::False,
            "nil" => Token::Nil,
            "and" => Token::And,
            "or" => Token::Or,
            "not" => Token::Not,
            "import" => Token::Import,
            "spawn" => Token::Spawn,
            "defer" => Token::Defer,
            "print" => Token::Print,
            "match" => Token::Match,
            "type" => Token::Type,
            "impl" => Token::Impl,
            "const" => Token::Const,
            "self" => Token::SelfKw,
            "raw" => {
                // Contextual keyword: `raw` is only the effect-escape keyword
                // when immediately followed by `{` (optionally after whitespace).
                // In any other position it's a plain identifier so that common
                // names like `let raw = ...` work.
                let mut i = self.pos;
                while let Some(&c) = self.src.get(i) {
                    if c == b' ' || c == b'\t' { i += 1; } else { break; }
                }
                if self.src.get(i).copied() == Some(b'{') {
                    Token::Raw
                } else {
                    Token::Ident(s)
                }
            }
            "state_machine" => Token::StateMachine,
            "with" => Token::With,
            "trait" => Token::Trait,
            "await" => Token::Await,
            "enum" => Token::Enum,
            "try" => Token::Try,
            "catch" => Token::Catch,
            _ => Token::Ident(s),
        })
    }
}
