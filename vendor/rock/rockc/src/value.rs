use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::interpreter::Env;

#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<String>),
    Array(Rc<RefCell<Vec<Value>>>),
    Map(Rc<RefCell<Vec<(Value, Value)>>>),
    Struct(Rc<RefCell<Struct>>),
    TypeRef(Rc<String>),
    Range(i64, i64),
    Function(Rc<Closure>),
    Overloads(Rc<Vec<Rc<Closure>>>),
    Native(NativeFn),
    Task(Rc<RefCell<TaskState>>),
}

pub enum TaskState {
    Pending { callee: Value, args: Vec<Value>, id: u64 },
    Running { id: u64 },
    Ready { id: u64, value: Value },
    Failed { id: u64, message: String },
}

impl TaskState {
    pub fn id(&self) -> u64 {
        match self {
            TaskState::Pending { id, .. } => *id,
            TaskState::Running { id } => *id,
            TaskState::Ready { id, .. } => *id,
            TaskState::Failed { id, .. } => *id,
        }
    }
}

pub type NativeFn = Rc<dyn Fn(&[Value]) -> crate::error::Result<Value>>;

pub struct Struct {
    pub type_name: String,
    pub fields: Vec<(String, Value)>,
}

pub struct Closure {
    pub func: Rc<crate::ast::Function>,
    pub captured: Option<Rc<RefCell<Env>>>,
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "str",
            Value::Array(_) => "array",
            Value::Map(_) => "map",
            Value::Struct(s) => {
                let _ = s;
                "struct"
            }
            Value::TypeRef(_) => "type",
            Value::Range(..) => "range",
            Value::Function(_) => "fn",
            Value::Overloads(_) => "overloaded-fn",
            Value::Native(_) => "native",
            Value::Task(_) => "task",
        }
    }

    pub fn type_name_ext(&self) -> &'static str {
        if matches!(self, Value::Task(_)) { "task" } else { self.type_name() }
    }
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Nil => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Array(a) => !a.borrow().is_empty(),
            Value::Map(m) => !m.borrow().is_empty(),
            Value::Struct(_) => true,
            _ => true,
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Nil => write!(f, "nil"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(i) => write!(f, "{}", i),
            Value::Float(x) => {
                if x.fract() == 0.0 && x.is_finite() {
                    write!(f, "{:.1}", x)
                } else {
                    write!(f, "{}", x)
                }
            }
            Value::Str(s) => write!(f, "{}", s),
            Value::Array(a) => {
                let a = a.borrow();
                write!(f, "[")?;
                for (i, v) in a.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Value::Map(m) => {
                let m = m.borrow();
                write!(f, "{{")?;
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    match k {
                        Value::Str(s) => write!(f, "\"{}\": {}", s, v)?,
                        _ => write!(f, "{}: {}", k, v)?,
                    }
                }
                write!(f, "}}")
            }
            Value::Range(a, b) => write!(f, "{}..{}", a, b),
            Value::Function(c) => write!(f, "<fn {}>", c.func.name),
            Value::Overloads(fs) => write!(f, "<overloaded fn '{}' x{}>",
                fs.first().map(|c| c.func.name.as_str()).unwrap_or("?"), fs.len()),
            Value::Native(_) => write!(f, "<native fn>"),
            Value::Struct(s) => {
                let s = s.borrow();
                write!(f, "{} {{", s.type_name)?;
                for (i, (name, val)) in s.fields.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}: {}", name, val)?;
                }
                write!(f, "}}")
            }
            Value::TypeRef(name) => write!(f, "<type {}>", name),
            Value::Task(t) => {
                let t = t.borrow();
                match &*t {
                    TaskState::Pending { id, .. } => write!(f, "<task #{} pending>", id),
                    TaskState::Running { id } => write!(f, "<task #{} running>", id),
                    TaskState::Ready { id, .. } => write!(f, "<task #{} done>", id),
                    TaskState::Failed { id, message } => write!(f, "<task #{} failed: {}>", id, message),
                }
            }
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Int(a), Value::Float(b)) | (Value::Float(b), Value::Int(a)) => (*a as f64) == *b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => *a.borrow() == *b.borrow(),
            (Value::Map(a), Value::Map(b)) => *a.borrow() == *b.borrow(),
            (Value::Range(a1, a2), Value::Range(b1, b2)) => a1 == b1 && a2 == b2,
            (Value::Task(a), Value::Task(b)) => a.borrow().id() == b.borrow().id(),
            _ => false,
        }
    }
}
