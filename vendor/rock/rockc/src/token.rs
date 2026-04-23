use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Int(i64),
    Float(f64),
    Str(String),
    InterpStr(Vec<InterpPiece>),
    Ident(String),
    True,
    False,
    Nil,

    Fn,
    If,
    Else,
    Loop,
    While,
    For,
    In,
    Return,
    Break,
    Continue,
    Let,
    Mut,
    Import,
    Spawn,
    Defer,
    Print,
    Match,
    Type,
    Impl,
    Const,
    SelfKw,
    Raw,
    StateMachine,
    With,
    At,
    Trait,
    Await,
    Enum,
    Try,
    Catch,

    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Assign,
    Walrus,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,

    Eq,
    Neq,
    Lt,
    Gt,
    Le,
    Ge,

    And,
    Or,
    Not,

    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Colon,
    Dot,
    DotDot,
    Arrow,
    FatArrow,
    Pipe,
    PipeArrow,
    ReactiveArrow,
    Amp,

    Bang,
    Question,
    DoubleQuestion,
    QuestionDot,

    Eof,
}

#[derive(Debug, Clone)]
pub struct Spanned {
    pub token: Token,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InterpPiece {
    Lit(String),
    Expr(String),
}
