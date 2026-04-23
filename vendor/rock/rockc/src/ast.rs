use crate::token::Span;

#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Function(Function),
    Stmt(Stmt),
    Import { path: String, alias: Option<String>, span: Span },
    TypeDecl(TypeDecl),
    EnumDecl(EnumDecl),
    Impl(ImplBlock),
    Const { name: String, value: Expr, span: Span },
    StateMachine(StateMachineDecl),
    Prove(ProveBlock),
    Trait(TraitDecl),
    TraitImpl(TraitImpl),
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<Variant>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub name: String,
    pub kind: VariantKind,
}

#[derive(Debug, Clone)]
pub enum VariantKind {
    Nullary,
    Tuple(usize),
    Named(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct TraitDecl {
    pub name: String,
    pub methods: Vec<TraitMethod>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub name: String,
    pub has_self: bool,
    pub param_count: usize,
    pub default: Option<Function>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TraitImpl {
    pub trait_name: String,
    pub target: String,
    pub methods: Vec<Function>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ProveBlock {
    pub assertions: Vec<ProveAssertion>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ProveAssertion {
    Unreachable { from: Expr, to: Expr, span: Span },
    Never { expr: Expr, span: Span },
}

#[derive(Debug, Clone)]
pub struct StateMachineDecl {
    pub name: String,
    pub states: Vec<String>,
    pub transitions: Vec<(String, String)>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TypeDecl {
    pub name: String,
    pub type_params: Vec<String>,
    pub fields: Vec<Field>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub ty: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImplBlock {
    pub target: String,
    pub type_params: Vec<String>,
    pub methods: Vec<Function>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Block,
    pub span: Span,
    pub attrs: Vec<Attribute>,
    pub has_self: bool,
}

#[derive(Debug, Clone)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Option<String>,
    pub literal: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let { name: String, mutable: bool, value: Expr, span: Span },
    LetPattern { pattern: Pattern, value: Expr, span: Span },
    Assign { target: Expr, op: AssignOp, value: Expr, span: Span },
    Expr(Expr),
    Return(Option<Expr>, Span),
    Break(Span),
    Continue(Span),
    While { cond: Expr, body: Block, span: Span },
    Loop { body: Block, span: Span },
    For { var: String, iter: Expr, body: Block, span: Span },
    Defer { body: Block, span: Span },
    With { ctx: Expr, body: Block, span: Span },
    Reactive { name: String, expr: Expr, span: Span },
    TryCatch { try_body: Block, err_name: String, catch_body: Block, span: Span },
}

#[derive(Debug, Clone, Copy)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64, Span),
    Float(f64, Span),
    Str(String, Span),
    Bool(bool, Span),
    Nil(Span),
    Ident(String, Span),
    SelfExpr(Span),
    Array(Vec<Expr>, Span),
    Map(Vec<(Expr, Expr)>, Span),
    StructLit { name: String, fields: Vec<(String, Expr)>, span: Span },
    MethodCall { receiver: Box<Expr>, method: String, args: Vec<Expr>, span: Span },
    Path { segments: Vec<String>, span: Span },
    Block(Block),
    Lambda { params: Vec<Param>, body: Block, span: Span },
    Interp(Vec<InterpPart>, Span),
    Match { scrutinee: Box<Expr>, arms: Vec<MatchArm>, span: Span },
    If { cond: Box<Expr>, then: Block, else_branch: Option<Box<Expr>>, span: Span },
    Unary { op: UnaryOp, rhs: Box<Expr>, span: Span },
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr>, span: Span },
    Call { callee: Box<Expr>, args: Vec<Expr>, span: Span },
    Index { base: Box<Expr>, idx: Box<Expr>, span: Span },
    Field { base: Box<Expr>, name: String, span: Span },
    OptField { base: Box<Expr>, name: String, span: Span },
    Range { start: Box<Expr>, end: Box<Expr>, span: Span },
    Panic(Box<Expr>, Span),
    DefaultOr { lhs: Box<Expr>, default: Box<Expr>, span: Span },
    Pipe { lhs: Box<Expr>, rhs: Box<Expr>, span: Span },
    Spawn(Box<Expr>, Span),
    Raw(Block),
    Comptime(Box<Expr>, Span),
    Try(Box<Expr>, Span),
    Await(Box<Expr>, Span),
}

#[derive(Debug, Clone)]
pub enum InterpPart {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Wildcard,
    Literal(Expr),
    Binding(String),
    Range { start: Expr, end: Expr },
    Array { items: Vec<Pattern>, rest: Option<Option<String>> },
    Struct { type_name: Option<String>, fields: Vec<(String, Pattern)>, rest: bool },
    Tuple(Vec<Pattern>),
    Or(Vec<Pattern>),
    VariantCall { name: String, args: Vec<Pattern> },
}

#[derive(Debug, Clone, Copy)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod,
    Eq, Neq, Lt, Gt, Le, Ge,
    And, Or,
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Int(_, s) | Expr::Float(_, s) | Expr::Str(_, s)
            | Expr::Bool(_, s) | Expr::Nil(s) | Expr::Ident(_, s)
            | Expr::SelfExpr(s)
            | Expr::Array(_, s) | Expr::Map(_, s) | Expr::Interp(_, s) => *s,
            Expr::Block(b) => b.span,
            Expr::StructLit { span, .. } | Expr::MethodCall { span, .. }
            | Expr::Path { span, .. }
            | Expr::Lambda { span, .. } | Expr::Match { span, .. }
            | Expr::If { span, .. } | Expr::Unary { span, .. }
            | Expr::Binary { span, .. } | Expr::Call { span, .. }
            | Expr::Index { span, .. } | Expr::Field { span, .. }
            | Expr::OptField { span, .. }
            | Expr::Range { span, .. } | Expr::Panic(_, span)
            | Expr::DefaultOr { span, .. } | Expr::Pipe { span, .. } => *span,
            Expr::Spawn(_, s) | Expr::Comptime(_, s)
            | Expr::Try(_, s) | Expr::Await(_, s) => *s,
            Expr::Raw(b) => b.span,
        }
    }
}
