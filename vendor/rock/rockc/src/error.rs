use std::fmt;

#[derive(Debug, Clone)]
pub struct RockError {
    pub kind: ErrorKind,
    pub message: String,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum ErrorKind {
    Lex,
    Parse,
    Runtime,
    Type,
}

impl fmt::Display for RockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tag = match self.kind {
            ErrorKind::Lex => "LexError",
            ErrorKind::Parse => "ParseError",
            ErrorKind::Runtime => "RuntimeError",
            ErrorKind::Type => "TypeError",
        };
        write!(f, "{} at {}:{}: {}", tag, self.line, self.col, self.message)
    }
}

impl std::error::Error for RockError {}

impl RockError {
    pub fn lex(msg: impl Into<String>, line: usize, col: usize) -> Self {
        Self { kind: ErrorKind::Lex, message: msg.into(), line, col }
    }
    pub fn parse(msg: impl Into<String>, line: usize, col: usize) -> Self {
        Self { kind: ErrorKind::Parse, message: msg.into(), line, col }
    }
    pub fn runtime(msg: impl Into<String>) -> Self {
        Self { kind: ErrorKind::Runtime, message: msg.into(), line: 0, col: 0 }
    }
    pub fn type_err(msg: impl Into<String>) -> Self {
        Self { kind: ErrorKind::Type, message: msg.into(), line: 0, col: 0 }
    }

    pub fn pretty(&self, source: &str, file: Option<&str>) -> String {
        let tag = match self.kind {
            ErrorKind::Lex => "lex error",
            ErrorKind::Parse => "parse error",
            ErrorKind::Runtime => "runtime error",
            ErrorKind::Type => "type error",
        };
        let mut out = String::new();
        let loc = if self.line > 0 {
            match file {
                Some(f) => format!("{}:{}:{}", f, self.line, self.col),
                None => format!("{}:{}", self.line, self.col),
            }
        } else {
            file.map(|f| f.to_string()).unwrap_or_default()
        };
        out.push_str(&format!("error[{}]: {}\n", tag, self.message));
        if !loc.is_empty() {
            out.push_str(&format!(" --> {}\n", loc));
        }
        if self.line > 0 {
            let lines: Vec<&str> = source.lines().collect();
            let lineno = self.line;
            if lineno >= 1 && lineno <= lines.len() {
                let line_text = lines[lineno - 1];
                let gutter_w = lineno.to_string().len();
                out.push_str(&format!("{:>w$} |\n", "", w = gutter_w));
                out.push_str(&format!("{:>w$} | {}\n", lineno, line_text, w = gutter_w));
                let mut caret = String::new();
                for _ in 0..self.col.saturating_sub(1) { caret.push(' '); }
                caret.push('^');
                out.push_str(&format!("{:>w$} | {}\n", "", caret, w = gutter_w));
            }
        }
        out
    }
}

pub type Result<T> = std::result::Result<T, RockError>;
