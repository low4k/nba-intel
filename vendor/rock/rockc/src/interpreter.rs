use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::*;
use crate::error::{Result, RockError};
use crate::value::{Closure, Struct, Value, TaskState};

pub enum Flow {
    Return(Value),
    Break,
    Continue,
    Err(RockError),
}

impl From<RockError> for Flow {
    fn from(e: RockError) -> Self { Flow::Err(e) }
}

type FlowResult<T> = std::result::Result<T, Flow>;

pub struct Env {
    vars: HashMap<String, (Value, bool)>,
    parent: Option<Rc<RefCell<Env>>>,
}

impl Env {
    pub fn new() -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Env { vars: HashMap::new(), parent: None }))
    }

    pub fn with_parent(parent: Rc<RefCell<Env>>) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Env { vars: HashMap::new(), parent: Some(parent) }))
    }

    pub fn define(&mut self, name: &str, value: Value, mutable: bool) {
        self.vars.insert(name.to_string(), (value, mutable));
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some((v, _)) = self.vars.get(name) {
            Some(v.clone())
        } else if let Some(p) = &self.parent {
            p.borrow().get(name)
        } else {
            None
        }
    }

    pub fn set(&mut self, name: &str, value: Value) -> Result<()> {
        if let Some(entry) = self.vars.get_mut(name) {
            if !entry.1 {
                return Err(RockError::runtime(format!("cannot assign to immutable '{}'", name)));
            }
            entry.0 = value;
            return Ok(());
        }
        if let Some(p) = &self.parent {
            return p.borrow_mut().set(name, value);
        }
        Err(RockError::runtime(format!("undefined variable '{}'", name)))
    }

    pub fn names(&self) -> std::collections::HashSet<String> {
        self.vars.keys().cloned().collect()
    }

    pub fn undefine(&mut self, name: &str) -> bool {
        self.vars.remove(name).is_some()
    }
}

pub struct Interpreter {
    globals: Rc<RefCell<Env>>,
    type_decls: HashMap<String, TypeDecl>,
    impls: HashMap<String, HashMap<String, Rc<Function>>>,
    state_machines: HashMap<String, StateMachineDecl>,
    channels: Rc<RefCell<HashMap<u64, Vec<Value>>>>,
    next_chan_id: Rc<RefCell<u64>>,
    reactive: Rc<RefCell<HashMap<String, Expr>>>,
    effects: Rc<RefCell<EffectFlags>>,
    in_reactive: Rc<RefCell<bool>>,
    traits: HashMap<String, TraitDecl>,
    trait_impls: HashMap<String, Vec<String>>,
    task_queue: Rc<RefCell<Vec<Value>>>,
    next_task_id: Rc<RefCell<u64>>,
    variant_info: HashMap<String, VariantKind>,
    skip_main: Rc<RefCell<bool>>,
    /// Paths currently being imported, used to detect circular imports.
    loading_imports: Rc<RefCell<Vec<std::path::PathBuf>>>,
    /// Paths already fully imported, to avoid re-executing shared deps.
    loaded_imports: Rc<RefCell<std::collections::HashSet<std::path::PathBuf>>>,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct EffectFlags {
    pub no_io: bool,
    pub no_alloc: bool,
    pub pure_: bool,
}

impl Interpreter {
    pub fn new() -> Self {
        let globals = Env::new();
        let channels = Rc::new(RefCell::new(HashMap::new()));
        let next_chan_id = Rc::new(RefCell::new(0u64));
        let effects = Rc::new(RefCell::new(EffectFlags::default()));
        Self::install_builtins(&globals, channels.clone(), next_chan_id.clone(), effects.clone());
        Self {
            globals,
            type_decls: HashMap::new(),
            impls: HashMap::new(),
            state_machines: HashMap::new(),
            channels,
            next_chan_id,
            reactive: Rc::new(RefCell::new(HashMap::new())),
            effects,
            in_reactive: Rc::new(RefCell::new(false)),
            traits: HashMap::new(),
            trait_impls: HashMap::new(),
            task_queue: Rc::new(RefCell::new(Vec::new())),
            next_task_id: Rc::new(RefCell::new(0u64)),
            variant_info: HashMap::new(),
            skip_main: Rc::new(RefCell::new(false)),
            loading_imports: Rc::new(RefCell::new(Vec::new())),
            loaded_imports: Rc::new(RefCell::new(std::collections::HashSet::new())),
        }
    }

    fn install_builtins(
        env: &Rc<RefCell<Env>>,
        channels: Rc<RefCell<HashMap<u64, Vec<Value>>>>,
        next_id: Rc<RefCell<u64>>,
        effects: Rc<RefCell<EffectFlags>>,
    ) {
        let guard_io = |effects: Rc<RefCell<EffectFlags>>, name: &'static str| -> Result<()> {
            let e = effects.borrow();
            if e.no_io || e.pure_ {
                return Err(RockError::runtime(format!(
                    "effect violation: cannot call IO builtin '{}' in @no_io/@pure context", name
                )));
            }
            Ok(())
        };
        let _ = guard_io;

        let eff_print = effects.clone();
        let print: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            {
                let e = eff_print.borrow();
                if e.no_io || e.pure_ {
                    return Err(RockError::runtime(
                        "effect violation: 'print' not allowed in @no_io/@pure context"
                    ));
                }
            }
            let parts: Vec<String> = args.iter().map(|v| v.to_string()).collect();
            println!("{}", parts.join(" "));
            Ok(Value::Nil)
        });
        env.borrow_mut().define("print", Value::Native(print), false);

        let len: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 {
                return Err(RockError::runtime("len() takes 1 argument"));
            }
            match &args[0] {
                Value::Array(a) => Ok(Value::Int(a.borrow().len() as i64)),
                Value::Str(s) => Ok(Value::Int(s.chars().count() as i64)),
                Value::Map(m) => Ok(Value::Int(m.borrow().len() as i64)),
                Value::Nil => Ok(Value::Int(0)),
                other => Err(RockError::runtime(format!("len() not defined for {}", other.type_name()))),
            }
        });
        env.borrow_mut().define("len", Value::Native(len), false);

        let push: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 {
                return Err(RockError::runtime("push() takes 2 arguments"));
            }
            match &args[0] {
                Value::Array(a) => {
                    a.borrow_mut().push(args[1].clone());
                    Ok(Value::Nil)
                }
                other => Err(RockError::runtime(format!("push() not defined for {}", other.type_name()))),
            }
        });
        env.borrow_mut().define("push", Value::Native(push), false);

        let str_of: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 {
                return Err(RockError::runtime("str() takes 1 argument"));
            }
            Ok(Value::Str(Rc::new(args[0].to_string())))
        });
        env.borrow_mut().define("str", Value::Native(str_of), false);

        let int_of: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("int() takes 1 argument")); }
            match &args[0] {
                Value::Int(i) => Ok(Value::Int(*i)),
                Value::Float(f) => Ok(Value::Int(*f as i64)),
                Value::Bool(b) => Ok(Value::Int(if *b { 1 } else { 0 })),
                Value::Str(s) => s.trim().parse::<i64>()
                    .map(Value::Int)
                    .map_err(|e| RockError::runtime(format!("int(): {}", e))),
                other => Err(RockError::runtime(format!("int() not defined for {}", other.type_name()))),
            }
        });
        env.borrow_mut().define("int", Value::Native(int_of), false);

        let float_of: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("float() takes 1 argument")); }
            match &args[0] {
                Value::Int(i) => Ok(Value::Float(*i as f64)),
                Value::Float(f) => Ok(Value::Float(*f)),
                Value::Str(s) => s.trim().parse::<f64>()
                    .map(Value::Float)
                    .map_err(|e| RockError::runtime(format!("float(): {}", e))),
                other => Err(RockError::runtime(format!("float() not defined for {}", other.type_name()))),
            }
        });
        env.borrow_mut().define("float", Value::Native(float_of), false);

        // Safe parsers: return nil on failure instead of panicking. Compose
        // naturally with the `??` default operator: `parse_int(s) ?? 0`.
        let parse_int_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("parse_int() takes 1 argument")); }
            match &args[0] {
                Value::Int(i) => Ok(Value::Int(*i)),
                Value::Float(f) => Ok(Value::Int(*f as i64)),
                Value::Bool(b) => Ok(Value::Int(if *b { 1 } else { 0 })),
                Value::Str(s) => Ok(s.trim().parse::<i64>().map(Value::Int).unwrap_or(Value::Nil)),
                Value::Nil => Ok(Value::Nil),
                _ => Ok(Value::Nil),
            }
        });
        env.borrow_mut().define("parse_int", Value::Native(parse_int_fn), false);

        let parse_float_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("parse_float() takes 1 argument")); }
            match &args[0] {
                Value::Int(i) => Ok(Value::Float(*i as f64)),
                Value::Float(f) => Ok(Value::Float(*f)),
                Value::Str(s) => Ok(s.trim().parse::<f64>().map(Value::Float).unwrap_or(Value::Nil)),
                Value::Nil => Ok(Value::Nil),
                _ => Ok(Value::Nil),
            }
        });
        env.borrow_mut().define("parse_float", Value::Native(parse_float_fn), false);

        let parse_bool_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("parse_bool() takes 1 argument")); }
            match &args[0] {
                Value::Bool(b) => Ok(Value::Bool(*b)),
                Value::Int(i) => Ok(Value::Bool(*i != 0)),
                Value::Str(s) => {
                    let t = s.trim().to_ascii_lowercase();
                    match t.as_str() {
                        "true" | "yes" | "1" | "on" | "y" | "t" => Ok(Value::Bool(true)),
                        "false" | "no" | "0" | "off" | "n" | "f" | "" => Ok(Value::Bool(false)),
                        _ => Ok(Value::Nil),
                    }
                }
                Value::Nil => Ok(Value::Nil),
                _ => Ok(Value::Nil),
            }
        });
        env.borrow_mut().define("parse_bool", Value::Native(parse_bool_fn), false);

        // ---- Result constructors and helpers (ok/err/is_ok/is_err/unwrap/unwrap_or) ----
        fn mk_ok(v: Value) -> Value {
            Value::Struct(Rc::new(RefCell::new(Struct {
                type_name: "Ok".to_string(),
                fields: vec![("value".to_string(), v)],
            })))
        }
        fn mk_err(v: Value) -> Value {
            Value::Struct(Rc::new(RefCell::new(Struct {
                type_name: "Err".to_string(),
                fields: vec![("message".to_string(), v)],
            })))
        }
        let ok_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("ok() takes 1 argument")); }
            Ok(mk_ok(args[0].clone()))
        });
        env.borrow_mut().define("ok", Value::Native(ok_fn), false);

        let err_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("err() takes 1 argument")); }
            Ok(mk_err(args[0].clone()))
        });
        env.borrow_mut().define("err", Value::Native(err_fn), false);

        let is_ok_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("is_ok() takes 1 argument")); }
            Ok(Value::Bool(matches!(&args[0], Value::Struct(s) if s.borrow().type_name == "Ok")))
        });
        env.borrow_mut().define("is_ok", Value::Native(is_ok_fn), false);

        let is_err_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("is_err() takes 1 argument")); }
            Ok(Value::Bool(matches!(&args[0], Value::Struct(s) if s.borrow().type_name == "Err")))
        });
        env.borrow_mut().define("is_err", Value::Native(is_err_fn), false);

        let unwrap_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("unwrap() takes 1 argument")); }
            match &args[0] {
                Value::Struct(s) if s.borrow().type_name == "Ok" => {
                    Ok(s.borrow().fields.iter().find(|(n,_)| n == "value").map(|(_,v)| v.clone()).unwrap_or(Value::Nil))
                }
                Value::Struct(s) if s.borrow().type_name == "Err" => {
                    let msg = s.borrow().fields.iter().find(|(n,_)| n == "message").map(|(_,v)| v.to_string()).unwrap_or_else(|| "unwrap on Err".to_string());
                    Err(RockError::runtime(format!("unwrap on Err: {}", msg)))
                }
                Value::Nil => Err(RockError::runtime("unwrap on nil")),
                other => Ok(other.clone()),
            }
        });
        env.borrow_mut().define("unwrap", Value::Native(unwrap_fn), false);

        let unwrap_or_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("unwrap_or() takes 2 arguments")); }
            match &args[0] {
                Value::Struct(s) if s.borrow().type_name == "Ok" => {
                    Ok(s.borrow().fields.iter().find(|(n,_)| n == "value").map(|(_,v)| v.clone()).unwrap_or(Value::Nil))
                }
                Value::Struct(s) if s.borrow().type_name == "Err" => Ok(args[1].clone()),
                Value::Nil => Ok(args[1].clone()),
                other => Ok(other.clone()),
            }
        });
        env.borrow_mut().define("unwrap_or", Value::Native(unwrap_or_fn), false);

        // ---- url encoding (percent-encoding, no deps) — exposed via `url.encode` / `url.decode`
        //      to avoid shadowing user-defined url_encode/url_decode in modules that
        //      predate this stdlib addition (e.g. nba-intel's http_util).
        let url_encode_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("url.encode() takes 1 argument")); }
            let s = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                other => other.to_string(),
            };
            let mut out = String::with_capacity(s.len());
            for b in s.bytes() {
                match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
                    _ => out.push_str(&format!("%{:02X}", b)),
                }
            }
            Ok(Value::Str(Rc::new(out)))
        });

        let url_decode_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("url.decode() takes 1 argument")); }
            let s = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                other => other.to_string(),
            };
            let bytes = s.as_bytes();
            let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
            let mut i = 0;
            while i < bytes.len() {
                let b = bytes[i];
                if b == b'+' { out.push(b' '); i += 1; }
                else if b == b'%' && i + 2 < bytes.len() {
                    let hex = &s[i+1..i+3];
                    match u8::from_str_radix(hex, 16) {
                        Ok(v) => { out.push(v); i += 3; }
                        Err(_) => { out.push(b); i += 1; }
                    }
                } else { out.push(b); i += 1; }
            }
            match String::from_utf8(out) {
                Ok(s) => Ok(Value::Str(Rc::new(s))),
                Err(_) => Ok(Value::Nil),
            }
        });
        let url_mod = Value::Map(Rc::new(RefCell::new(vec![
            (Value::Str(Rc::new("encode".to_string())), Value::Native(url_encode_fn)),
            (Value::Str(Rc::new("decode".to_string())), Value::Native(url_decode_fn)),
        ])));
        env.borrow_mut().define("url", url_mod, false);

        let assert_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.is_empty() { return Err(RockError::runtime("assert() takes at least 1 argument")); }
            if !args[0].is_truthy() {
                let msg = args.get(1).map(|v| v.to_string())
                    .unwrap_or_else(|| "assertion failed".to_string());
                return Err(RockError::runtime(format!("assertion failed: {}", msg)));
            }
            Ok(Value::Nil)
        });
        env.borrow_mut().define("assert", Value::Native(assert_fn), false);

        let assert_eq: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("assert_eq() takes 2 arguments")); }
            if args[0] != args[1] {
                return Err(RockError::runtime(format!(
                    "assert_eq failed: left={}, right={}", args[0], args[1]
                )));
            }
            Ok(Value::Nil)
        });
        env.borrow_mut().define("assert_eq", Value::Native(assert_eq), false);

        let eff_input = effects.clone();
        let input_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            {
                let e = eff_input.borrow();
                if e.no_io || e.pure_ {
                    return Err(RockError::runtime("effect violation: 'input' not allowed in @no_io/@pure context"));
                }
            }
            use std::io::{self, BufRead, Write};
            if let Some(prompt) = args.get(0) {
                print!("{}", prompt);
                let _ = io::stdout().flush();
            }
            let stdin = io::stdin();
            let mut line = String::new();
            stdin.lock().read_line(&mut line).map_err(|e| RockError::runtime(e.to_string()))?;
            if line.ends_with('\n') { line.pop(); }
            if line.ends_with('\r') { line.pop(); }
            Ok(Value::Str(Rc::new(line)))
        });
        env.borrow_mut().define("input", Value::Native(input_fn), false);

        let eff_rf = effects.clone();
        let read_file: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            {
                let e = eff_rf.borrow();
                if e.no_io || e.pure_ {
                    return Err(RockError::runtime("effect violation: 'read_file' not allowed in @no_io/@pure context"));
                }
            }
            if args.len() != 1 { return Err(RockError::runtime("read_file() takes 1 argument")); }
            let path = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                other => return Err(RockError::runtime(format!("read_file(): expected str, got {}", other.type_name()))),
            };
            match std::fs::read_to_string(&path) {
                Ok(s) => Ok(Value::Str(Rc::new(s))),
                Err(_) => Ok(Value::Nil),
            }
        });
        env.borrow_mut().define("read_file", Value::Native(read_file), false);

        let eff_wf = effects.clone();
        let write_file: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            {
                let e = eff_wf.borrow();
                if e.no_io || e.pure_ {
                    return Err(RockError::runtime("effect violation: 'write_file' not allowed in @no_io/@pure context"));
                }
            }
            if args.len() != 2 { return Err(RockError::runtime("write_file() takes 2 arguments")); }
            let path = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                other => return Err(RockError::runtime(format!("write_file(): path must be str, got {}", other.type_name()))),
            };
            let content = args[1].to_string();
            std::fs::write(&path, content).map_err(|e| RockError::runtime(e.to_string()))?;
            Ok(Value::Nil)
        });
        env.borrow_mut().define("write_file", Value::Native(write_file), false);

        let eff_tn = effects.clone();
        let time_now: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |_args: &[Value]| {
            {
                let e = eff_tn.borrow();
                if e.no_io || e.pure_ {
                    return Err(RockError::runtime("effect violation: 'time_now' not allowed in @no_io/@pure context"));
                }
            }
            use std::time::{SystemTime, UNIX_EPOCH};
            let d = SystemTime::now().duration_since(UNIX_EPOCH)
                .map_err(|e| RockError::runtime(e.to_string()))?;
            Ok(Value::Float(d.as_secs_f64()))
        });
        env.borrow_mut().define("time_now", Value::Native(time_now), false);

        let keys_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("keys() takes 1 argument")); }
            match &args[0] {
                Value::Map(m) => {
                    let ks: Vec<Value> = m.borrow().iter().map(|(k, _)| k.clone()).collect();
                    Ok(Value::Array(Rc::new(RefCell::new(ks))))
                }
                other => Err(RockError::runtime(format!("keys() not defined for {}", other.type_name()))),
            }
        });
        env.borrow_mut().define("keys", Value::Native(keys_fn), false);

        let values_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("values() takes 1 argument")); }
            match &args[0] {
                Value::Map(m) => {
                    let vs: Vec<Value> = m.borrow().iter().map(|(_, v)| v.clone()).collect();
                    Ok(Value::Array(Rc::new(RefCell::new(vs))))
                }
                other => Err(RockError::runtime(format!("values() not defined for {}", other.type_name()))),
            }
        });
        env.borrow_mut().define("values", Value::Native(values_fn), false);

        let map_set: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 3 { return Err(RockError::runtime("set() takes 3 arguments (map, key, value)")); }
            match &args[0] {
                Value::Map(m) => {
                    let mut m = m.borrow_mut();
                    for entry in m.iter_mut() {
                        if entry.0 == args[1] { entry.1 = args[2].clone(); return Ok(Value::Nil); }
                    }
                    m.push((args[1].clone(), args[2].clone()));
                    Ok(Value::Nil)
                }
                other => Err(RockError::runtime(format!("set() not defined for {}", other.type_name()))),
            }
        });
        env.borrow_mut().define("set", Value::Native(map_set), false);

        let has_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("has() takes 2 arguments")); }
            match &args[0] {
                Value::Map(m) => {
                    for (k, _) in m.borrow().iter() {
                        if k == &args[1] { return Ok(Value::Bool(true)); }
                    }
                    Ok(Value::Bool(false))
                }
                Value::Array(a) => {
                    for v in a.borrow().iter() {
                        if v == &args[1] { return Ok(Value::Bool(true)); }
                    }
                    Ok(Value::Bool(false))
                }
                other => Err(RockError::runtime(format!("has() not defined for {}", other.type_name()))),
            }
        });
        env.borrow_mut().define("has", Value::Native(has_fn), false);

        let range_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            let (start, end) = match args.len() {
                1 => (0i64, match &args[0] {
                    Value::Int(i) => *i,
                    other => return Err(RockError::runtime(format!("range() expected int, got {}", other.type_name()))),
                }),
                2 => match (&args[0], &args[1]) {
                    (Value::Int(a), Value::Int(b)) => (*a, *b),
                    _ => return Err(RockError::runtime("range() expected int arguments")),
                },
                _ => return Err(RockError::runtime("range() takes 1 or 2 arguments")),
            };
            Ok(Value::Range(start, end))
        });
        env.borrow_mut().define("range", Value::Native(range_fn), false);

        let math_floor: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("floor() takes 1 argument")); }
            match &args[0] {
                Value::Float(f) => Ok(Value::Int(f.floor() as i64)),
                Value::Int(i) => Ok(Value::Int(*i)),
                other => Err(RockError::runtime(format!("floor() not defined for {}", other.type_name()))),
            }
        });
        env.borrow_mut().define("floor", Value::Native(math_floor), false);

        let math_sqrt: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("sqrt() takes 1 argument")); }
            match &args[0] {
                Value::Float(f) => Ok(Value::Float(f.sqrt())),
                Value::Int(i) => Ok(Value::Float((*i as f64).sqrt())),
                other => Err(RockError::runtime(format!("sqrt() not defined for {}", other.type_name()))),
            }
        });
        env.borrow_mut().define("sqrt", Value::Native(math_sqrt), false);

        let abs_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("abs() takes 1 argument")); }
            match &args[0] {
                Value::Int(i) => Ok(Value::Int(i.abs())),
                Value::Float(f) => Ok(Value::Float(f.abs())),
                other => Err(RockError::runtime(format!("abs() not defined for {}", other.type_name()))),
            }
        });
        env.borrow_mut().define("abs", Value::Native(abs_fn), false);

        let type_of: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("type_of() takes 1 argument")); }
            Ok(Value::Str(Rc::new(args[0].type_name().to_string())))
        });
        env.borrow_mut().define("type_of", Value::Native(type_of), false);

        let map_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|_args: &[Value]| {
            Err(RockError::runtime("map() builtin requires an interpreter context; use a for loop or list comprehension"))
        });
        let _ = map_fn;

        let chans_new = channels.clone();
        let id_new = next_id.clone();
        let channel_new: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |_args: &[Value]| {
            let mut id = id_new.borrow_mut();
            *id += 1;
            let cid = *id;
            chans_new.borrow_mut().insert(cid, Vec::new());
            let mut fields = Vec::new();
            fields.push(("__chan_id".to_string(), Value::Int(cid as i64)));
            Ok(Value::Struct(Rc::new(RefCell::new(Struct {
                type_name: "Channel".to_string(),
                fields,
            }))))
        });
        env.borrow_mut().define("channel", Value::Native(channel_new), false);

        let chans_send = channels.clone();
        let channel_send: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("send() takes (channel, value)")); }
            let cid = channel_id(&args[0])?;
            chans_send.borrow_mut().entry(cid).or_default().push(args[1].clone());
            Ok(Value::Nil)
        });
        env.borrow_mut().define("send", Value::Native(channel_send), false);

        let chans_recv = channels.clone();
        let channel_recv: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("recv() takes (channel)")); }
            let cid = channel_id(&args[0])?;
            let mut cs = chans_recv.borrow_mut();
            let q = cs.entry(cid).or_default();
            if q.is_empty() { Ok(Value::Nil) } else { Ok(q.remove(0)) }
        });
        env.borrow_mut().define("recv", Value::Native(channel_recv), false);

        let chans_len = channels.clone();
        let channel_len: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("channel_len() takes (channel)")); }
            let cid = channel_id(&args[0])?;
            Ok(Value::Int(chans_len.borrow().get(&cid).map(|q| q.len()).unwrap_or(0) as i64))
        });
        env.borrow_mut().define("channel_len", Value::Native(channel_len), false);

        let panic_fn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            let msg = args.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ");
            Err(RockError::runtime(format!("panic: {}", msg)))
        });
        env.borrow_mut().define("panic", Value::Native(panic_fn), false);

        let array_fill: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 {
                return Err(RockError::runtime("__array_fill: expected (value, count)"));
            }
            let n = match &args[1] {
                Value::Int(i) if *i >= 0 => *i as usize,
                _ => return Err(RockError::runtime("__array_fill: count must be non-negative int")),
            };
            let v = args[0].clone();
            let out = (0..n).map(|_| v.clone()).collect::<Vec<_>>();
            Ok(Value::Array(Rc::new(RefCell::new(out))))
        });
        env.borrow_mut().define("__array_fill", Value::Native(array_fill), false);

        let arena_counter = Rc::new(RefCell::new(0u64));
        let arena_usage = Rc::new(RefCell::new(HashMap::<u64, i64>::new()));

        let counter_new = arena_counter.clone();
        let usage_new = arena_usage.clone();
        let arena_new: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            let size = match args.first() {
                Some(Value::Int(n)) => *n,
                Some(_) => return Err(RockError::runtime("arena_new: size must be int")),
                None => 0,
            };
            let mut c = counter_new.borrow_mut();
            *c += 1;
            let id = *c;
            usage_new.borrow_mut().insert(id, 0);
            let st = Struct {
                type_name: "Arena".to_string(),
                fields: vec![
                    ("__arena_id".to_string(), Value::Int(id as i64)),
                    ("capacity".to_string(), Value::Int(size)),
                ],
            };
            Ok(Value::Struct(Rc::new(RefCell::new(st))))
        });
        env.borrow_mut().define("arena_new", Value::Native(arena_new), false);

        let usage_alloc = arena_usage.clone();
        let arena_alloc: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("arena_alloc: expected (arena, bytes)")); }
            let id = arena_id(&args[0])?;
            let n = match &args[1] {
                Value::Int(n) if *n >= 0 => *n,
                _ => return Err(RockError::runtime("arena_alloc: bytes must be non-negative int")),
            };
            let mut u = usage_alloc.borrow_mut();
            let cur = u.entry(id).or_insert(0);
            *cur += n;
            Ok(Value::Int(*cur))
        });
        env.borrow_mut().define("arena_alloc", Value::Native(arena_alloc), false);

        let usage_clear = arena_usage.clone();
        let arena_clear: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("arena_clear: expected (arena)")); }
            let id = arena_id(&args[0])?;
            usage_clear.borrow_mut().insert(id, 0);
            Ok(Value::Nil)
        });
        env.borrow_mut().define("arena_clear", Value::Native(arena_clear), false);

        let usage_used = arena_usage.clone();
        let arena_used: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("arena_used: expected (arena)")); }
            let id = arena_id(&args[0])?;
            Ok(Value::Int(*usage_used.borrow().get(&id).unwrap_or(&0)))
        });
        env.borrow_mut().define("arena_used", Value::Native(arena_used), false);

        Self::install_std_modules(env, effects.clone());
    }

    fn install_std_modules(env: &Rc<RefCell<Env>>, effects: Rc<RefCell<EffectFlags>>) {
        fn mk_map(entries: Vec<(&str, Value)>) -> Value {
            let pairs: Vec<(Value, Value)> = entries.into_iter()
                .map(|(k, v)| (Value::Str(Rc::new(k.to_string())), v))
                .collect();
            Value::Map(Rc::new(RefCell::new(pairs)))
        }
        fn n1(name: &'static str, f: fn(f64) -> f64) -> Value {
            let cb: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
                if args.len() != 1 { return Err(RockError::runtime(format!("{}: expected 1 argument", name))); }
                let x = match &args[0] {
                    Value::Int(i) => *i as f64,
                    Value::Float(x) => *x,
                    other => return Err(RockError::runtime(format!("{}: expected number, got {}", name, other.type_name()))),
                };
                Ok(Value::Float(f(x)))
            });
            Value::Native(cb)
        }
        fn n2(name: &'static str, f: fn(f64, f64) -> f64) -> Value {
            let cb: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
                if args.len() != 2 { return Err(RockError::runtime(format!("{}: expected 2 arguments", name))); }
                let to_f = |v: &Value| -> Result<f64> {
                    match v {
                        Value::Int(i) => Ok(*i as f64),
                        Value::Float(x) => Ok(*x),
                        other => Err(RockError::runtime(format!("{}: expected number, got {}", name, other.type_name()))),
                    }
                };
                Ok(Value::Float(f(to_f(&args[0])?, to_f(&args[1])?)))
            });
            Value::Native(cb)
        }

        let math = mk_map(vec![
            ("pi", Value::Float(std::f64::consts::PI)),
            ("e", Value::Float(std::f64::consts::E)),
            ("tau", Value::Float(std::f64::consts::TAU)),
            ("inf", Value::Float(f64::INFINITY)),
            ("nan", Value::Float(f64::NAN)),
            ("sin", n1("sin", f64::sin)),
            ("cos", n1("cos", f64::cos)),
            ("tan", n1("tan", f64::tan)),
            ("asin", n1("asin", f64::asin)),
            ("acos", n1("acos", f64::acos)),
            ("atan", n1("atan", f64::atan)),
            ("atan2", n2("atan2", f64::atan2)),
            ("exp", n1("exp", f64::exp)),
            ("ln", n1("ln", f64::ln)),
            ("log2", n1("log2", f64::log2)),
            ("log10", n1("log10", f64::log10)),
            ("sqrt", n1("sqrt", f64::sqrt)),
            ("cbrt", n1("cbrt", f64::cbrt)),
            ("floor", n1("floor", f64::floor)),
            ("ceil", n1("ceil", f64::ceil)),
            ("round", n1("round", f64::round)),
            ("abs", n1("abs", f64::abs)),
            ("pow", n2("pow", f64::powf)),
            ("min", n2("min", f64::min)),
            ("max", n2("max", f64::max)),
            ("hypot", n2("hypot", f64::hypot)),
        ]);
        env.borrow_mut().define("math", math, false);

        // ---- bits module: integer bitwise operations ----
        fn bint(name: &'static str, v: &Value) -> Result<i64> {
            match v {
                Value::Int(i) => Ok(*i),
                other => Err(RockError::runtime(format!("{}: expected int, got {}", name, other.type_name()))),
            }
        }
        fn bin2(name: &'static str, op: fn(i64, i64) -> i64) -> Value {
            let cb: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
                if args.len() != 2 { return Err(RockError::runtime(format!("{}: expected 2 ints", name))); }
                Ok(Value::Int(op(bint(name, &args[0])?, bint(name, &args[1])?)))
            });
            Value::Native(cb)
        }
        let bits_and = bin2("bits.band", |a, b| a & b);
        let bits_or  = bin2("bits.bor",  |a, b| a | b);
        let bits_xor = bin2("bits.bxor", |a, b| a ^ b);
        let bits_shl = bin2("bits.shl", |a, b| ((a as u64).wrapping_shl(b as u32)) as i64);
        let bits_shr = bin2("bits.shr", |a, b| ((a as u64).wrapping_shr(b as u32)) as i64); // logical (unsigned)
        let bits_sar = bin2("bits.sar", |a, b| a.wrapping_shr(b as u32));                    // arithmetic (signed)
        let bits_not: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("bits.bnot: expected 1 int")); }
            Ok(Value::Int(!bint("bits.bnot", &args[0])?))
        });
        let bits_count_ones: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("bits.count_ones: expected 1 int")); }
            Ok(Value::Int(bint("bits.count_ones", &args[0])?.count_ones() as i64))
        });
        let bits_leading_zeros: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("bits.leading_zeros: expected 1 int")); }
            Ok(Value::Int(bint("bits.leading_zeros", &args[0])?.leading_zeros() as i64))
        });
        let bits_trailing_zeros: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("bits.trailing_zeros: expected 1 int")); }
            Ok(Value::Int(bint("bits.trailing_zeros", &args[0])?.trailing_zeros() as i64))
        });
        let bits_rotate_left: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("bits.rotate_left: expected (n, k)")); }
            Ok(Value::Int((bint("bits.rotate_left", &args[0])? as u64).rotate_left(bint("bits.rotate_left", &args[1])? as u32) as i64))
        });
        let bits_rotate_right: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("bits.rotate_right: expected (n, k)")); }
            Ok(Value::Int((bint("bits.rotate_right", &args[0])? as u64).rotate_right(bint("bits.rotate_right", &args[1])? as u32) as i64))
        });
        let bits_get: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("bits.get: expected (n, idx)")); }
            let n = bint("bits.get", &args[0])?;
            let i = bint("bits.get", &args[1])?;
            if !(0..64).contains(&i) { return Err(RockError::runtime("bits.get: idx must be 0..63")); }
            Ok(Value::Int(((n >> i) & 1) as i64))
        });
        let bits_set: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 3 { return Err(RockError::runtime("bits.set: expected (n, idx, v)")); }
            let n = bint("bits.set", &args[0])?;
            let i = bint("bits.set", &args[1])?;
            let v = bint("bits.set", &args[2])?;
            if !(0..64).contains(&i) { return Err(RockError::runtime("bits.set: idx must be 0..63")); }
            let mask = 1i64 << i;
            Ok(Value::Int(if v != 0 { n | mask } else { n & !mask }))
        });
        let bits_to_bin: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("bits.to_bin: expected 1 int")); }
            Ok(Value::Str(Rc::new(format!("{:b}", bint("bits.to_bin", &args[0])? as u64))))
        });
        let bits_to_hex: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("bits.to_hex: expected 1 int")); }
            Ok(Value::Str(Rc::new(format!("{:x}", bint("bits.to_hex", &args[0])? as u64))))
        });
        let bits_from_bin: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("bits.from_bin: expected (string)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str().to_string(), _ => return Err(RockError::runtime("bits.from_bin: expected string")) };
            u64::from_str_radix(s.trim_start_matches("0b"), 2)
                .map(|n| Value::Int(n as i64))
                .map_err(|e| RockError::runtime(format!("bits.from_bin: {}", e)))
        });
        let bits_from_hex: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("bits.from_hex: expected (string)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str().to_string(), _ => return Err(RockError::runtime("bits.from_hex: expected string")) };
            u64::from_str_radix(s.trim_start_matches("0x"), 16)
                .map(|n| Value::Int(n as i64))
                .map_err(|e| RockError::runtime(format!("bits.from_hex: {}", e)))
        });
        let bits_mod = mk_map(vec![
            ("band", bits_and),
            ("bor", bits_or),
            ("bxor", bits_xor),
            ("bnot", Value::Native(bits_not)),
            ("shl", bits_shl),
            ("shr", bits_shr),
            ("sar", bits_sar),
            ("count_ones", Value::Native(bits_count_ones)),
            ("leading_zeros", Value::Native(bits_leading_zeros)),
            ("trailing_zeros", Value::Native(bits_trailing_zeros)),
            ("rotate_left", Value::Native(bits_rotate_left)),
            ("rotate_right", Value::Native(bits_rotate_right)),
            ("get", Value::Native(bits_get)),
            ("set", Value::Native(bits_set)),
            ("to_bin", Value::Native(bits_to_bin)),
            ("to_hex", Value::Native(bits_to_hex)),
            ("from_bin", Value::Native(bits_from_bin)),
            ("from_hex", Value::Native(bits_from_hex)),
        ]);
        env.borrow_mut().define("bits", bits_mod, false);

        let eff_time = effects.clone();
        let time_now_ms: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |_args: &[Value]| {
            {
                let e = eff_time.borrow();
                if e.pure_ {
                    return Err(RockError::runtime("effect violation: 'time.now_ms' not allowed in @pure context"));
                }
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| RockError::runtime(e.to_string()))?;
            Ok(Value::Int(now.as_millis() as i64))
        });
        let time_now_ns: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|_args: &[Value]| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| RockError::runtime(e.to_string()))?;
            Ok(Value::Int(now.as_nanos() as i64))
        });
        let eff_sleep = effects.clone();
        let time_sleep: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            {
                let e = eff_sleep.borrow();
                if e.no_io || e.pure_ {
                    return Err(RockError::runtime("effect violation: 'time.sleep_ms' not allowed in @no_io/@pure context"));
                }
            }
            if args.len() != 1 { return Err(RockError::runtime("time.sleep_ms: expected (ms)")); }
            let ms = match &args[0] {
                Value::Int(i) => *i as u64,
                Value::Float(f) => *f as u64,
                other => return Err(RockError::runtime(format!("time.sleep_ms: expected number, got {}", other.type_name()))),
            };
            std::thread::sleep(std::time::Duration::from_millis(ms));
            Ok(Value::Nil)
        });
        let time_format_ms: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("time.format_ms: expected (ms)")); }
            let ms = match &args[0] { Value::Int(i) => *i, Value::Float(f) => *f as i64, _ => return Err(RockError::runtime("time.format_ms: expected int ms")) };
            // Format as ISO-8601 UTC: YYYY-MM-DDTHH:MM:SS.sssZ (Gregorian, no leap seconds).
            Ok(Value::Str(Rc::new(format_unix_ms_iso(ms))))
        });
        let time_now_iso: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|_args: &[Value]| {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| RockError::runtime(e.to_string()))?;
            Ok(Value::Str(Rc::new(format_unix_ms_iso(now.as_millis() as i64))))
        });
        let time_parse_iso: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("time.parse_iso: expected (iso_str)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str().to_string(), _ => return Err(RockError::runtime("time.parse_iso: expected string")) };
            parse_iso_to_unix_ms(&s).map(Value::Int).map_err(|e| RockError::runtime(format!("time.parse_iso '{}': {}", s, e)))
        });
        let time_mod = mk_map(vec![
            ("now_ms", Value::Native(time_now_ms)),
            ("now_ns", Value::Native(time_now_ns)),
            ("sleep_ms", Value::Native(time_sleep)),
            ("format_ms", Value::Native(time_format_ms)),
            ("now_iso", Value::Native(time_now_iso)),
            ("parse_iso", Value::Native(time_parse_iso)),
        ]);
        env.borrow_mut().define("time", time_mod, false);

        let eff_fs1 = effects.clone();
        let fs_read: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            {
                let e = eff_fs1.borrow();
                if e.no_io || e.pure_ {
                    return Err(RockError::runtime("effect violation: 'fs.read' not allowed in @no_io/@pure context"));
                }
            }
            if args.len() != 1 { return Err(RockError::runtime("fs.read: expected (path)")); }
            let path = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                other => return Err(RockError::runtime(format!("fs.read: expected string, got {}", other.type_name()))),
            };
            std::fs::read_to_string(&path)
                .map(|s| Value::Str(Rc::new(s)))
                .map_err(|e| RockError::runtime(format!("fs.read '{}': {}", path, e)))
        });
        let eff_fs2 = effects.clone();
        let fs_write: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            {
                let e = eff_fs2.borrow();
                if e.no_io || e.pure_ {
                    return Err(RockError::runtime("effect violation: 'fs.write' not allowed in @no_io/@pure context"));
                }
            }
            if args.len() != 2 { return Err(RockError::runtime("fs.write: expected (path, content)")); }
            let path = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                other => return Err(RockError::runtime(format!("fs.write: expected string path, got {}", other.type_name()))),
            };
            let content = args[1].to_string();
            // Auto-create parent directories so fs.write is ergonomic for
            // app data dirs, cache dirs, etc.
            if let Some(parent) = std::path::Path::new(&path).parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| RockError::runtime(format!("fs.write '{}': mkdir parent: {}", path, e)))?;
                }
            }
            std::fs::write(&path, content)
                .map(|_| Value::Nil)
                .map_err(|e| RockError::runtime(format!("fs.write '{}': {}", path, e)))
        });
        let fs_exists: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("fs.exists: expected (path)")); }
            let path = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                other => return Err(RockError::runtime(format!("fs.exists: expected string, got {}", other.type_name()))),
            };
            Ok(Value::Bool(std::path::Path::new(&path).exists()))
        });
        let eff_fs3 = effects.clone();
        let fs_list: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            {
                let e = eff_fs3.borrow();
                if e.no_io || e.pure_ {
                    return Err(RockError::runtime("effect violation: 'fs.list' not allowed in @no_io/@pure context"));
                }
            }
            if args.len() != 1 { return Err(RockError::runtime("fs.list: expected (path)")); }
            let path = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                other => return Err(RockError::runtime(format!("fs.list: expected string, got {}", other.type_name()))),
            };
            let rd = std::fs::read_dir(&path)
                .map_err(|e| RockError::runtime(format!("fs.list '{}': {}", path, e)))?;
            let mut names = Vec::new();
            for entry in rd {
                let entry = entry.map_err(|e| RockError::runtime(e.to_string()))?;
                if let Some(n) = entry.file_name().to_str() {
                    names.push(Value::Str(Rc::new(n.to_string())));
                }
            }
            Ok(Value::Array(Rc::new(RefCell::new(names))))
        });
        let eff_fs4 = effects.clone();
        let fs_mkdir: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            {
                let e = eff_fs4.borrow();
                if e.no_io || e.pure_ {
                    return Err(RockError::runtime("effect violation: 'fs.mkdir' not allowed in @no_io/@pure context"));
                }
            }
            if args.len() != 1 { return Err(RockError::runtime("fs.mkdir: expected (path)")); }
            let path = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                other => return Err(RockError::runtime(format!("fs.mkdir: expected string, got {}", other.type_name()))),
            };
            std::fs::create_dir_all(&path)
                .map(|_| Value::Nil)
                .map_err(|e| RockError::runtime(format!("fs.mkdir '{}': {}", path, e)))
        });
        let fs_mod = mk_map(vec![
            ("read", Value::Native(fs_read)),
            ("write", Value::Native(fs_write)),
            ("exists", Value::Native(fs_exists)),
            ("list", Value::Native(fs_list)),
            ("mkdir", Value::Native(fs_mkdir)),
        ]);
        env.borrow_mut().define("fs", fs_mod, false);

        let json_parse: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("json.parse: expected (text)")); }
            let s = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                other => return Err(RockError::runtime(format!("json.parse: expected string, got {}", other.type_name()))),
            };
            let mut p = JsonParser { src: s.as_bytes(), pos: 0 };
            p.skip_ws();
            let v = p.parse_value()?;
            p.skip_ws();
            if p.pos != p.src.len() {
                return Err(RockError::runtime(format!("json.parse: trailing input at byte {}", p.pos)));
            }
            Ok(v)
        });
        let json_stringify: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("json.stringify: expected (value)")); }
            let mut out = String::new();
            json_write(&args[0], &mut out)?;
            Ok(Value::Str(Rc::new(out)))
        });
        let json_mod = mk_map(vec![
            ("parse", Value::Native(json_parse)),
            ("stringify", Value::Native(json_stringify)),
        ]);
        env.borrow_mut().define("json", json_mod, false);

        let str_trim: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("str.trim: expected (s)")); }
            if let Value::Str(s) = &args[0] { Ok(Value::Str(Rc::new(s.trim().to_string()))) }
            else { Err(RockError::runtime("str.trim: expected string")) }
        });
        let str_upper: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("str.upper: expected (s)")); }
            if let Value::Str(s) = &args[0] { Ok(Value::Str(Rc::new(s.to_uppercase()))) }
            else { Err(RockError::runtime("str.upper: expected string")) }
        });
        let str_lower: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("str.lower: expected (s)")); }
            if let Value::Str(s) = &args[0] { Ok(Value::Str(Rc::new(s.to_lowercase()))) }
            else { Err(RockError::runtime("str.lower: expected string")) }
        });
        let str_split: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("str.split: expected (s, sep)")); }
            let (s, sep) = match (&args[0], &args[1]) {
                (Value::Str(s), Value::Str(sep)) => (s.as_str().to_string(), sep.as_str().to_string()),
                _ => return Err(RockError::runtime("str.split: expected (string, string)")),
            };
            let parts: Vec<Value> = if sep.is_empty() {
                s.chars().map(|c| Value::Str(Rc::new(c.to_string()))).collect()
            } else {
                s.split(sep.as_str()).map(|p| Value::Str(Rc::new(p.to_string()))).collect()
            };
            Ok(Value::Array(Rc::new(RefCell::new(parts))))
        });
        let str_join: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("str.join: expected (parts, sep)")); }
            let (parts, sep) = match (&args[0], &args[1]) {
                (Value::Array(a), Value::Str(sep)) => (a.clone(), sep.as_str().to_string()),
                _ => return Err(RockError::runtime("str.join: expected (array, string)")),
            };
            let strs: Vec<String> = parts.borrow().iter().map(|v| v.to_string()).collect();
            Ok(Value::Str(Rc::new(strs.join(&sep))))
        });
        let str_replace: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 3 { return Err(RockError::runtime("str.replace: expected (s, from, to)")); }
            match (&args[0], &args[1], &args[2]) {
                (Value::Str(s), Value::Str(a), Value::Str(b)) => {
                    Ok(Value::Str(Rc::new(s.replace(a.as_str(), b.as_str()))))
                }
                _ => Err(RockError::runtime("str.replace: expected (string, string, string)")),
            }
        });
        let str_contains: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("str.contains: expected (s, sub)")); }
            match (&args[0], &args[1]) {
                (Value::Str(s), Value::Str(sub)) => Ok(Value::Bool(s.contains(sub.as_str()))),
                _ => Err(RockError::runtime("str.contains: expected (string, string)")),
            }
        });
        let str_starts_with: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("str.starts_with: expected (s, prefix)")); }
            match (&args[0], &args[1]) {
                (Value::Str(s), Value::Str(p)) => Ok(Value::Bool(s.starts_with(p.as_str()))),
                _ => Err(RockError::runtime("str.starts_with: expected (string, string)")),
            }
        });
        let str_ends_with: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("str.ends_with: expected (s, suffix)")); }
            match (&args[0], &args[1]) {
                (Value::Str(s), Value::Str(p)) => Ok(Value::Bool(s.ends_with(p.as_str()))),
                _ => Err(RockError::runtime("str.ends_with: expected (string, string)")),
            }
        });
        let str_mod = mk_map(vec![
            ("trim", Value::Native(str_trim)),
            ("upper", Value::Native(str_upper)),
            ("lower", Value::Native(str_lower)),
            ("split", Value::Native(str_split)),
            ("join", Value::Native(str_join)),
            ("replace", Value::Native(str_replace)),
            ("contains", Value::Native(str_contains)),
            ("starts_with", Value::Native(str_starts_with)),
            ("ends_with", Value::Native(str_ends_with)),
        ]);
        env.borrow_mut().define("strs", str_mod, false);

        let rng_state = Rc::new(RefCell::new(0x9E3779B97F4A7C15u64));
        {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1);
            *rng_state.borrow_mut() ^= now.wrapping_mul(0xBF58476D1CE4E5B9);
        }
        fn splitmix_next(state: &Rc<RefCell<u64>>) -> u64 {
            let mut s = state.borrow_mut();
            *s = s.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = *s;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            z ^ (z >> 31)
        }
        let rs1 = rng_state.clone();
        let rand_seed: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("random.seed: expected (int)")); }
            let seed = match &args[0] {
                Value::Int(i) => *i as u64,
                other => return Err(RockError::runtime(format!("random.seed: expected int, got {}", other.type_name()))),
            };
            *rs1.borrow_mut() = seed ^ 0x9E3779B97F4A7C15;
            Ok(Value::Nil)
        });
        let rs2 = rng_state.clone();
        let rand_int: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("random.int: expected (lo, hi)")); }
            let (lo, hi) = match (&args[0], &args[1]) {
                (Value::Int(a), Value::Int(b)) => (*a, *b),
                _ => return Err(RockError::runtime("random.int: expected (int, int)")),
            };
            if lo > hi { return Err(RockError::runtime("random.int: lo > hi")); }
            let range = (hi - lo + 1) as u64;
            let r = splitmix_next(&rs2) % range;
            Ok(Value::Int(lo + r as i64))
        });
        let rs3 = rng_state.clone();
        let rand_float: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            if !args.is_empty() { return Err(RockError::runtime("random.float: expected ()")); }
            let r = splitmix_next(&rs3);
            let f = (r >> 11) as f64 / ((1u64 << 53) as f64);
            Ok(Value::Float(f))
        });
        let rs4 = rng_state.clone();
        let rand_choice: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("random.choice: expected (array)")); }
            let arr = match &args[0] {
                Value::Array(a) => a.clone(),
                other => return Err(RockError::runtime(format!("random.choice: expected array, got {}", other.type_name()))),
            };
            let items = arr.borrow();
            if items.is_empty() { return Err(RockError::runtime("random.choice: empty array")); }
            let idx = (splitmix_next(&rs4) as usize) % items.len();
            Ok(items[idx].clone())
        });
        let rs5 = rng_state.clone();
        let rand_shuffle: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("random.shuffle: expected (array)")); }
            let arr = match &args[0] {
                Value::Array(a) => a.clone(),
                other => return Err(RockError::runtime(format!("random.shuffle: expected array, got {}", other.type_name()))),
            };
            let mut items = arr.borrow_mut();
            let n = items.len();
            let mut i = n;
            while i > 1 {
                i -= 1;
                let j = (splitmix_next(&rs5) as usize) % (i + 1);
                items.swap(i, j);
            }
            Ok(Value::Nil)
        });
        let random_mod = mk_map(vec![
            ("seed", Value::Native(rand_seed)),
            ("int", Value::Native(rand_int)),
            ("float", Value::Native(rand_float)),
            ("choice", Value::Native(rand_choice)),
            ("shuffle", Value::Native(rand_shuffle)),
        ]);
        env.borrow_mut().define("random", random_mod, false);

        let eff_args = effects.clone();
        let proc_args: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |_args: &[Value]| {
            {
                let e = eff_args.borrow();
                if e.pure_ { return Err(RockError::runtime("effect violation: 'process.args' not allowed in @pure")); }
            }
            let args: Vec<Value> = std::env::args()
                .map(|s| Value::Str(Rc::new(s)))
                .collect();
            Ok(Value::Array(Rc::new(RefCell::new(args))))
        });
        let eff_env = effects.clone();
        let proc_env: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            {
                let e = eff_env.borrow();
                if e.pure_ { return Err(RockError::runtime("effect violation: 'process.env' not allowed in @pure")); }
            }
            if args.len() != 1 { return Err(RockError::runtime("process.env: expected (name)")); }
            let name = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                other => return Err(RockError::runtime(format!("process.env: expected string, got {}", other.type_name()))),
            };
            match std::env::var(&name) {
                Ok(v) => Ok(Value::Str(Rc::new(v))),
                Err(_) => Ok(Value::Nil),
            }
        });
        let proc_exit: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            let code = match args.get(0) {
                Some(Value::Int(i)) => *i as i32,
                None => 0,
                Some(other) => return Err(RockError::runtime(format!("process.exit: expected int, got {}", other.type_name()))),
            };
            std::process::exit(code);
        });
        let eff_cwd = effects.clone();
        let proc_cwd: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |_args: &[Value]| {
            {
                let e = eff_cwd.borrow();
                if e.pure_ { return Err(RockError::runtime("effect violation: 'process.cwd' not allowed in @pure")); }
            }
            let p = std::env::current_dir().map_err(|e| RockError::runtime(e.to_string()))?;
            Ok(Value::Str(Rc::new(p.to_string_lossy().into_owned())))
        });
        let process_mod = mk_map(vec![
            ("args", Value::Native(proc_args)),
            ("env", Value::Native(proc_env)),
            ("exit", Value::Native(proc_exit)),
            ("cwd", Value::Native(proc_cwd)),
        ]);
        env.borrow_mut().define("process", process_mod, false);

        let hash_fnv1a: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("crypto.fnv1a: expected (string)")); }
            let s = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                other => return Err(RockError::runtime(format!("crypto.fnv1a: expected string, got {}", other.type_name()))),
            };
            let mut h: u64 = 0xCBF29CE484222325;
            for b in s.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001B3);
            }
            Ok(Value::Int(h as i64))
        });
        let hash_djb2: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("crypto.djb2: expected (string)")); }
            let s = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                other => return Err(RockError::runtime(format!("crypto.djb2: expected string, got {}", other.type_name()))),
            };
            let mut h: u64 = 5381;
            for b in s.bytes() {
                h = h.wrapping_mul(33).wrapping_add(b as u64);
            }
            Ok(Value::Int(h as i64))
        });
        let b64_encode: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("crypto.base64_encode: expected (string)")); }
            let s = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                other => return Err(RockError::runtime(format!("crypto.base64_encode: expected string, got {}", other.type_name()))),
            };
            const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let bytes = s.as_bytes();
            let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
            let mut i = 0;
            while i + 2 < bytes.len() {
                let n = (bytes[i] as u32) << 16 | (bytes[i+1] as u32) << 8 | bytes[i+2] as u32;
                out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
                out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
                out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
                out.push(ALPHA[(n & 0x3F) as usize] as char);
                i += 3;
            }
            match bytes.len() - i {
                1 => {
                    let n = (bytes[i] as u32) << 16;
                    out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
                    out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
                    out.push('=');
                    out.push('=');
                }
                2 => {
                    let n = (bytes[i] as u32) << 16 | (bytes[i+1] as u32) << 8;
                    out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
                    out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
                    out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
                    out.push('=');
                }
                _ => {}
            }
            Ok(Value::Str(Rc::new(out)))
        });
        let b64_decode: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("crypto.base64_decode: expected (string)")); }
            let s = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                other => return Err(RockError::runtime(format!("crypto.base64_decode: expected string, got {}", other.type_name()))),
            };
            let val = |c: u8| -> Result<u32> {
                Ok(match c {
                    b'A'..=b'Z' => (c - b'A') as u32,
                    b'a'..=b'z' => (c - b'a' + 26) as u32,
                    b'0'..=b'9' => (c - b'0' + 52) as u32,
                    b'+' => 62,
                    b'/' => 63,
                    _ => return Err(RockError::runtime(format!("crypto.base64_decode: invalid char '{}'", c as char))),
                })
            };
            let bytes: Vec<u8> = s.bytes().filter(|&b| b != b'\n' && b != b'\r' && b != b' ').collect();
            if bytes.len() % 4 != 0 { return Err(RockError::runtime("crypto.base64_decode: bad length")); }
            let mut out: Vec<u8> = Vec::with_capacity(bytes.len() / 4 * 3);
            let mut i = 0;
            while i < bytes.len() {
                let c0 = val(bytes[i])?;
                let c1 = val(bytes[i+1])?;
                let (c2, pad2) = if bytes[i+2] == b'=' { (0, true) } else { (val(bytes[i+2])?, false) };
                let (c3, pad3) = if bytes[i+3] == b'=' { (0, true) } else { (val(bytes[i+3])?, false) };
                let n = (c0 << 18) | (c1 << 12) | (c2 << 6) | c3;
                out.push(((n >> 16) & 0xFF) as u8);
                if !pad2 { out.push(((n >> 8) & 0xFF) as u8); }
                if !pad3 { out.push((n & 0xFF) as u8); }
                i += 4;
            }
            let text = String::from_utf8(out).map_err(|e| RockError::runtime(format!("crypto.base64_decode: {}", e)))?;
            Ok(Value::Str(Rc::new(text)))
        });
        let hex_encode: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("crypto.hex_encode: expected (string)")); }
            let s = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                Value::Int(i) => return Ok(Value::Str(Rc::new(format!("{:x}", *i as u64)))),
                other => return Err(RockError::runtime(format!("crypto.hex_encode: expected string, got {}", other.type_name()))),
            };
            let mut out = String::with_capacity(s.len() * 2);
            for b in s.bytes() { out.push_str(&format!("{:02x}", b)); }
            Ok(Value::Str(Rc::new(out)))
        });
        let crypto_mod = mk_map(vec![
            ("fnv1a", Value::Native(hash_fnv1a)),
            ("djb2", Value::Native(hash_djb2)),
            ("base64_encode", Value::Native(b64_encode)),
            ("base64_decode", Value::Native(b64_decode)),
            ("hex_encode", Value::Native(hex_encode)),
        ]);
        env.borrow_mut().define("crypto", crypto_mod, false);

        // ---- regex module ----
        let re_match: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("regex.matches: expected (pattern, text)")); }
            let (pat, text) = match (&args[0], &args[1]) {
                (Value::Str(p), Value::Str(t)) => (p.as_str(), t.as_str()),
                _ => return Err(RockError::runtime("regex.matches: expected (string, string)")),
            };
            let re = regex_compile(pat).map_err(RockError::runtime)?;
            Ok(Value::Bool(re.is_match(text)))
        });
        let re_find: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("regex.find: expected (pattern, text)")); }
            let (pat, text) = match (&args[0], &args[1]) {
                (Value::Str(p), Value::Str(t)) => (p.as_str(), t.as_str()),
                _ => return Err(RockError::runtime("regex.find: expected (string, string)")),
            };
            let re = regex_compile(pat).map_err(RockError::runtime)?;
            match re.find(text) {
                Some(m) => Ok(Value::Str(Rc::new(text[m.0..m.1].to_string()))),
                None => Ok(Value::Nil),
            }
        });
        let re_find_all: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("regex.find_all: expected (pattern, text)")); }
            let (pat, text) = match (&args[0], &args[1]) {
                (Value::Str(p), Value::Str(t)) => (p.as_str(), t.as_str()),
                _ => return Err(RockError::runtime("regex.find_all: expected (string, string)")),
            };
            let re = regex_compile(pat).map_err(RockError::runtime)?;
            let mut out = Vec::new();
            let mut pos = 0;
            while pos <= text.len() {
                match re.find_at(text, pos) {
                    Some((s, e)) => {
                        out.push(Value::Str(Rc::new(text[s..e].to_string())));
                        pos = if e == s { e + 1 } else { e };
                    }
                    None => break,
                }
            }
            Ok(Value::Array(Rc::new(RefCell::new(out))))
        });
        let re_replace: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 3 { return Err(RockError::runtime("regex.replace: expected (pattern, text, repl)")); }
            let (pat, text, repl) = match (&args[0], &args[1], &args[2]) {
                (Value::Str(p), Value::Str(t), Value::Str(r)) => (p.as_str(), t.as_str(), r.as_str()),
                _ => return Err(RockError::runtime("regex.replace: expected (string, string, string)")),
            };
            let re = regex_compile(pat).map_err(RockError::runtime)?;
            let mut out = String::with_capacity(text.len());
            let mut pos = 0;
            while pos <= text.len() {
                match re.find_at(text, pos) {
                    Some((s, e)) => {
                        out.push_str(&text[pos..s]);
                        out.push_str(repl);
                        pos = if e == s { out.push(text.as_bytes().get(e).map(|b| *b as char).unwrap_or(' ')); e + 1 } else { e };
                    }
                    None => { out.push_str(&text[pos..]); break; }
                }
            }
            Ok(Value::Str(Rc::new(out)))
        });
        let re_split: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 2 { return Err(RockError::runtime("regex.split: expected (pattern, text)")); }
            let (pat, text) = match (&args[0], &args[1]) {
                (Value::Str(p), Value::Str(t)) => (p.as_str(), t.as_str()),
                _ => return Err(RockError::runtime("regex.split: expected (string, string)")),
            };
            let re = regex_compile(pat).map_err(RockError::runtime)?;
            let mut out = Vec::new();
            let mut pos = 0;
            while pos <= text.len() {
                match re.find_at(text, pos) {
                    Some((s, e)) => {
                        out.push(Value::Str(Rc::new(text[pos..s].to_string())));
                        pos = if e == s { e + 1 } else { e };
                    }
                    None => break,
                }
            }
            out.push(Value::Str(Rc::new(text[pos.min(text.len())..].to_string())));
            Ok(Value::Array(Rc::new(RefCell::new(out))))
        });
        let regex_mod = mk_map(vec![
            ("matches", Value::Native(re_match)),
            ("find", Value::Native(re_find)),
            ("find_all", Value::Native(re_find_all)),
            ("replace", Value::Native(re_replace)),
            ("split", Value::Native(re_split)),
        ]);
        env.borrow_mut().define("regex", regex_mod, false);

        // ---- http module (plain HTTP only, no TLS) ----
        let eff_http1 = effects.clone();
        let http_get: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            {
                let e = eff_http1.borrow();
                if e.no_io || e.pure_ {
                    return Err(RockError::runtime("effect violation: 'http.get' not allowed in @no_io/@pure context"));
                }
            }
            if args.len() != 1 { return Err(RockError::runtime("http.get: expected (url)")); }
            let url = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                _ => return Err(RockError::runtime("http.get: expected string url")),
            };
            http_request("GET", &url, None).map_err(RockError::runtime)
        });
        let eff_http2 = effects.clone();
        let http_post: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            {
                let e = eff_http2.borrow();
                if e.no_io || e.pure_ {
                    return Err(RockError::runtime("effect violation: 'http.post' not allowed in @no_io/@pure context"));
                }
            }
            if args.len() != 2 { return Err(RockError::runtime("http.post: expected (url, body)")); }
            let url = match &args[0] {
                Value::Str(s) => s.as_str().to_string(),
                _ => return Err(RockError::runtime("http.post: expected string url")),
            };
            let body = match &args[1] {
                Value::Str(s) => s.as_str().to_string(),
                other => other.to_string(),
            };
            http_request("POST", &url, Some(&body)).map_err(RockError::runtime)
        });
        let http_mod = mk_map(vec![
            ("get", Value::Native(http_get)),
            ("post", Value::Native(http_post)),
        ]);
        env.borrow_mut().define("http", http_mod, false);

        // ---- log module (structured JSON Lines to stderr) ----
        fn log_emit(level: &str, args: &[Value]) -> Result<Value> {
            // Build a tiny JSON object: {"ts":..,"level":..,"msg":..,...fields}
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let msg = args.get(0).map(|v| match v {
                Value::Str(s) => s.as_str().to_string(),
                other => other.to_string(),
            }).unwrap_or_default();
            fn json_escape(s: &str) -> String {
                let mut o = String::with_capacity(s.len() + 2);
                o.push('"');
                for c in s.chars() {
                    match c {
                        '"' => o.push_str("\\\""),
                        '\\' => o.push_str("\\\\"),
                        '\n' => o.push_str("\\n"),
                        '\r' => o.push_str("\\r"),
                        '\t' => o.push_str("\\t"),
                        c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
                        c => o.push(c),
                    }
                }
                o.push('"');
                o
            }
            fn json_val(v: &Value) -> String {
                match v {
                    Value::Nil => "null".to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Int(n) => n.to_string(),
                    Value::Float(f) => {
                        if f.is_finite() { f.to_string() } else { "null".to_string() }
                    }
                    Value::Str(s) => json_escape(s.as_str()),
                    Value::Array(a) => {
                        let parts: Vec<String> = a.borrow().iter().map(json_val).collect();
                        format!("[{}]", parts.join(","))
                    }
                    Value::Map(m) => {
                        let parts: Vec<String> = m.borrow().iter().map(|(k, v)| {
                            let ks = match k {
                                Value::Str(s) => s.as_str().to_string(),
                                other => other.to_string(),
                            };
                            format!("{}:{}", json_escape(&ks), json_val(v))
                        }).collect();
                        format!("{{{}}}", parts.join(","))
                    }
                    other => json_escape(&other.to_string()),
                }
            }
            let mut out = String::new();
            out.push('{');
            out.push_str(&format!("\"ts\":{}", ts));
            out.push_str(&format!(",\"level\":{}", json_escape(level)));
            out.push_str(&format!(",\"msg\":{}", json_escape(&msg)));
            if let Some(Value::Map(m)) = args.get(1) {
                for (k, v) in m.borrow().iter() {
                    let ks = match k {
                        Value::Str(s) => s.as_str().to_string(),
                        other => other.to_string(),
                    };
                    if ks == "ts" || ks == "level" || ks == "msg" { continue; }
                    out.push(',');
                    out.push_str(&json_escape(&ks));
                    out.push(':');
                    out.push_str(&json_val(v));
                }
            }
            out.push('}');
            eprintln!("{}", out);
            Ok(Value::Nil)
        }
        let log_info: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args| log_emit("info", args));
        let log_warn: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args| log_emit("warn", args));
        let log_error: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args| log_emit("error", args));
        let log_debug: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args| log_emit("debug", args));
        let log_mod = mk_map(vec![
            ("info", Value::Native(log_info)),
            ("warn", Value::Native(log_warn)),
            ("error", Value::Native(log_error)),
            ("debug", Value::Native(log_debug)),
        ]);
        env.borrow_mut().define("log", log_mod, false);

        // ---- net module (TCP) ----
        let eff_net1 = effects.clone();
        let net_listen: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            { let e = eff_net1.borrow(); if e.no_io || e.pure_ { return Err(RockError::runtime("effect violation: 'net.listen' not allowed in @no_io/@pure")); } }
            if args.len() != 1 { return Err(RockError::runtime("net.listen: expected (addr)")); }
            let addr = match &args[0] { Value::Str(s) => s.as_str().to_string(), _ => return Err(RockError::runtime("net.listen: expected string addr")) };
            let listener = std::net::TcpListener::bind(&addr).map_err(|e| RockError::runtime(format!("net.listen '{}': {}", addr, e)))?;
            let id = net_store_listener(listener);
            Ok(Value::Int(id as i64))
        });
        let eff_net2 = effects.clone();
        let net_accept: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            { let e = eff_net2.borrow(); if e.no_io || e.pure_ { return Err(RockError::runtime("effect violation: 'net.accept' not allowed in @no_io/@pure")); } }
            if args.len() != 1 { return Err(RockError::runtime("net.accept: expected (listener_id)")); }
            let id = match &args[0] { Value::Int(i) => *i as u64, _ => return Err(RockError::runtime("net.accept: expected int listener id")) };
            let listener = net_get_listener(id).ok_or_else(|| RockError::runtime(format!("net.accept: invalid listener id {}", id)))?;
            let (stream, peer) = listener.accept().map_err(|e| RockError::runtime(format!("net.accept: {}", e)))?;
            let sid = net_store_stream(stream);
            let mut entries: Vec<(Value, Value)> = Vec::new();
            entries.push((Value::Str(Rc::new("conn".to_string())), Value::Int(sid as i64)));
            entries.push((Value::Str(Rc::new("peer".to_string())), Value::Str(Rc::new(peer.to_string()))));
            Ok(Value::Map(Rc::new(RefCell::new(entries))))
        });
        let eff_net3 = effects.clone();
        let net_connect: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            { let e = eff_net3.borrow(); if e.no_io || e.pure_ { return Err(RockError::runtime("effect violation: 'net.connect' not allowed in @no_io/@pure")); } }
            if args.len() != 1 { return Err(RockError::runtime("net.connect: expected (addr)")); }
            let addr = match &args[0] { Value::Str(s) => s.as_str().to_string(), _ => return Err(RockError::runtime("net.connect: expected string addr")) };
            let stream = std::net::TcpStream::connect(&addr).map_err(|e| RockError::runtime(format!("net.connect '{}': {}", addr, e)))?;
            let sid = net_store_stream(stream);
            Ok(Value::Int(sid as i64))
        });
        let eff_net4 = effects.clone();
        let net_read: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            { let e = eff_net4.borrow(); if e.no_io || e.pure_ { return Err(RockError::runtime("effect violation: 'net.read' not allowed in @no_io/@pure")); } }
            if args.len() < 1 || args.len() > 2 { return Err(RockError::runtime("net.read: expected (conn_id, [n])")); }
            let id = match &args[0] { Value::Int(i) => *i as u64, _ => return Err(RockError::runtime("net.read: expected int conn id")) };
            let n = if args.len() == 2 {
                match &args[1] { Value::Int(i) => *i as usize, _ => return Err(RockError::runtime("net.read: n must be int")) }
            } else { 4096 };
            let mut buf = vec![0u8; n];
            use std::io::Read;
            let read = net_with_stream(id, |s| s.read(&mut buf))
                .ok_or_else(|| RockError::runtime(format!("net.read: invalid conn id {}", id)))?
                .map_err(|e| RockError::runtime(format!("net.read: {}", e)))?;
            buf.truncate(read);
            Ok(Value::Str(Rc::new(String::from_utf8_lossy(&buf).into_owned())))
        });
        let eff_net5 = effects.clone();
        let net_write: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            { let e = eff_net5.borrow(); if e.no_io || e.pure_ { return Err(RockError::runtime("effect violation: 'net.write' not allowed in @no_io/@pure")); } }
            if args.len() != 2 { return Err(RockError::runtime("net.write: expected (conn_id, data)")); }
            let id = match &args[0] { Value::Int(i) => *i as u64, _ => return Err(RockError::runtime("net.write: expected int conn id")) };
            let data = match &args[1] { Value::Str(s) => s.as_str().as_bytes().to_vec(), other => other.to_string().into_bytes() };
            use std::io::Write;
            let written = net_with_stream(id, |s| s.write(&data))
                .ok_or_else(|| RockError::runtime(format!("net.write: invalid conn id {}", id)))?
                .map_err(|e| RockError::runtime(format!("net.write: {}", e)))?;
            Ok(Value::Int(written as i64))
        });
        let net_close: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("net.close: expected (id)")); }
            let id = match &args[0] { Value::Int(i) => *i as u64, _ => return Err(RockError::runtime("net.close: expected int id")) };
            let removed = net_close_handle(id);
            Ok(Value::Bool(removed))
        });
        let net_mod = mk_map(vec![
            ("listen", Value::Native(net_listen)),
            ("accept", Value::Native(net_accept)),
            ("connect", Value::Native(net_connect)),
            ("read", Value::Native(net_read)),
            ("write", Value::Native(net_write)),
            ("close", Value::Native(net_close)),
        ]);
        env.borrow_mut().define("net", net_mod, false);

        // ---- os module ----
        let eff_os1 = effects.clone();
        let os_args: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |_args: &[Value]| {
            { let e = eff_os1.borrow(); if e.pure_ { return Err(RockError::runtime("effect violation: 'os.args' not allowed in @pure")); } }
            let argv: Vec<Value> = std::env::args().map(|s| Value::Str(Rc::new(s))).collect();
            Ok(Value::Array(Rc::new(RefCell::new(argv))))
        });
        let eff_os2 = effects.clone();
        let os_get_env: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            { let e = eff_os2.borrow(); if e.pure_ { return Err(RockError::runtime("effect violation: 'os.get_env' not allowed in @pure")); } }
            if args.len() != 1 { return Err(RockError::runtime("os.get_env: expected (name)")); }
            let name = match &args[0] { Value::Str(s) => s.as_str().to_string(), _ => return Err(RockError::runtime("os.get_env: expected string")) };
            Ok(match std::env::var(&name) { Ok(v) => Value::Str(Rc::new(v)), Err(_) => Value::Nil })
        });
        let eff_os3 = effects.clone();
        let os_set_env: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            { let e = eff_os3.borrow(); if e.no_io || e.pure_ { return Err(RockError::runtime("effect violation: 'os.set_env' not allowed in @no_io/@pure")); } }
            if args.len() != 2 { return Err(RockError::runtime("os.set_env: expected (name, value)")); }
            let name = match &args[0] { Value::Str(s) => s.as_str().to_string(), _ => return Err(RockError::runtime("os.set_env: name must be string")) };
            let val = match &args[1] { Value::Str(s) => s.as_str().to_string(), other => other.to_string() };
            std::env::set_var(&name, &val);
            Ok(Value::Nil)
        });
        let eff_os4 = effects.clone();
        let os_env_all: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |_args: &[Value]| {
            { let e = eff_os4.borrow(); if e.pure_ { return Err(RockError::runtime("effect violation: 'os.env' not allowed in @pure")); } }
            let pairs: Vec<(Value, Value)> = std::env::vars()
                .map(|(k, v)| (Value::Str(Rc::new(k)), Value::Str(Rc::new(v))))
                .collect();
            Ok(Value::Map(Rc::new(RefCell::new(pairs))))
        });
        let os_platform: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|_| Ok(Value::Str(Rc::new(std::env::consts::OS.to_string()))));
        let os_arch: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|_| Ok(Value::Str(Rc::new(std::env::consts::ARCH.to_string()))));
        let os_family: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|_| Ok(Value::Str(Rc::new(std::env::consts::FAMILY.to_string()))));
        let eff_os5 = effects.clone();
        let os_cwd: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |_| {
            { let e = eff_os5.borrow(); if e.pure_ { return Err(RockError::runtime("effect violation: 'os.cwd' not allowed in @pure")); } }
            let p = std::env::current_dir().map_err(|e| RockError::runtime(format!("os.cwd: {}", e)))?;
            Ok(Value::Str(Rc::new(p.to_string_lossy().into_owned())))
        });
        let eff_os6 = effects.clone();
        let os_chdir: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
            { let e = eff_os6.borrow(); if e.no_io || e.pure_ { return Err(RockError::runtime("effect violation: 'os.chdir' not allowed in @no_io/@pure")); } }
            if args.len() != 1 { return Err(RockError::runtime("os.chdir: expected (path)")); }
            let p = match &args[0] { Value::Str(s) => s.as_str().to_string(), _ => return Err(RockError::runtime("os.chdir: expected string")) };
            std::env::set_current_dir(&p).map_err(|e| RockError::runtime(format!("os.chdir '{}': {}", p, e)))?;
            Ok(Value::Nil)
        });
        let os_exit: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            let code = match args.first() { Some(Value::Int(i)) => *i as i32, Some(_) => return Err(RockError::runtime("os.exit: code must be int")), None => 0 };
            std::process::exit(code);
        });
        let os_hostname: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|_| {
            // Try $HOSTNAME, then `hostname` command, then "unknown".
            if let Ok(h) = std::env::var("HOSTNAME") { if !h.is_empty() { return Ok(Value::Str(Rc::new(h))); } }
            if let Ok(out) = std::process::Command::new("hostname").output() {
                if out.status.success() {
                    let h = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !h.is_empty() { return Ok(Value::Str(Rc::new(h))); }
                }
            }
            Ok(Value::Str(Rc::new("unknown".to_string())))
        });
        let os_user_home: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|_| {
            Ok(match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
                Ok(h) => Value::Str(Rc::new(h)),
                Err(_) => Value::Nil,
            })
        });
        let os_temp_dir: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|_| {
            Ok(Value::Str(Rc::new(std::env::temp_dir().to_string_lossy().into_owned())))
        });
        let os_mod = mk_map(vec![
            ("args", Value::Native(os_args)),
            ("get_env", Value::Native(os_get_env)),
            ("set_env", Value::Native(os_set_env)),
            ("env", Value::Native(os_env_all)),
            ("platform", Value::Native(os_platform)),
            ("arch", Value::Native(os_arch)),
            ("family", Value::Native(os_family)),
            ("cwd", Value::Native(os_cwd)),
            ("chdir", Value::Native(os_chdir)),
            ("exit", Value::Native(os_exit)),
            ("hostname", Value::Native(os_hostname)),
            ("user_home", Value::Native(os_user_home)),
            ("temp_dir", Value::Native(os_temp_dir)),
            ("path_sep", Value::Str(Rc::new(std::path::MAIN_SEPARATOR.to_string()))),
            ("line_sep", Value::Str(Rc::new(if cfg!(windows) { "\r\n".to_string() } else { "\n".to_string() }))),
        ]);
        env.borrow_mut().define("os", os_mod, false);

        // ---- path module (pure, no effects required) ----
        let path_join: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.is_empty() { return Err(RockError::runtime("path.join: expected at least 1 path")); }
            let mut pb = std::path::PathBuf::new();
            for a in args {
                let s = match a { Value::Str(s) => s.as_str().to_string(), _ => return Err(RockError::runtime("path.join: expected strings")) };
                pb.push(s);
            }
            Ok(Value::Str(Rc::new(pb.to_string_lossy().into_owned())))
        });
        let path_dirname: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("path.dirname: expected (path)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str(), _ => return Err(RockError::runtime("path.dirname: expected string")) };
            let p = std::path::Path::new(s);
            Ok(Value::Str(Rc::new(p.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default())))
        });
        let path_basename: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("path.basename: expected (path)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str(), _ => return Err(RockError::runtime("path.basename: expected string")) };
            let p = std::path::Path::new(s);
            Ok(Value::Str(Rc::new(p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())))
        });
        let path_stem: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("path.stem: expected (path)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str(), _ => return Err(RockError::runtime("path.stem: expected string")) };
            let p = std::path::Path::new(s);
            Ok(Value::Str(Rc::new(p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())))
        });
        let path_ext: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("path.ext: expected (path)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str(), _ => return Err(RockError::runtime("path.ext: expected string")) };
            let p = std::path::Path::new(s);
            Ok(Value::Str(Rc::new(p.extension().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())))
        });
        let path_is_abs: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("path.is_abs: expected (path)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str(), _ => return Err(RockError::runtime("path.is_abs: expected string")) };
            Ok(Value::Bool(std::path::Path::new(s).is_absolute()))
        });
        let path_exists: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("path.exists: expected (path)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str(), _ => return Err(RockError::runtime("path.exists: expected string")) };
            Ok(Value::Bool(std::path::Path::new(s).exists()))
        });
        let path_is_file: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("path.is_file: expected (path)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str(), _ => return Err(RockError::runtime("path.is_file: expected string")) };
            Ok(Value::Bool(std::path::Path::new(s).is_file()))
        });
        let path_is_dir: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("path.is_dir: expected (path)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str(), _ => return Err(RockError::runtime("path.is_dir: expected string")) };
            Ok(Value::Bool(std::path::Path::new(s).is_dir()))
        });
        let path_absolute: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("path.absolute: expected (path)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str(), _ => return Err(RockError::runtime("path.absolute: expected string")) };
            let p = std::path::Path::new(s);
            let abs = if p.is_absolute() { p.to_path_buf() } else {
                std::env::current_dir().map_err(|e| RockError::runtime(format!("path.absolute: {}", e)))?.join(p)
            };
            Ok(Value::Str(Rc::new(abs.to_string_lossy().into_owned())))
        });
        let path_normalize: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("path.normalize: expected (path)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str(), _ => return Err(RockError::runtime("path.normalize: expected string")) };
            let p = std::path::Path::new(s);
            let mut out = std::path::PathBuf::new();
            for comp in p.components() {
                use std::path::Component::*;
                match comp {
                    CurDir => {},
                    ParentDir => { out.pop(); },
                    other => out.push(other.as_os_str()),
                }
            }
            let result = out.to_string_lossy().into_owned();
            Ok(Value::Str(Rc::new(if result.is_empty() { ".".to_string() } else { result })))
        });
        let path_split: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("path.split: expected (path)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str(), _ => return Err(RockError::runtime("path.split: expected string")) };
            let parts: Vec<Value> = std::path::Path::new(s).components()
                .map(|c| Value::Str(Rc::new(c.as_os_str().to_string_lossy().into_owned())))
                .collect();
            Ok(Value::Array(Rc::new(RefCell::new(parts))))
        });
        let path_mod = mk_map(vec![
            ("join", Value::Native(path_join)),
            ("dirname", Value::Native(path_dirname)),
            ("basename", Value::Native(path_basename)),
            ("stem", Value::Native(path_stem)),
            ("ext", Value::Native(path_ext)),
            ("is_abs", Value::Native(path_is_abs)),
            ("exists", Value::Native(path_exists)),
            ("is_file", Value::Native(path_is_file)),
            ("is_dir", Value::Native(path_is_dir)),
            ("absolute", Value::Native(path_absolute)),
            ("normalize", Value::Native(path_normalize)),
            ("split", Value::Native(path_split)),
        ]);
        env.borrow_mut().define("path", path_mod, false);

        // ---- base64 module (convenience aliases over crypto) ----
        let b64_e2: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("base64.encode: expected (data)")); }
            let bytes: Vec<u8> = match &args[0] { Value::Str(s) => s.as_bytes().to_vec(), other => other.to_string().into_bytes() };
            Ok(Value::Str(Rc::new(base64_encode_bytes(&bytes))))
        });
        let b64_d2: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("base64.decode: expected (s)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str().to_string(), _ => return Err(RockError::runtime("base64.decode: expected string")) };
            let out = base64_decode_str(&s).map_err(|e| RockError::runtime(format!("base64.decode: {}", e)))?;
            Ok(Value::Str(Rc::new(String::from_utf8_lossy(&out).into_owned())))
        });
        let base64_mod = mk_map(vec![
            ("encode", Value::Native(b64_e2)),
            ("decode", Value::Native(b64_d2)),
        ]);
        env.borrow_mut().define("base64", base64_mod, false);

        // ---- hex module ----
        let hex_e: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("hex.encode: expected (data)")); }
            let bytes: Vec<u8> = match &args[0] { Value::Str(s) => s.as_bytes().to_vec(), Value::Int(i) => format!("{:x}", *i as u64).into_bytes(), other => other.to_string().into_bytes() };
            let mut out = String::with_capacity(bytes.len() * 2);
            for b in &bytes { out.push_str(&format!("{:02x}", b)); }
            Ok(Value::Str(Rc::new(out)))
        });
        let hex_d: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|args: &[Value]| {
            if args.len() != 1 { return Err(RockError::runtime("hex.decode: expected (s)")); }
            let s = match &args[0] { Value::Str(s) => s.as_str().to_string(), _ => return Err(RockError::runtime("hex.decode: expected string")) };
            let s = s.trim();
            if s.len() % 2 != 0 { return Err(RockError::runtime("hex.decode: odd-length input")); }
            let mut out = Vec::with_capacity(s.len() / 2);
            let bytes = s.as_bytes();
            let hv = |c: u8| -> std::result::Result<u8, String> {
                match c { b'0'..=b'9' => Ok(c - b'0'), b'a'..=b'f' => Ok(10 + c - b'a'), b'A'..=b'F' => Ok(10 + c - b'A'), _ => Err(format!("invalid hex char '{}'", c as char)) }
            };
            let mut i = 0;
            while i < bytes.len() {
                let hi = hv(bytes[i]).map_err(RockError::runtime)?;
                let lo = hv(bytes[i+1]).map_err(RockError::runtime)?;
                out.push((hi << 4) | lo);
                i += 2;
            }
            Ok(Value::Str(Rc::new(String::from_utf8_lossy(&out).into_owned())))
        });
        let hex_mod = mk_map(vec![
            ("encode", Value::Native(hex_e)),
            ("decode", Value::Native(hex_d)),
        ]);
        env.borrow_mut().define("hex", hex_mod, false);

        // ---- uuid module ----
        let uuid_v4: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|_args: &[Value]| {
            Ok(Value::Str(Rc::new(uuid_v4_string())))
        });
        let uuid_v7: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(|_args: &[Value]| {
            Ok(Value::Str(Rc::new(uuid_v7_string())))
        });
        let uuid_mod = mk_map(vec![
            ("v4", Value::Native(uuid_v4)),
            ("v7", Value::Native(uuid_v7)),
        ]);
        env.borrow_mut().define("uuid", uuid_mod, false);
    }

    pub fn run(&mut self, program: &Program) -> Result<Value> {
        self.run_with_base(program, std::env::current_dir().ok())
    }

    pub fn load_only(&mut self, program: &Program) -> Result<()> {
        let prev = self.skip_main.replace(true);
        let res = self.run_with_base(program, std::env::current_dir().ok());
        self.skip_main.replace(prev);
        res.map(|_| ())
    }

    pub fn invoke_global(&mut self, name: &str, args: Vec<Value>) -> Result<Value> {
        let v = self.globals.borrow().get(name).ok_or_else(|| {
            RockError::runtime(format!("function '{}' not defined", name))
        })?;
        match v {
            Value::Function(c) => self.call_closure(&c, args),
            Value::Overloads(fs) => {
                let chosen = self.pick_overload(&fs, &args).map_err(|f| match f {
                    Flow::Err(e) => e,
                    _ => RockError::runtime("overload selection failed"),
                })?;
                self.call_closure(&chosen, args)
            }
            _ => Err(RockError::runtime(format!("'{}' is not a function", name))),
        }
    }

    pub fn list_global_fns(&self) -> Vec<String> {
        self.globals.borrow().vars.keys().filter(|k| {
            let v = self.globals.borrow().vars.get(k.as_str()).map(|x| x.0.clone());
            matches!(v, Some(Value::Function(_)) | Some(Value::Overloads(_)))
        }).cloned().collect()
    }

    /// Evaluate a REPL chunk: install any items (fn/type/const/etc) into globals,
    /// then run any top-level statements, returning the value of the LAST top-level
    /// expression statement (for printing). main() is NOT auto-invoked.
    pub fn eval_repl(&mut self, program: &Program) -> Result<Option<Value>> {
        let prev = self.skip_main.replace(true);
        // Install items (functions, types, enums, consts, traits, impls, imports, state machines).
        // We re-use run_with_base for that; but it would also try to run main — skip_main blocks that.
        // Filter out top-level statements; we want to run them ourselves so we can capture the
        // last expression value.
        let items_only = Program {
            items: program.items.iter().filter(|it| !matches!(it, Item::Stmt(_))).cloned().collect(),
        };
        let res = self.run_with_base(&items_only, std::env::current_dir().ok());
        self.skip_main.replace(prev);
        res?;

        // Now run the statements in order, capturing the last expression value.
        let env = self.globals.clone();
        let mut last_value: Option<Value> = None;
        for it in &program.items {
            if let Item::Stmt(stmt) = it {
                let was_expr = matches!(stmt, Stmt::Expr(_));
                match self.exec_stmt(stmt, &env) {
                    Ok(v) => {
                        if was_expr { last_value = Some(v); } else { last_value = None; }
                    }
                    Err(Flow::Err(e)) => return Err(e),
                    Err(Flow::Return(_)) => return Err(RockError::runtime("'return' outside function")),
                    Err(Flow::Break) => return Err(RockError::runtime("'break' outside loop")),
                    Err(Flow::Continue) => return Err(RockError::runtime("'continue' outside loop")),
                }
            }
        }
        Ok(last_value)
    }

    pub fn run_prove_only(&mut self, program: &Program) -> Result<()> {
        for item in &program.items {
            if let Item::StateMachine(sm) = item {
                self.state_machines.insert(sm.name.clone(), sm.clone());
            }
        }
        for item in &program.items {
            if let Item::Prove(b) = item {
                self.verify_prove_block(b)?;
            }
        }
        Ok(())
    }

    pub fn run_with_base(&mut self, program: &Program, base_dir: Option<std::path::PathBuf>) -> Result<Value> {
        for item in &program.items {
            match item {
                Item::Import { path, alias, .. } => {
                    // Detect name collisions between module alias and pre-existing
                    // function/overload of the same name in the current scope.
                    if let Some(alias_name) = alias {
                        if let Some(existing) = self.globals.borrow().get(alias_name) {
                            match &existing {
                                Value::Function(_) | Value::Overloads(_) => {
                                    return Err(RockError::runtime(format!(
                                        "import alias '{}' collides with existing function '{}' — rename the module alias or the function",
                                        alias_name, alias_name
                                    )));
                                }
                                _ => {}
                            }
                        }
                    }
                    let pb = resolve_import_path(path, base_dir.as_deref())
                        .map_err(|e| RockError::runtime(format!("import '{}': {}", path, e)))?;
                    let canonical = std::fs::canonicalize(&pb).unwrap_or_else(|_| pb.clone());
                    if self.loading_imports.borrow().iter().any(|p| p == &canonical) {
                        let chain: Vec<String> = self.loading_imports.borrow().iter()
                            .map(|p| p.file_name().and_then(|s| s.to_str()).unwrap_or("?").to_string())
                            .chain(std::iter::once(
                                canonical.file_name().and_then(|s| s.to_str()).unwrap_or("?").to_string()
                            ))
                            .collect();
                        return Err(RockError::runtime(format!(
                            "circular import detected: {}", chain.join(" -> ")
                        )));
                    }
                    self.loading_imports.borrow_mut().push(canonical.clone());
                    let src = std::fs::read_to_string(&pb).map_err(|e| {
                        RockError::runtime(format!("import '{}': {}", path, e))
                    })?;
                    let toks = crate::lexer::Lexer::new(&src).tokenize()?;
                    let sub = crate::parser::Parser::new(toks).parse_program()?;
                    let parent = pb.parent().map(|p| p.to_path_buf());
                    let before_vals: std::collections::HashMap<String, Value> = {
                        let g = self.globals.borrow();
                        g.names().into_iter()
                            .filter_map(|k| g.get(&k).map(|v| (k, v)))
                            .collect()
                    };
                    let prev_skip = *self.skip_main.borrow();
                    *self.skip_main.borrow_mut() = true;
                    let res = self.run_with_base(&sub, parent);
                    *self.skip_main.borrow_mut() = prev_skip;
                    res?;
                    if let Some(alias_name) = alias {
                        // `pub` visibility: collect the set of names the submodule
                        // explicitly marked public. If the submodule uses `pub` at
                        // all, only those names are exposed to the importer.
                        // If it uses no `pub` at all, every top-level symbol is
                        // exposed (backward compat with pre-pub code).
                        let mut pub_names: std::collections::HashSet<String> =
                            std::collections::HashSet::new();
                        let mut any_pub = false;
                        for it in &sub.items {
                            match it {
                                Item::Function(f) if f.is_pub => {
                                    any_pub = true;
                                    pub_names.insert(f.name.clone());
                                }
                                Item::Import { is_pub: true, alias: Some(a), .. } => {
                                    any_pub = true;
                                    pub_names.insert(a.clone());
                                }
                                Item::TypeDecl(td) if td.is_pub => {
                                    any_pub = true;
                                    pub_names.insert(td.name.clone());
                                }
                                Item::EnumDecl(ed) if ed.is_pub => {
                                    any_pub = true;
                                    pub_names.insert(ed.name.clone());
                                }
                                Item::Const { is_pub: true, name, .. } => {
                                    any_pub = true;
                                    pub_names.insert(name.clone());
                                }
                                _ => {}
                            }
                        }
                        // A module's alias Map should contain every top-level symbol the
                        // module defined, even symbols whose names collide with pre-existing
                        // builtins (e.g. a module defining `fn url_decode()` when `url.decode`
                        // is a builtin). We detect this by comparing the VALUE of each name
                        // before and after the sub-program runs, not just the set of names.
                        let new_keys: Vec<String> = {
                            let g = self.globals.borrow();
                            g.names().into_iter().filter(|k| {
                                match (before_vals.get(k), g.get(k)) {
                                    (None, Some(_)) => true,
                                    (Some(a), Some(b)) => !values_shallow_eq(a, &b),
                                    _ => false,
                                }
                            }).collect()
                        };
                        let exposed_keys: Vec<String> = if any_pub {
                            new_keys.iter().filter(|k| pub_names.contains(*k)).cloned().collect()
                        } else {
                            new_keys.clone()
                        };
                        let mut entries: Vec<(Value, Value)> = Vec::with_capacity(exposed_keys.len());
                        let mut to_undefine: Vec<String> = Vec::new();
                        for k in &new_keys {
                            if let Some(v) = self.globals.borrow().get(k) {
                                let is_submodule_alias = matches!(v, Value::Map(_));
                                if exposed_keys.contains(k) {
                                    entries.push((Value::Str(Rc::new(k.clone())), v));
                                }
                                if !is_submodule_alias {
                                    to_undefine.push(k.clone());
                                }
                            }
                        }
                        for k in &to_undefine {
                            self.globals.borrow_mut().undefine(k);
                        }
                        // Restore any builtin values that the module temporarily shadowed
                        // (so subsequent code in the outer scope still sees e.g. `url.decode`).
                        for (k, v) in &before_vals {
                            if to_undefine.contains(k) {
                                self.globals.borrow_mut().define(k, v.clone(), false);
                            }
                        }
                        self.globals.borrow_mut().define(
                            alias_name,
                            Value::Map(Rc::new(RefCell::new(entries))),
                            false,
                        );
                    }
                    // Pop this path off the loading stack; mark as fully loaded.
                    self.loading_imports.borrow_mut().pop();
                    self.loaded_imports.borrow_mut().insert(canonical);
                }
                Item::Function(f) => {
                    let closure = Rc::new(Closure { func: Rc::new(f.clone()), captured: None });
                    let existing = self.globals.borrow().get(&f.name);
                    match existing {
                        Some(Value::Function(prev)) => {
                            let overloads = Rc::new(vec![prev, closure]);
                            self.globals.borrow_mut().define(&f.name, Value::Overloads(overloads), false);
                        }
                        Some(Value::Overloads(prev)) => {
                            let mut v: Vec<Rc<Closure>> = prev.as_ref().clone();
                            v.push(closure);
                            self.globals.borrow_mut().define(&f.name, Value::Overloads(Rc::new(v)), false);
                        }
                        _ => {
                            self.globals.borrow_mut().define(&f.name, Value::Function(closure), false);
                        }
                    }
                }
                Item::TypeDecl(td) => {
                    self.type_decls.insert(td.name.clone(), td.clone());
                    self.globals.borrow_mut().define(
                        &td.name,
                        Value::TypeRef(Rc::new(td.name.clone())),
                        false,
                    );
                }
                Item::EnumDecl(ed) => {
                    let mut variant_map: Vec<(Value, Value)> = Vec::new();
                    for v in &ed.variants {
                        self.variant_info.insert(v.name.clone(), v.kind.clone());
                        let cv: Value = match &v.kind {
                            VariantKind::Nullary => {
                                Value::Struct(Rc::new(RefCell::new(Struct {
                                    type_name: v.name.clone(),
                                    fields: Vec::new(),
                                })))
                            }
                            VariantKind::Tuple(n) => {
                                let vn = v.name.clone();
                                let n = *n;
                                let f: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
                                    if args.len() != n {
                                        return Err(RockError::runtime(format!(
                                            "variant '{}' expects {} args, got {}", vn, n, args.len()
                                        )));
                                    }
                                    let mut fields = Vec::with_capacity(args.len());
                                    for (i, a) in args.iter().enumerate() {
                                        fields.push((format!("_{}", i), a.clone()));
                                    }
                                    Ok(Value::Struct(Rc::new(RefCell::new(Struct {
                                        type_name: vn.clone(),
                                        fields,
                                    }))))
                                });
                                Value::Native(f)
                            }
                            VariantKind::Named(fnames) => {
                                let vn = v.name.clone();
                                let fnames = fnames.clone();
                                let f: Rc<dyn Fn(&[Value]) -> Result<Value>> = Rc::new(move |args: &[Value]| {
                                    if args.len() != fnames.len() {
                                        return Err(RockError::runtime(format!(
                                            "variant '{}' expects {} args, got {}", vn, fnames.len(), args.len()
                                        )));
                                    }
                                    let mut fields = Vec::with_capacity(args.len());
                                    for (name, a) in fnames.iter().zip(args.iter()) {
                                        fields.push((name.clone(), a.clone()));
                                    }
                                    Ok(Value::Struct(Rc::new(RefCell::new(Struct {
                                        type_name: vn.clone(),
                                        fields,
                                    }))))
                                });
                                Value::Native(f)
                            }
                        };
                        self.globals.borrow_mut().define(&v.name, cv.clone(), false);
                        variant_map.push((Value::Str(Rc::new(v.name.clone())), cv));
                    }
                    self.globals.borrow_mut().define(
                        &ed.name,
                        Value::Map(Rc::new(RefCell::new(variant_map))),
                        false,
                    );
                }
                Item::Impl(blk) => {
                    let entry = self.impls.entry(blk.target.clone()).or_default();
                    for m in &blk.methods {
                        entry.insert(m.name.clone(), Rc::new(m.clone()));
                    }
                }
                Item::Const { name, value, .. } => {
                    let v = match self.eval(value, &self.globals.clone()) {
                        Ok(v) => v,
                        Err(Flow::Err(e)) => return Err(e),
                        Err(_) => return Err(RockError::runtime("invalid const initializer")),
                    };
                    self.globals.borrow_mut().define(name, v, false);
                }
                Item::StateMachine(sm) => {
                    let mut fields = Vec::new();
                    for st in &sm.states {
                        fields.push((st.clone(), Value::Str(Rc::new(format!("{}.{}", sm.name, st)))));
                    }
                    let sm_struct = Value::Struct(Rc::new(RefCell::new(Struct {
                        type_name: sm.name.clone(),
                        fields,
                    })));
                    self.globals.borrow_mut().define(&sm.name, sm_struct, false);
                    self.state_machines.insert(sm.name.clone(), sm.clone());
                }
                Item::Prove(block) => {
                    self.verify_prove_block(block)?;
                }
                Item::Trait(td) => {
                    self.traits.insert(td.name.clone(), td.clone());
                }
                Item::TraitImpl(ti) => {
                    let trait_decl = self.traits.get(&ti.trait_name).cloned().ok_or_else(|| {
                        RockError::runtime(format!("unknown trait '{}'", ti.trait_name))
                    })?;
                    let mut provided: HashMap<String, Rc<Function>> = HashMap::new();
                    for m in &ti.methods {
                        provided.insert(m.name.clone(), Rc::new(m.clone()));
                    }
                    for tm in &trait_decl.methods {
                        if !provided.contains_key(&tm.name) {
                            if let Some(default_fn) = &tm.default {
                                provided.insert(tm.name.clone(), Rc::new(default_fn.clone()));
                            } else {
                                return Err(RockError::runtime(format!(
                                    "impl {} for {}: missing method '{}'",
                                    ti.trait_name, ti.target, tm.name
                                )));
                            }
                        }
                    }
                    let entry = self.impls.entry(ti.target.clone()).or_default();
                    for (n, f) in provided {
                        entry.insert(n, f);
                    }
                    self.trait_impls
                        .entry(ti.trait_name.clone())
                        .or_default()
                        .push(ti.target.clone());
                }
                _ => {}
            }
        }

        let mut last = Value::Nil;
        for item in &program.items {
            if let Item::Stmt(s) = item {
                last = match self.exec_stmt(s, &self.globals.clone()) {
                    Ok(v) => v,
                    Err(Flow::Err(e)) => return Err(e),
                    Err(Flow::Return(v)) => return Ok(v),
                    Err(Flow::Break) | Err(Flow::Continue) => {
                        return Err(RockError::runtime("break/continue outside loop"));
                    }
                };
            }
        }

        let main_fn = self.globals.borrow().get("main");
        if !*self.skip_main.borrow() {
            if let Some(Value::Function(main)) = main_fn {
                let result = self.call_closure(&main, vec![])?;
                self.drain_task_queue();
                return Ok(result);
            }
        }
        self.drain_task_queue();
        Ok(last)
    }

    fn exec_block(&mut self, block: &Block, parent: &Rc<RefCell<Env>>) -> FlowResult<Value> {
        let env = Env::with_parent(parent.clone());
        let mut last = Value::Nil;
        let mut deferred: Vec<Block> = Vec::new();
        let n = block.stmts.len();
        let mut outcome: FlowResult<()> = Ok(());
        for (i, stmt) in block.stmts.iter().enumerate() {
            if let Stmt::Defer { body, .. } = stmt {
                deferred.push(body.clone());
                continue;
            }
            match self.exec_stmt(stmt, &env) {
                Ok(v) => { if i == n - 1 { last = v; } }
                Err(flow) => { outcome = Err(flow); break; }
            }
        }
        for body in deferred.into_iter().rev() {
            let _ = self.exec_block(&body, &env);
        }
        outcome?;
        Ok(last)
    }

    fn exec_stmt(&mut self, stmt: &Stmt, env: &Rc<RefCell<Env>>) -> FlowResult<Value> {
        match stmt {
            Stmt::Let { name, mutable, value, .. } => {
                let v = self.eval(value, env)?;
                env.borrow_mut().define(name, v, *mutable);
                Ok(Value::Nil)
            }
            Stmt::LetPattern { pattern, value, .. } => {
                let v = self.eval(value, env)?;
                let matched = self.pattern_match(pattern, &v, env)?;
                if !matched {
                    return Err(Flow::Err(RockError::runtime(
                        "destructuring let: value did not match pattern",
                    )));
                }
                Ok(Value::Nil)
            }
            Stmt::Assign { target, op, value, .. } => {
                let new_val = self.eval(value, env)?;
                match target {
                    Expr::Ident(name, _) => {
                        let final_val = match op {
                            AssignOp::Set => new_val,
                            AssignOp::Add => {
                                let cur = env.borrow().get(name).ok_or_else(|| {
                                    RockError::runtime(format!("undefined '{}'", name))
                                })?;
                                numeric_binop(&cur, &new_val, BinOp::Add)?
                            }
                            AssignOp::Sub => {
                                let cur = env.borrow().get(name).ok_or_else(|| {
                                    RockError::runtime(format!("undefined '{}'", name))
                                })?;
                                numeric_binop(&cur, &new_val, BinOp::Sub)?
                            }
                            AssignOp::Mul => {
                                let cur = env.borrow().get(name).ok_or_else(|| {
                                    RockError::runtime(format!("undefined '{}'", name))
                                })?;
                                numeric_binop(&cur, &new_val, BinOp::Mul)?
                            }
                            AssignOp::Div => {
                                let cur = env.borrow().get(name).ok_or_else(|| {
                                    RockError::runtime(format!("undefined '{}'", name))
                                })?;
                                numeric_binop(&cur, &new_val, BinOp::Div)?
                            }
                        };
                        env.borrow_mut().set(name, final_val)?;
                        Ok(Value::Nil)
                    }
                    Expr::Index { base, idx, .. } => {
                        let base_v = self.eval(base, env)?;
                        let idx_v = self.eval(idx, env)?;
                        match base_v {
                            Value::Array(arr) => {
                                let i = to_usize(&idx_v)?;
                                let mut a = arr.borrow_mut();
                                if i >= a.len() {
                                    return Err(Flow::Err(RockError::runtime("index out of range")));
                                }
                                let final_val = match op {
                                    AssignOp::Set => new_val,
                                    AssignOp::Add => numeric_binop(&a[i], &new_val, BinOp::Add)?,
                                    AssignOp::Sub => numeric_binop(&a[i], &new_val, BinOp::Sub)?,
                                    AssignOp::Mul => numeric_binop(&a[i], &new_val, BinOp::Mul)?,
                                    AssignOp::Div => numeric_binop(&a[i], &new_val, BinOp::Div)?,
                                };
                                a[i] = final_val;
                                Ok(Value::Nil)
                            }
                            Value::Map(m) => {
                                if !matches!(op, AssignOp::Set) {
                                    return Err(Flow::Err(RockError::runtime("compound assign not supported on maps")));
                                }
                                let mut m = m.borrow_mut();
                                for entry in m.iter_mut() {
                                    if entry.0 == idx_v { entry.1 = new_val; return Ok(Value::Nil); }
                                }
                                m.push((idx_v, new_val));
                                Ok(Value::Nil)
                            }
                            _ => Err(Flow::Err(RockError::runtime("indexed assign on non-collection"))),
                        }
                    }
                    Expr::Field { base, name, .. } => {
                        let base_v = self.eval(base, env)?;
                        match base_v {
                            Value::Struct(s) => {
                                let mut s = s.borrow_mut();
                                for entry in s.fields.iter_mut() {
                                    if entry.0 == *name {
                                        let final_val = match op {
                                            AssignOp::Set => new_val,
                                            AssignOp::Add => numeric_binop(&entry.1, &new_val, BinOp::Add)?,
                                            AssignOp::Sub => numeric_binop(&entry.1, &new_val, BinOp::Sub)?,
                                            AssignOp::Mul => numeric_binop(&entry.1, &new_val, BinOp::Mul)?,
                                            AssignOp::Div => numeric_binop(&entry.1, &new_val, BinOp::Div)?,
                                        };
                                        entry.1 = final_val;
                                        return Ok(Value::Nil);
                                    }
                                }
                                Err(Flow::Err(RockError::runtime(format!(
                                    "no field '{}' on '{}'", name, s.type_name
                                ))))
                            }
                            Value::Map(m) => {
                                if !matches!(op, AssignOp::Set) {
                                    return Err(Flow::Err(RockError::runtime("compound assign not supported on map field")));
                                }
                                let key = Value::Str(Rc::new(name.clone()));
                                let mut m = m.borrow_mut();
                                for entry in m.iter_mut() {
                                    if entry.0 == key { entry.1 = new_val; return Ok(Value::Nil); }
                                }
                                m.push((key, new_val));
                                Ok(Value::Nil)
                            }
                            other => Err(Flow::Err(RockError::runtime(format!(
                                "cannot assign field on {}", other.type_name()
                            )))),
                        }
                    }
                    _ => Err(Flow::Err(RockError::runtime("invalid assignment target"))),
                }
            }
            Stmt::Expr(e) => self.eval(e, env),
            Stmt::Return(e, _) => {
                let v = if let Some(e) = e { self.eval(e, env)? } else { Value::Nil };
                Err(Flow::Return(v))
            }
            Stmt::Break(_) => Err(Flow::Break),
            Stmt::Continue(_) => Err(Flow::Continue),
            Stmt::While { cond, body, .. } => {
                loop {
                    let c = self.eval(cond, env)?;
                    if !c.is_truthy() { break; }
                    match self.exec_block(body, env) {
                        Ok(_) => {}
                        Err(Flow::Break) => break,
                        Err(Flow::Continue) => continue,
                        Err(other) => return Err(other),
                    }
                }
                Ok(Value::Nil)
            }
            Stmt::Loop { body, .. } => {
                loop {
                    match self.exec_block(body, env) {
                        Ok(_) => {}
                        Err(Flow::Break) => break,
                        Err(Flow::Continue) => continue,
                        Err(other) => return Err(other),
                    }
                }
                Ok(Value::Nil)
            }
            Stmt::For { var, iter, body, .. } => {
                let iv = self.eval(iter, env)?;
                match iv {
                    Value::Range(a, b) => {
                        let mut i = a;
                        while i < b {
                            let inner = Env::with_parent(env.clone());
                            inner.borrow_mut().define(var, Value::Int(i), true);
                            match self.exec_block(body, &inner) {
                                Ok(_) => {}
                                Err(Flow::Break) => break,
                                Err(Flow::Continue) => {}
                                Err(other) => return Err(other),
                            }
                            i += 1;
                        }
                    }
                    Value::Array(arr) => {
                        let snapshot: Vec<Value> = arr.borrow().clone();
                        for item in snapshot {
                            let inner = Env::with_parent(env.clone());
                            inner.borrow_mut().define(var, item, true);
                            match self.exec_block(body, &inner) {
                                Ok(_) => {}
                                Err(Flow::Break) => break,
                                Err(Flow::Continue) => {}
                                Err(other) => return Err(other),
                            }
                        }
                    }
                    Value::Map(m) => {
                        let snapshot: Vec<(Value, Value)> = m.borrow().clone();
                        for (k, v) in snapshot {
                            let inner = Env::with_parent(env.clone());
                            let pair = vec![k, v];
                            inner.borrow_mut().define(var, Value::Array(Rc::new(RefCell::new(pair))), true);
                            match self.exec_block(body, &inner) {
                                Ok(_) => {}
                                Err(Flow::Break) => break,
                                Err(Flow::Continue) => {}
                                Err(other) => return Err(other),
                            }
                        }
                    }
                    Value::Str(s) => {
                        let chars: Vec<Value> = s.chars()
                            .map(|c| Value::Str(Rc::new(c.to_string())))
                            .collect();
                        for item in chars {
                            let inner = Env::with_parent(env.clone());
                            inner.borrow_mut().define(var, item, true);
                            match self.exec_block(body, &inner) {
                                Ok(_) => {}
                                Err(Flow::Break) => break,
                                Err(Flow::Continue) => {}
                                Err(other) => return Err(other),
                            }
                        }
                    }
                    other => return Err(Flow::Err(RockError::runtime(
                        format!("cannot iterate over {}", other.type_name())
                    ))),
                }
                Ok(Value::Nil)
            }
            Stmt::Defer { .. } => Ok(Value::Nil),
            Stmt::With { ctx, body, .. } => {
                let ctx_val = self.eval(ctx, env)?;
                let inner = Env::with_parent(env.clone());
                inner.borrow_mut().define("__ctx__", ctx_val, false);
                self.exec_block(body, &inner)
            }
            Stmt::Reactive { name, expr, .. } => {
                self.reactive.borrow_mut().insert(name.clone(), expr.clone());
                let v = self.eval(expr, env)?;
                if env.borrow().get(name).is_some() {
                    env.borrow_mut().set(name, v).map_err(Flow::Err)?;
                } else {
                    env.borrow_mut().define(name, v, true);
                }
                Ok(Value::Nil)
            }
            Stmt::TryCatch { try_body, err_name, catch_body, .. } => {
                let try_env = Env::with_parent(env.clone());
                match self.exec_block(try_body, &try_env) {
                    Ok(v) => Ok(v),
                    Err(Flow::Err(e)) => {
                        let catch_env = Env::with_parent(env.clone());
                        catch_env.borrow_mut().define(err_name, Value::Str(Rc::new(e.message.clone())), false);
                        self.exec_block(catch_body, &catch_env)
                    }
                    Err(other) => Err(other),
                }
            }
        }
    }

    fn eval(&mut self, expr: &Expr, env: &Rc<RefCell<Env>>) -> FlowResult<Value> {
        match expr {
            Expr::Int(n, _) => Ok(Value::Int(*n)),
            Expr::Float(f, _) => Ok(Value::Float(*f)),
            Expr::Str(s, _) => Ok(Value::Str(Rc::new(s.clone()))),
            Expr::Bool(b, _) => Ok(Value::Bool(*b)),
            Expr::Nil(_) => Ok(Value::Nil),
            Expr::Ident(name, _) => {
                let reactive_expr = {
                    if *self.in_reactive.borrow() { None }
                    else { self.reactive.borrow().get(name).cloned() }
                };
                if let Some(re) = reactive_expr {
                    *self.in_reactive.borrow_mut() = true;
                    let r = self.eval(&re, env);
                    *self.in_reactive.borrow_mut() = false;
                    return r;
                }
                env.borrow().get(name).ok_or_else(|| {
                    Flow::Err(RockError::runtime(format!("undefined '{}'", name)))
                })
            }
            Expr::SelfExpr(_) => env.borrow().get("self").ok_or_else(|| {
                Flow::Err(RockError::runtime("'self' not in scope"))
            }),
            Expr::Path { segments, .. } => {
                if segments.len() != 2 {
                    return Err(Flow::Err(RockError::runtime("unsupported path")));
                }
                Err(Flow::Err(RockError::runtime(format!(
                    "path '{}' unsupported; use {}.{}", segments.join("::"), segments[0], segments[1]
                ))))
            }
            Expr::StructLit { name, fields, span } => {
                self.check_no_alloc("struct literal")?;
                if let Some(VariantKind::Named(ref fnames)) = self.variant_info.get(name).cloned() {
                    let _ = span;
                    let mut out = Vec::with_capacity(fnames.len());
                    for f in fnames {
                        let provided = fields.iter().find(|(n, _)| n == f);
                        let v = match provided {
                            Some((_, e)) => self.eval(e, env)?,
                            None => return Err(Flow::Err(RockError::runtime(format!(
                                "missing field '{}' in variant '{}'", f, name
                            )))),
                        };
                        out.push((f.clone(), v));
                    }
                    for (fname, _) in fields {
                        if !fnames.iter().any(|n| n == fname) {
                            return Err(Flow::Err(RockError::runtime(format!(
                                "variant '{}' has no field '{}'", name, fname
                            ))));
                        }
                    }
                    return Ok(Value::Struct(Rc::new(RefCell::new(Struct {
                        type_name: name.clone(),
                        fields: out,
                    }))));
                }
                let decl = self.type_decls.get(name).cloned().ok_or_else(|| {
                    Flow::Err(RockError::runtime(format!("unknown type '{}'", name)))
                })?;
                let _ = span;
                let mut out = Vec::with_capacity(decl.fields.len());
                for f in &decl.fields {
                    let provided = fields.iter().find(|(n, _)| n == &f.name);
                    let v = match provided {
                        Some((_, e)) => self.eval(e, env)?,
                        None => return Err(Flow::Err(RockError::runtime(format!(
                            "missing field '{}' in '{}' literal", f.name, name
                        )))),
                    };
                    out.push((f.name.clone(), v));
                }
                for (fname, _) in fields {
                    if !decl.fields.iter().any(|f| &f.name == fname) {
                        return Err(Flow::Err(RockError::runtime(format!(
                            "type '{}' has no field '{}'", name, fname
                        ))));
                    }
                }
                Ok(Value::Struct(Rc::new(RefCell::new(Struct {
                    type_name: name.clone(),
                    fields: out,
                }))))
            }
            Expr::MethodCall { receiver, method, args, .. } => {
                let recv = self.eval(receiver, env)?;
                let mut evaluated = Vec::with_capacity(args.len());
                for a in args { evaluated.push(self.eval(a, env)?); }
                self.dispatch_method(recv, method, evaluated)
            }
            Expr::Array(items, _) => {
                self.check_no_alloc("array literal")?;
                let mut out = Vec::with_capacity(items.len());
                for it in items { out.push(self.eval(it, env)?); }
                Ok(Value::Array(Rc::new(RefCell::new(out))))
            }
            Expr::Block(b) => self.exec_block(b, env),
            Expr::If { cond, then, else_branch, .. } => {
                let c = self.eval(cond, env)?;
                if c.is_truthy() {
                    self.exec_block(then, env)
                } else if let Some(eb) = else_branch {
                    self.eval(eb, env)
                } else {
                    Ok(Value::Nil)
                }
            }
            Expr::Unary { op, rhs, .. } => {
                let v = self.eval(rhs, env)?;
                match op {
                    UnaryOp::Neg => match v {
                        Value::Int(i) => Ok(Value::Int(-i)),
                        Value::Float(f) => Ok(Value::Float(-f)),
                        other => Err(Flow::Err(RockError::type_err(
                            format!("cannot negate {}", other.type_name())
                        ))),
                    },
                    UnaryOp::Not => Ok(Value::Bool(!v.is_truthy())),
                }
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                if matches!(op, BinOp::And) {
                    let l = self.eval(lhs, env)?;
                    if !l.is_truthy() { return Ok(l); }
                    return self.eval(rhs, env);
                }
                if matches!(op, BinOp::Or) {
                    let l = self.eval(lhs, env)?;
                    if l.is_truthy() { return Ok(l); }
                    return self.eval(rhs, env);
                }
                let l = self.eval(lhs, env)?;
                let r = self.eval(rhs, env)?;
                Ok(eval_binop(&l, &r, *op)?)
            }
            Expr::Call { callee, args, .. } => {
                if let Expr::Ident(name, _) = callee.as_ref() {
                    match name.as_str() {
                        "__grad" => return self.eval_grad(args, env),
                        "__trace" => return self.eval_trace(args, env),
                        "__reflect" => return self.eval_reflect(args, env),
                        _ => {}
                    }
                }
                let callee_v = self.eval(callee, env)?;
                let mut evaluated = Vec::with_capacity(args.len());
                for a in args { evaluated.push(self.eval(a, env)?); }
                self.call(&callee_v, evaluated)
            }
            Expr::Index { base, idx, .. } => {
                let b = self.eval(base, env)?;
                let i = self.eval(idx, env)?;
                match (b, i) {
                    (Value::Array(a), idx) => {
                        let ix = to_usize(&idx).map_err(Flow::Err)?;
                        let a = a.borrow();
                        a.get(ix).cloned().ok_or_else(|| Flow::Err(
                            RockError::runtime(format!("index {} out of bounds", ix))
                        ))
                    }
                    (Value::Map(m), key) => {
                        let m = m.borrow();
                        for (k, v) in m.iter() {
                            if k == &key { return Ok(v.clone()); }
                        }
                        Ok(Value::Nil)
                    }
                    (Value::Str(s), idx) => {
                        let ix = to_usize(&idx).map_err(Flow::Err)?;
                        s.chars().nth(ix)
                            .map(|c| Value::Str(Rc::new(c.to_string())))
                            .ok_or_else(|| Flow::Err(
                                RockError::runtime(format!("string index {} out of bounds", ix))
                            ))
                    }
                    (other, _) => Err(Flow::Err(RockError::type_err(
                        format!("cannot index {}", other.type_name())
                    ))),
                }
            }
            Expr::Field { base, name, .. } => {
                let b = self.eval(base, env)?;
                match (&b, name.as_str()) {
                    (Value::Array(a), "len") => Ok(Value::Int(a.borrow().len() as i64)),
                    (Value::Map(m), "len") => Ok(Value::Int(m.borrow().len() as i64)),
                    (Value::Str(s), "len") => Ok(Value::Int(s.chars().count() as i64)),
                    (Value::Struct(s), fname) => {
                        for (k, v) in s.borrow().fields.iter() {
                            if k == fname { return Ok(v.clone()); }
                        }
                        Err(Flow::Err(RockError::runtime(format!(
                            "no field '{}' on struct '{}'", fname, s.borrow().type_name
                        ))))
                    }
                    (Value::TypeRef(tn), mname) => {
                        if let Some(methods) = self.impls.get(tn.as_str()) {
                            if let Some(m) = methods.get(mname) {
                                return Ok(Value::Function(Rc::new(Closure {
                                    func: m.clone(),
                                    captured: None,
                                })));
                            }
                        }
                        Err(Flow::Err(RockError::runtime(format!(
                            "no associated '{}' on type '{}'", mname, tn
                        ))))
                    }
                    (Value::Map(m), key) => {
                        let m = m.borrow();
                        for (k, v) in m.iter() {
                            if let Value::Str(s) = k {
                                if s.as_str() == key { return Ok(v.clone()); }
                            }
                        }
                        Ok(Value::Nil)
                    }
                    _ => Err(Flow::Err(RockError::runtime(
                        format!("no field '{}' on {}", name, b.type_name())
                    ))),
                }
            }
            Expr::OptField { base, name, .. } => {
                let b = self.eval(base, env)?;
                if matches!(b, Value::Nil) {
                    return Ok(Value::Nil);
                }
                match (&b, name.as_str()) {
                    (Value::Struct(s), fname) => {
                        for (k, v) in s.borrow().fields.iter() {
                            if k == fname { return Ok(v.clone()); }
                        }
                        Ok(Value::Nil)
                    }
                    (Value::Map(m), key) => {
                        let m = m.borrow();
                        for (k, v) in m.iter() {
                            if let Value::Str(s) = k {
                                if s.as_str() == key { return Ok(v.clone()); }
                            }
                        }
                        Ok(Value::Nil)
                    }
                    _ => Ok(Value::Nil),
                }
            }
            Expr::Range { start, end, .. } => {
                let a = self.eval(start, env)?;
                let b = self.eval(end, env)?;
                match (a, b) {
                    (Value::Int(a), Value::Int(b)) => Ok(Value::Range(a, b)),
                    _ => Err(Flow::Err(RockError::type_err("range bounds must be int"))),
                }
            }
            Expr::Panic(inner, _) => {
                let v = self.eval(inner, env)?;
                if let Value::Nil = v {
                    Err(Flow::Err(RockError::runtime("unwrap of nil")))
                } else {
                    Ok(v)
                }
            }
            Expr::DefaultOr { lhs, default, .. } => {
                let v = self.eval(lhs, env)?;
                if matches!(v, Value::Nil) {
                    self.eval(default, env)
                } else {
                    Ok(v)
                }
            }
            Expr::Pipe { lhs, rhs, .. } => {
                let value = self.eval(lhs, env)?;
                let callee = self.eval(rhs, env)?;
                self.call(&callee, vec![value])
            }
            Expr::Lambda { params, body, span } => {
                let func = Function {
                    name: "<lambda>".to_string(),
                    params: params.clone(),
                    body: body.clone(),
                    span: *span,
                    attrs: Vec::new(),
                    has_self: false,
                    is_pub: false,
                };
                Ok(Value::Function(Rc::new(Closure {
                    func: Rc::new(func),
                    captured: Some(env.clone()),
                })))
            }
            Expr::Map(pairs, _) => {
                self.check_no_alloc("map literal")?;
                let mut out = Vec::with_capacity(pairs.len());
                for (k, v) in pairs {
                    let kv = self.eval(k, env)?;
                    let vv = self.eval(v, env)?;
                    out.push((kv, vv));
                }
                Ok(Value::Map(Rc::new(RefCell::new(out))))
            }
            Expr::Interp(parts, _) => {
                self.check_no_alloc("string interpolation")?;
                let mut s = String::new();
                for p in parts {
                    match p {
                        InterpPart::Lit(lit) => s.push_str(lit),
                        InterpPart::Expr(e) => {
                            let v = self.eval(e, env)?;
                            s.push_str(&v.to_string());
                        }
                    }
                }
                Ok(Value::Str(Rc::new(s)))
            }
            Expr::Match { scrutinee, arms, .. } => {
                let value = self.eval(scrutinee, env)?;
                for arm in arms {
                    let inner = Env::with_parent(env.clone());
                    if self.pattern_match(&arm.pattern, &value, &inner)? {
                        if let Some(guard) = &arm.guard {
                            let g = self.eval(guard, &inner)?;
                            if !g.is_truthy() { continue; }
                        }
                        return self.eval(&arm.body, &inner);
                    }
                }
                Err(Flow::Err(RockError::runtime("no match arm matched")))
            }
            Expr::Spawn(inner, _) => {
                if let Expr::Call { callee, args, .. } = inner.as_ref() {
                    let callee_v = self.eval(callee, env)?;
                    let mut evaluated = Vec::with_capacity(args.len());
                    for a in args { evaluated.push(self.eval(a, env)?); }
                    let id = {
                        let mut n = self.next_task_id.borrow_mut();
                        let v = *n; *n += 1; v
                    };
                    let task = Rc::new(RefCell::new(TaskState::Pending {
                        callee: callee_v,
                        args: evaluated,
                        id,
                    }));
                    let handle = Value::Task(task);
                    self.task_queue.borrow_mut().push(handle.clone());
                    Ok(handle)
                } else {
                    self.eval(inner, env)
                }
            }
            Expr::Raw(block) => self.exec_block(block, env),
            Expr::Comptime(inner, _) => self.eval(inner, env),
            Expr::Await(inner, _) => {
                let v = self.eval(inner, env)?;
                match v {
                    Value::Task(t) => self.await_task(&t).map_err(Flow::Err),
                    other => Ok(other),
                }
            }
            Expr::Try(inner, _) => {
                let v = self.eval(inner, env)?;
                match &v {
                    Value::Nil => Err(Flow::Return(Value::Nil)),
                    Value::Struct(s) if s.borrow().type_name == "Err" => {
                        Err(Flow::Return(v.clone()))
                    }
                    Value::Struct(s) if s.borrow().type_name == "Ok" => {
                        let inner_val = s.borrow().fields.iter()
                            .find(|(n, _)| n == "value")
                            .map(|(_, v)| v.clone())
                            .unwrap_or(Value::Nil);
                        Ok(inner_val)
                    }
                    _ => Ok(v),
                }
            }
            Expr::TryCatch { try_body, err_name, catch_body, .. } => {
                let try_env = Env::with_parent(env.clone());
                match self.exec_block(try_body, &try_env) {
                    Ok(v) => Ok(v),
                    Err(Flow::Err(e)) => {
                        let catch_env = Env::with_parent(env.clone());
                        catch_env.borrow_mut().define(err_name, Value::Str(Rc::new(e.message.clone())), false);
                        self.exec_block(catch_body, &catch_env)
                    }
                    Err(other) => Err(other),
                }
            }
        }
    }

    fn pattern_match(&mut self, pat: &Pattern, value: &Value, env: &Rc<RefCell<Env>>) -> FlowResult<bool> {
        match pat {
            Pattern::Wildcard => Ok(true),
            Pattern::Binding(name) => {
                if let Some(VariantKind::Nullary) = self.variant_info.get(name) {
                    if let Value::Struct(s) = value {
                        if &s.borrow().type_name == name && s.borrow().fields.is_empty() {
                            return Ok(true);
                        }
                    }
                    return Ok(false);
                }
                env.borrow_mut().define(name, value.clone(), false);
                Ok(true)
            }
            Pattern::Literal(expr) => {
                let lit = self.eval(expr, env)?;
                Ok(lit == *value)
            }
            Pattern::Range { start, end } => {
                let s = self.eval(start, env)?;
                let e = self.eval(end, env)?;
                match (value, s, e) {
                    (Value::Int(v), Value::Int(a), Value::Int(b)) => Ok(*v >= a && *v < b),
                    _ => Ok(false),
                }
            }
            Pattern::Array { items, rest } => {
                let arr = match value {
                    Value::Array(a) => a.borrow().clone(),
                    _ => return Ok(false),
                };
                match rest {
                    None => {
                        if arr.len() != items.len() { return Ok(false); }
                        for (p, v) in items.iter().zip(arr.iter()) {
                            if !self.pattern_match(p, v, env)? { return Ok(false); }
                        }
                        Ok(true)
                    }
                    Some(rest_name) => {
                        if arr.len() < items.len() { return Ok(false); }
                        for (p, v) in items.iter().zip(arr.iter()) {
                            if !self.pattern_match(p, v, env)? { return Ok(false); }
                        }
                        if let Some(name) = rest_name {
                            let remaining: Vec<Value> = arr[items.len()..].to_vec();
                            env.borrow_mut().define(
                                name,
                                Value::Array(Rc::new(RefCell::new(remaining))),
                                false,
                            );
                        }
                        Ok(true)
                    }
                }
            }
            Pattern::Tuple(items) => {
                let arr = match value {
                    Value::Array(a) => a.borrow().clone(),
                    _ => return Ok(false),
                };
                if arr.len() != items.len() { return Ok(false); }
                for (p, v) in items.iter().zip(arr.iter()) {
                    if !self.pattern_match(p, v, env)? { return Ok(false); }
                }
                Ok(true)
            }
            Pattern::Struct { type_name, fields, rest } => {
                match value {
                    Value::Struct(s) => {
                        let sref = s.borrow();
                        if let Some(tn) = type_name {
                            if &sref.type_name != tn { return Ok(false); }
                        }
                        if !*rest && fields.len() != sref.fields.len() { return Ok(false); }
                        for (fname, subpat) in fields {
                            let mut found = None;
                            for (k, v) in sref.fields.iter() {
                                if k == fname { found = Some(v.clone()); break; }
                            }
                            match found {
                                Some(v) => {
                                    if !self.pattern_match(subpat, &v, env)? { return Ok(false); }
                                }
                                None => return Ok(false),
                            }
                        }
                        Ok(true)
                    }
                    Value::Map(m) => {
                        if type_name.is_some() { return Ok(false); }
                        let mref = m.borrow();
                        for (fname, subpat) in fields {
                            let mut found = None;
                            for (k, v) in mref.iter() {
                                if let Value::Str(s) = k {
                                    if s.as_str() == fname { found = Some(v.clone()); break; }
                                }
                            }
                            match found {
                                Some(v) => {
                                    if !self.pattern_match(subpat, &v, env)? { return Ok(false); }
                                }
                                None => return Ok(false),
                            }
                        }
                        Ok(true)
                    }
                    _ => Ok(false),
                }
            }
            Pattern::Or(alts) => {
                for alt in alts {
                    if self.pattern_match(alt, value, env)? { return Ok(true); }
                }
                Ok(false)
            }
            Pattern::VariantCall { name, args } => {
                let s = match value {
                    Value::Struct(s) => s.clone(),
                    _ => return Ok(false),
                };
                let sref = s.borrow();
                if &sref.type_name != name { return Ok(false); }
                if sref.fields.len() != args.len() { return Ok(false); }
                for (i, pat) in args.iter().enumerate() {
                    let v = sref.fields[i].1.clone();
                    if !self.pattern_match(pat, &v, env)? { return Ok(false); }
                }
                Ok(true)
            }
        }
    }

    fn call(&mut self, callee: &Value, args: Vec<Value>) -> FlowResult<Value> {
        match callee {
            Value::Function(c) => self.call_closure(c, args).map_err(Flow::Err),
            Value::Overloads(fs) => {
                let chosen = self.pick_overload(fs, &args)?;
                self.call_closure(&chosen, args).map_err(Flow::Err)
            }
            Value::Native(nf) => {
                self.check_native_effect(nf)?;
                nf(&args).map_err(Flow::Err)
            }
            Value::TypeRef(tn) => {
                if let Some(methods) = self.impls.get(tn.as_str()).cloned() {
                    if let Some(m) = methods.get("new") {
                        let closure = Rc::new(Closure { func: m.clone(), captured: None });
                        return self.call_closure(&closure, args).map_err(Flow::Err);
                    }
                }
                Err(Flow::Err(RockError::runtime(format!(
                    "type '{}' has no constructor; define 'fn new' in impl", tn
                ))))
            }
            other => Err(Flow::Err(RockError::type_err(
                format!("cannot call {}", other.type_name())
            ))),
        }
    }

    fn pick_overload(&mut self, fs: &Rc<Vec<Rc<Closure>>>, args: &[Value]) -> FlowResult<Rc<Closure>> {
        let mut best: Option<(usize, Rc<Closure>)> = None;
        for c in fs.iter() {
            let f = &c.func;
            if f.params.len() != args.len() { continue; }
            let mut score: usize = 0;
            let mut ok = true;
            for (p, a) in f.params.iter().zip(args.iter()) {
                let s = self.match_param_score(p, a)?;
                match s {
                    None => { ok = false; break; }
                    Some(n) => score += n,
                }
            }
            if ok {
                match &best {
                    None => best = Some((score, c.clone())),
                    Some((s, _)) if score > *s => best = Some((score, c.clone())),
                    _ => {}
                }
            }
        }
        match best {
            Some((_, c)) => Ok(c),
            None => Err(Flow::Err(RockError::runtime(format!(
                "no matching overload for '{}' with {} argument(s) of type(s) [{}]",
                fs.first().map(|c| c.func.name.as_str()).unwrap_or("?"),
                args.len(),
                args.iter().map(|a| a.type_name()).collect::<Vec<_>>().join(", ")
            )))),
        }
    }

    fn match_param_score(&mut self, p: &Param, a: &Value) -> FlowResult<Option<usize>> {
        if let Some(lit_expr) = &p.literal {
            let lit = self.eval(lit_expr, &self.globals.clone())?;
            return Ok(if lit == *a { Some(100) } else { None });
        }
        if let Some(ty) = &p.ty {
            let matches = type_matches(ty, a);
            return Ok(if matches { Some(10) } else { None });
        }
        Ok(Some(1))
    }

    fn check_native_effect(&self, _nf: &crate::value::NativeFn) -> FlowResult<()> {
        Ok(())
    }

    fn dispatch_method(&mut self, recv: Value, method: &str, args: Vec<Value>) -> FlowResult<Value> {
        if let Value::Array(a) = &recv {
            match method {
                "map" => {
                    if args.len() != 1 { return Err(Flow::Err(RockError::runtime("map: expected (fn)"))); }
                    let f = args[0].clone();
                    let items: Vec<Value> = a.borrow().clone();
                    let mut out = Vec::with_capacity(items.len());
                    for v in items { out.push(self.call(&f, vec![v])?); }
                    return Ok(Value::Array(Rc::new(RefCell::new(out))));
                }
                "filter" => {
                    if args.len() != 1 { return Err(Flow::Err(RockError::runtime("filter: expected (fn)"))); }
                    let f = args[0].clone();
                    let items: Vec<Value> = a.borrow().clone();
                    let mut out = Vec::new();
                    for v in items {
                        let keep = self.call(&f, vec![v.clone()])?;
                        if keep.is_truthy() { out.push(v); }
                    }
                    return Ok(Value::Array(Rc::new(RefCell::new(out))));
                }
                "reduce" => {
                    if args.len() != 2 { return Err(Flow::Err(RockError::runtime("reduce: expected (fn, init)"))); }
                    let f = args[0].clone();
                    let mut acc = args[1].clone();
                    let items: Vec<Value> = a.borrow().clone();
                    for v in items { acc = self.call(&f, vec![acc, v])?; }
                    return Ok(acc);
                }
                "any" => {
                    if args.len() != 1 { return Err(Flow::Err(RockError::runtime("any: expected (fn)"))); }
                    let f = args[0].clone();
                    let items: Vec<Value> = a.borrow().clone();
                    for v in items {
                        if self.call(&f, vec![v])?.is_truthy() { return Ok(Value::Bool(true)); }
                    }
                    return Ok(Value::Bool(false));
                }
                "all" => {
                    if args.len() != 1 { return Err(Flow::Err(RockError::runtime("all: expected (fn)"))); }
                    let f = args[0].clone();
                    let items: Vec<Value> = a.borrow().clone();
                    for v in items {
                        if !self.call(&f, vec![v])?.is_truthy() { return Ok(Value::Bool(false)); }
                    }
                    return Ok(Value::Bool(true));
                }
                "find" => {
                    if args.len() != 1 { return Err(Flow::Err(RockError::runtime("find: expected (fn)"))); }
                    let f = args[0].clone();
                    let items: Vec<Value> = a.borrow().clone();
                    for v in items {
                        if self.call(&f, vec![v.clone()])?.is_truthy() { return Ok(v); }
                    }
                    return Ok(Value::Nil);
                }
                "each" => {
                    if args.len() != 1 { return Err(Flow::Err(RockError::runtime("each: expected (fn)"))); }
                    let f = args[0].clone();
                    let items: Vec<Value> = a.borrow().clone();
                    for v in items { self.call(&f, vec![v])?; }
                    return Ok(Value::Nil);
                }
                "count" => {
                    if args.is_empty() {
                        return Ok(Value::Int(a.borrow().len() as i64));
                    }
                    let f = args[0].clone();
                    let items: Vec<Value> = a.borrow().clone();
                    let mut n = 0i64;
                    for v in items {
                        if self.call(&f, vec![v])?.is_truthy() { n += 1; }
                    }
                    return Ok(Value::Int(n));
                }
                "sort" => {
                    let items: Vec<Value> = a.borrow().clone();
                    let sorted = if args.is_empty() {
                        let mut v = items;
                        v.sort_by(|x, y| default_value_cmp(x, y));
                        v
                    } else {
                        let f = args[0].clone();
                        let mut indexed: Vec<(usize, Value)> = items.into_iter().enumerate().collect();
                        let mut err: Option<Flow> = None;
                        indexed.sort_by(|(_, x), (_, y)| {
                            if err.is_some() { return std::cmp::Ordering::Equal; }
                            match self.call(&f, vec![x.clone(), y.clone()]) {
                                Ok(Value::Int(i)) => if i < 0 { std::cmp::Ordering::Less } else if i > 0 { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Equal },
                                Ok(Value::Float(fv)) => if fv < 0.0 { std::cmp::Ordering::Less } else if fv > 0.0 { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Equal },
                                Ok(Value::Bool(b)) => if b { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater },
                                Ok(_) => std::cmp::Ordering::Equal,
                                Err(e) => { err = Some(e); std::cmp::Ordering::Equal }
                            }
                        });
                        if let Some(e) = err { return Err(e); }
                        indexed.into_iter().map(|(_, v)| v).collect()
                    };
                    return Ok(Value::Array(Rc::new(RefCell::new(sorted))));
                }
                "slice" => {
                    let items = a.borrow();
                    let n = items.len() as i64;
                    let normalize = |i: i64| -> usize {
                        if i < 0 { ((n + i).max(0)) as usize } else { (i.min(n)) as usize }
                    };
                    let (start, end) = match (args.get(0), args.get(1)) {
                        (Some(Value::Int(s)), Some(Value::Int(e))) => (normalize(*s), normalize(*e)),
                        (Some(Value::Int(s)), None) => (normalize(*s), n as usize),
                        _ => return Err(Flow::Err(RockError::runtime("slice: expected (start, end?)"))),
                    };
                    let start = start.min(end);
                    let out: Vec<Value> = items[start..end].to_vec();
                    return Ok(Value::Array(Rc::new(RefCell::new(out))));
                }
                "flatten" => {
                    let items = a.borrow();
                    let mut out = Vec::new();
                    for v in items.iter() {
                        if let Value::Array(inner) = v {
                            for iv in inner.borrow().iter() { out.push(iv.clone()); }
                        } else {
                            out.push(v.clone());
                        }
                    }
                    return Ok(Value::Array(Rc::new(RefCell::new(out))));
                }
                "sum" => {
                    let items = a.borrow();
                    let mut has_float = false;
                    for v in items.iter() {
                        if matches!(v, Value::Float(_)) { has_float = true; break; }
                    }
                    if has_float {
                        let mut s = 0.0f64;
                        for v in items.iter() {
                            match v {
                                Value::Int(i) => s += *i as f64,
                                Value::Float(f) => s += *f,
                                other => return Err(Flow::Err(RockError::runtime(format!("sum: non-numeric {}", other.type_name())))),
                            }
                        }
                        return Ok(Value::Float(s));
                    }
                    let mut s = 0i64;
                    for v in items.iter() {
                        match v {
                            Value::Int(i) => s += i,
                            other => return Err(Flow::Err(RockError::runtime(format!("sum: non-numeric {}", other.type_name())))),
                        }
                    }
                    return Ok(Value::Int(s));
                }
                "min" => {
                    let items = a.borrow();
                    if items.is_empty() { return Err(Flow::Err(RockError::runtime("min: empty array"))); }
                    let mut best = items[0].clone();
                    for v in items.iter().skip(1) {
                        if default_value_cmp(v, &best) == std::cmp::Ordering::Less { best = v.clone(); }
                    }
                    return Ok(best);
                }
                "max" => {
                    let items = a.borrow();
                    if items.is_empty() { return Err(Flow::Err(RockError::runtime("max: empty array"))); }
                    let mut best = items[0].clone();
                    for v in items.iter().skip(1) {
                        if default_value_cmp(v, &best) == std::cmp::Ordering::Greater { best = v.clone(); }
                    }
                    return Ok(best);
                }
                "first" => {
                    let items = a.borrow();
                    return Ok(items.first().cloned().unwrap_or(Value::Nil));
                }
                "last" => {
                    let items = a.borrow();
                    return Ok(items.last().cloned().unwrap_or(Value::Nil));
                }
                "take" => {
                    let n = match args.get(0) {
                        Some(Value::Int(n)) if *n >= 0 => *n as usize,
                        _ => return Err(Flow::Err(RockError::runtime("take: expected non-negative int"))),
                    };
                    let items = a.borrow();
                    let out: Vec<Value> = items.iter().take(n).cloned().collect();
                    return Ok(Value::Array(Rc::new(RefCell::new(out))));
                }
                "drop" => {
                    let n = match args.get(0) {
                        Some(Value::Int(n)) if *n >= 0 => *n as usize,
                        _ => return Err(Flow::Err(RockError::runtime("drop: expected non-negative int"))),
                    };
                    let items = a.borrow();
                    let out: Vec<Value> = items.iter().skip(n).cloned().collect();
                    return Ok(Value::Array(Rc::new(RefCell::new(out))));
                }
                "zip" => {
                    let other = match args.get(0) {
                        Some(Value::Array(b)) => b.borrow().clone(),
                        _ => return Err(Flow::Err(RockError::runtime("zip: expected array"))),
                    };
                    let items = a.borrow();
                    let n = items.len().min(other.len());
                    let mut out = Vec::with_capacity(n);
                    for i in 0..n {
                        let pair = vec![items[i].clone(), other[i].clone()];
                        out.push(Value::Array(Rc::new(RefCell::new(pair))));
                    }
                    return Ok(Value::Array(Rc::new(RefCell::new(out))));
                }
                "chunks" => {
                    let n = match args.get(0) {
                        Some(Value::Int(n)) if *n > 0 => *n as usize,
                        _ => return Err(Flow::Err(RockError::runtime("chunks: expected positive int"))),
                    };
                    let items = a.borrow();
                    let mut out = Vec::new();
                    for chunk in items.chunks(n) {
                        out.push(Value::Array(Rc::new(RefCell::new(chunk.to_vec()))));
                    }
                    return Ok(Value::Array(Rc::new(RefCell::new(out))));
                }
                "windows" => {
                    let n = match args.get(0) {
                        Some(Value::Int(n)) if *n > 0 => *n as usize,
                        _ => return Err(Flow::Err(RockError::runtime("windows: expected positive int"))),
                    };
                    let items = a.borrow();
                    let mut out = Vec::new();
                    if items.len() >= n {
                        for w in items.windows(n) {
                            out.push(Value::Array(Rc::new(RefCell::new(w.to_vec()))));
                        }
                    }
                    return Ok(Value::Array(Rc::new(RefCell::new(out))));
                }
                "unique" => {
                    let items = a.borrow();
                    let mut out: Vec<Value> = Vec::new();
                    for v in items.iter() {
                        if !out.iter().any(|x| x == v) { out.push(v.clone()); }
                    }
                    return Ok(Value::Array(Rc::new(RefCell::new(out))));
                }
                "enumerate" => {
                    let items = a.borrow();
                    let mut out = Vec::with_capacity(items.len());
                    for (i, v) in items.iter().enumerate() {
                        let pair = vec![Value::Int(i as i64), v.clone()];
                        out.push(Value::Array(Rc::new(RefCell::new(pair))));
                    }
                    return Ok(Value::Array(Rc::new(RefCell::new(out))));
                }
                _ => {}
            }
        }
        if let Value::Map(m) = &recv {
            // Check map-keyed callable first (module aliases store functions as map fields)
            let found = {
                let mb = m.borrow();
                let mut out = None;
                for (k, v) in mb.iter() {
                    if let Value::Str(s) = k {
                        if s.as_str() == method {
                            out = Some(v.clone());
                            break;
                        }
                    }
                }
                out
            };
            if let Some(callable) = &found {
                if matches!(callable, Value::Native(_) | Value::Function(_) | Value::Overloads(_) | Value::TypeRef(_)) {
                    return self.call(callable, args);
                }
            }
            match method {
                "each" => {
                    if args.len() != 1 { return Err(Flow::Err(RockError::runtime("each: expected (fn)"))); }
                    let f = args[0].clone();
                    let pairs: Vec<(Value, Value)> = m.borrow().clone();
                    for (k, v) in pairs { self.call(&f, vec![k, v])?; }
                    return Ok(Value::Nil);
                }
                "map_values" => {
                    if args.len() != 1 { return Err(Flow::Err(RockError::runtime("map_values: expected (fn)"))); }
                    let f = args[0].clone();
                    let pairs: Vec<(Value, Value)> = m.borrow().clone();
                    let mut out: Vec<(Value, Value)> = Vec::with_capacity(pairs.len());
                    for (k, v) in pairs {
                        let nv = self.call(&f, vec![v])?;
                        out.push((k, nv));
                    }
                    return Ok(Value::Map(Rc::new(RefCell::new(out))));
                }
                "filter" => {
                    if args.len() != 1 { return Err(Flow::Err(RockError::runtime("filter: expected (fn)"))); }
                    let f = args[0].clone();
                    let pairs: Vec<(Value, Value)> = m.borrow().clone();
                    let mut out: Vec<(Value, Value)> = Vec::new();
                    for (k, v) in pairs {
                        let keep = self.call(&f, vec![k.clone(), v.clone()])?;
                        if keep.is_truthy() { out.push((k, v)); }
                    }
                    return Ok(Value::Map(Rc::new(RefCell::new(out))));
                }
                _ => {}
            }
        }
        if let Value::Struct(s) = &recv {
            let tn = s.borrow().type_name.clone();
            if let Some(methods) = self.impls.get(&tn).cloned() {
                if let Some(m) = methods.get(method) {
                    let closure = Rc::new(Closure { func: m.clone(), captured: None });
                    return self.call_method(&closure, recv.clone(), args).map_err(Flow::Err);
                }
            }
            return Err(Flow::Err(RockError::runtime(format!(
                "no method '{}' on '{}'", method, tn
            ))));
        }
        if let Value::TypeRef(tn) = &recv {
            if let Some(methods) = self.impls.get(tn.as_str()).cloned() {
                if let Some(m) = methods.get(method) {
                    let closure = Rc::new(Closure { func: m.clone(), captured: None });
                    return self.call_closure(&closure, args).map_err(Flow::Err);
                }
            }
            return Err(Flow::Err(RockError::runtime(format!(
                "no associated '{}' on type '{}'", method, tn
            ))));
        }
        if let Value::Map(m) = &recv {
            let found = {
                let mb = m.borrow();
                let mut out = None;
                for (k, v) in mb.iter() {
                    if let Value::Str(s) = k {
                        if s.as_str() == method {
                            out = Some(v.clone());
                            break;
                        }
                    }
                }
                out
            };
            if let Some(callable) = found {
                if matches!(callable, Value::Native(_) | Value::Function(_) | Value::Overloads(_) | Value::TypeRef(_)) {
                    return self.call(&callable, args);
                }
            }
        }
        builtin_method(&recv, method, &args).map_err(Flow::Err)
    }

    fn call_method(&mut self, closure: &Rc<Closure>, recv: Value, args: Vec<Value>) -> Result<Value> {
        let func = &closure.func;
        if args.len() != func.params.len() {
            return Err(RockError::runtime(format!(
                "{} expected {} arguments, got {}",
                func.name, func.params.len(), args.len()
            )));
        }
        let parent = closure.captured.clone().unwrap_or_else(|| self.globals.clone());
        let env = Env::with_parent(parent);
        env.borrow_mut().define("self", recv, true);
        for (p, a) in func.params.iter().zip(args.into_iter()) {
            if p.literal.is_none() {
                env.borrow_mut().define(&p.name, a, true);
            }
        }
        self.check_require(func, &env)?;
        let prev_effects = *self.effects.borrow();
        self.apply_effect_attrs(&func.attrs);
        let result = match self.exec_block(&func.body, &env) {
            Ok(v) => Ok(v),
            Err(Flow::Return(v)) => Ok(v),
            Err(Flow::Err(e)) => Err(e),
            Err(Flow::Break) | Err(Flow::Continue) => {
                Err(RockError::runtime("break/continue outside loop"))
            }
        };
        *self.effects.borrow_mut() = prev_effects;
        match result {
            Ok(v) => {
                self.check_ensure(func, &env, &v)?;
                Ok(v)
            }
            Err(e) => Err(e),
        }
    }

    fn apply_effect_attrs(&mut self, attrs: &[Attribute]) {
        let mut e = self.effects.borrow_mut();
        for a in attrs {
            match a.name.as_str() {
                "pure" => { e.pure_ = true; e.no_io = true; e.no_alloc = true; }
                "no_io" => { e.no_io = true; }
                "no_alloc" => { e.no_alloc = true; }
                _ => {}
            }
        }
    }

    fn eval_grad(&mut self, args: &[Expr], env: &Rc<RefCell<Env>>) -> FlowResult<Value> {
        if args.is_empty() {
            return Err(Flow::Err(RockError::runtime("@grad requires at least the function")));
        }
        let f = self.eval(&args[0], env)?;
        let mut vals = Vec::with_capacity(args.len() - 1);
        for a in &args[1..] { vals.push(self.eval(a, env)?); }
        let h = 1e-5_f64;
        let mut grads = Vec::with_capacity(vals.len());
        for i in 0..vals.len() {
            let (vi_plus, vi_minus) = match &vals[i] {
                Value::Float(x) => (Value::Float(*x + h), Value::Float(*x - h)),
                Value::Int(x) => (Value::Float(*x as f64 + h), Value::Float(*x as f64 - h)),
                _ => {
                    grads.push(Value::Nil);
                    continue;
                }
            };
            let mut a_plus = vals.clone();
            a_plus[i] = vi_plus;
            let mut a_minus = vals.clone();
            a_minus[i] = vi_minus;
            let yp = self.call(&f, a_plus)?;
            let ym = self.call(&f, a_minus)?;
            let (yp_f, ym_f) = match (yp, ym) {
                (Value::Float(a), Value::Float(b)) => (a, b),
                (Value::Int(a), Value::Int(b)) => (a as f64, b as f64),
                (Value::Float(a), Value::Int(b)) => (a, b as f64),
                (Value::Int(a), Value::Float(b)) => (a as f64, b),
                _ => return Err(Flow::Err(RockError::runtime(
                    "@grad: function must return a numeric value"
                ))),
            };
            grads.push(Value::Float((yp_f - ym_f) / (2.0 * h)));
        }
        Ok(Value::Array(Rc::new(RefCell::new(grads))))
    }

    fn eval_trace(&mut self, args: &[Expr], env: &Rc<RefCell<Env>>) -> FlowResult<Value> {
        if args.is_empty() {
            return Err(Flow::Err(RockError::runtime("@trace requires a function to call")));
        }
        let f = self.eval(&args[0], env)?;
        let mut vals = Vec::with_capacity(args.len() - 1);
        for a in &args[1..] { vals.push(self.eval(a, env)?); }
        let args_str = vals.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ");
        eprintln!("[@trace] call({})", args_str);
        let result = self.call(&f, vals)?;
        eprintln!("[@trace] returned {}", result);
        Ok(result)
    }

    fn await_task(&mut self, task: &Rc<RefCell<TaskState>>) -> Result<Value> {
        loop {
            let state = {
                let s = task.borrow();
                match &*s {
                    TaskState::Ready { value, .. } => return Ok(value.clone()),
                    TaskState::Failed { message, .. } => return Err(RockError::runtime(message.clone())),
                    TaskState::Pending { .. } => 0,
                    TaskState::Running { .. } => 1,
                }
            };
            if state == 0 {
                let (callee, args, id) = {
                    let mut s = task.borrow_mut();
                    let taken = std::mem::replace(&mut *s, TaskState::Running { id: 0 });
                    match taken {
                        TaskState::Pending { callee, args, id } => {
                            *s = TaskState::Running { id };
                            (callee, args, id)
                        }
                        other => { *s = other; continue; }
                    }
                };
                self.task_queue.borrow_mut().retain(|h| {
                    if let Value::Task(t) = h { t.borrow().id() != id } else { true }
                });
                match self.call(&callee, args) {
                    Ok(v) => *task.borrow_mut() = TaskState::Ready { id, value: v },
                    Err(Flow::Err(e)) => *task.borrow_mut() = TaskState::Failed { id, message: e.to_string() },
                    Err(_) => *task.borrow_mut() = TaskState::Failed { id, message: "control flow escaped task".to_string() },
                }
            } else {
                if !self.run_one_pending_task() {
                    return Err(RockError::runtime("await: task deadlocked (never became ready)"));
                }
            }
        }
    }

    fn run_one_pending_task(&mut self) -> bool {
        let next = {
            let mut q = self.task_queue.borrow_mut();
            if q.is_empty() { None } else { Some(q.remove(0)) }
        };
        match next {
            Some(Value::Task(t)) => {
                let _ = self.await_task(&t);
                true
            }
            _ => false,
        }
    }

    fn drain_task_queue(&mut self) {
        while self.run_one_pending_task() {}
    }

    fn eval_reflect(&mut self, args: &[Expr], env: &Rc<RefCell<Env>>) -> FlowResult<Value> {
        if args.len() != 1 {
            return Err(Flow::Err(RockError::runtime("@reflect(x) takes one argument")));
        }
        let v = self.eval(&args[0], env)?;
        match v {
            Value::Struct(s) => {
                let s = s.borrow();
                let mut pairs: Vec<Value> = Vec::new();
                for (n, fv) in s.fields.iter() {
                    let mut entry_fields = Vec::new();
                    entry_fields.push(("name".to_string(), Value::Str(Rc::new(n.clone()))));
                    entry_fields.push(("kind".to_string(), Value::Str(Rc::new(fv.type_name().to_string()))));
                    entry_fields.push(("value".to_string(), fv.clone()));
                    pairs.push(Value::Struct(Rc::new(RefCell::new(Struct {
                        type_name: "Field".to_string(),
                        fields: entry_fields,
                    }))));
                }
                Ok(Value::Array(Rc::new(RefCell::new(pairs))))
            }
            other => {
                let fields = vec![
                    ("name".to_string(), Value::Str(Rc::new("<value>".to_string()))),
                    ("kind".to_string(), Value::Str(Rc::new(other.type_name().to_string()))),
                    ("value".to_string(), other.clone()),
                ];
                Ok(Value::Array(Rc::new(RefCell::new(vec![
                    Value::Struct(Rc::new(RefCell::new(Struct {
                        type_name: "Field".to_string(),
                        fields,
                    })))
                ]))))
            }
        }
    }

    fn check_no_alloc(&self, what: &str) -> FlowResult<()> {
        let e = self.effects.borrow();
        if e.no_alloc || e.pure_ {
            return Err(Flow::Err(RockError::runtime(format!(
                "effect violation: {} not allowed in @no_alloc/@pure context", what
            ))));
        }
        Ok(())
    }

    fn verify_prove_block(&mut self, block: &ProveBlock) -> Result<()> {
        for a in &block.assertions {
            match a {
                ProveAssertion::Unreachable { from, to, .. } => {
                    let (sm_a, state_a) = extract_sm_state(from)
                        .ok_or_else(|| RockError::runtime(
                            "assert_unreachable: expected 'StateMachine.State' expression"
                        ))?;
                    let (sm_b, state_b) = extract_sm_state(to)
                        .ok_or_else(|| RockError::runtime(
                            "assert_unreachable: expected 'StateMachine.State' expression"
                        ))?;
                    if sm_a != sm_b {
                        return Err(RockError::runtime(format!(
                            "@prove assert_unreachable: cross-machine transition '{}' -> '{}' not supported",
                            sm_a, sm_b
                        )));
                    }
                    let sm = self.state_machines.get(&sm_a).ok_or_else(|| {
                        RockError::runtime(format!("@prove: unknown state_machine '{}'", sm_a))
                    })?.clone();
                    if !sm.states.iter().any(|s| s == &state_a) {
                        return Err(RockError::runtime(format!(
                            "@prove: '{}' has no state '{}'", sm_a, state_a
                        )));
                    }
                    if !sm.states.iter().any(|s| s == &state_b) {
                        return Err(RockError::runtime(format!(
                            "@prove: '{}' has no state '{}'", sm_b, state_b
                        )));
                    }
                    if reachable(&sm.transitions, &state_a, &state_b) {
                        return Err(RockError::runtime(format!(
                            "@prove FAILED: '{}.{}' IS reachable from '{}.{}'",
                            sm_b, state_b, sm_a, state_a
                        )));
                    }
                }
                ProveAssertion::Never { expr, .. } => {
                    let env = self.globals.clone();
                    let v = match self.eval(expr, &env) {
                        Ok(v) => v,
                        Err(Flow::Err(e)) => return Err(e),
                        Err(_) => return Err(RockError::runtime("@prove: invalid assert_never expression")),
                    };
                    if v.is_truthy() {
                        return Err(RockError::runtime(
                            "@prove FAILED: assert_never condition is true"
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn call_closure(&mut self, closure: &Rc<Closure>, args: Vec<Value>) -> Result<Value> {
        let func = &closure.func;
        if func.attrs.iter().any(|a| a.name == "extern_stub") {
            return Err(RockError::runtime(format!(
                "extern '{}' is unresolved; @extern FFI not implemented in dev mode (use 'rock build' for linked binaries)",
                func.name
            )));
        }
        if args.len() != func.params.len() {
            return Err(RockError::runtime(format!(
                "{} expected {} arguments, got {}",
                func.name, func.params.len(), args.len()
            )));
        }
        let parent = closure.captured.clone().unwrap_or_else(|| self.globals.clone());
        let env = Env::with_parent(parent);
        for (p, a) in func.params.iter().zip(args.into_iter()) {
            if p.literal.is_none() {
                env.borrow_mut().define(&p.name, a, true);
            }
        }
        self.check_require(func, &env)?;
        let prev_effects = *self.effects.borrow();
        self.apply_effect_attrs(&func.attrs);
        let result = match self.exec_block(&func.body, &env) {
            Ok(v) => Ok(v),
            Err(Flow::Return(v)) => Ok(v),
            Err(Flow::Err(e)) => Err(e),
            Err(Flow::Break) | Err(Flow::Continue) => {
                Err(RockError::runtime("break/continue outside loop"))
            }
        };
        *self.effects.borrow_mut() = prev_effects;
        match result {
            Ok(v) => {
                self.check_ensure(func, &env, &v)?;
                Ok(v)
            }
            Err(e) => Err(e),
        }
    }

    fn check_require(&mut self, func: &Function, env: &Rc<RefCell<Env>>) -> Result<()> {
        for attr in &func.attrs {
            if attr.name == "require" {
                for cond in &attr.args {
                    match self.eval(cond, env) {
                        Ok(v) => {
                            if !v.is_truthy() {
                                return Err(RockError::runtime(format!(
                                    "@require failed in '{}'", func.name
                                )));
                            }
                        }
                        Err(Flow::Err(e)) => return Err(e),
                        Err(_) => return Err(RockError::runtime("@require: invalid flow")),
                    }
                }
            }
        }
        Ok(())
    }

    fn check_ensure(&mut self, func: &Function, env: &Rc<RefCell<Env>>, result: &Value) -> Result<()> {
        let has_ensure = func.attrs.iter().any(|a| a.name == "ensure");
        if !has_ensure { return Ok(()); }
        let inner = Env::with_parent(env.clone());
        inner.borrow_mut().define("result", result.clone(), false);
        for attr in &func.attrs {
            if attr.name == "ensure" {
                for cond in &attr.args {
                    match self.eval(cond, &inner) {
                        Ok(v) => {
                            if !v.is_truthy() {
                                return Err(RockError::runtime(format!(
                                    "@ensure failed in '{}'", func.name
                                )));
                            }
                        }
                        Err(Flow::Err(e)) => return Err(e),
                        Err(_) => return Err(RockError::runtime("@ensure: invalid flow")),
                    }
                }
            }
        }
        Ok(())
    }
}

fn extract_sm_state(e: &Expr) -> Option<(String, String)> {
    match e {
        Expr::Field { base, name, .. } => {
            if let Expr::Ident(sm, _) = base.as_ref() {
                return Some((sm.clone(), name.clone()));
            }
            None
        }
        Expr::Path { segments, .. } if segments.len() == 2 => {
            Some((segments[0].clone(), segments[1].clone()))
        }
        _ => None,
    }
}

fn reachable(transitions: &[(String, String)], from: &str, to: &str) -> bool {
    use std::collections::HashSet;
    let mut visited: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = vec![from.to_string()];
    while let Some(cur) = stack.pop() {
        for (a, b) in transitions {
            if a == &cur {
                if b == to { return true; }
                if visited.insert(b.clone()) {
                    stack.push(b.clone());
                }
            }
        }
    }
    false
}

fn type_matches(ty: &str, v: &Value) -> bool {
    match ty {
        "int" => matches!(v, Value::Int(_)),
        "float" => matches!(v, Value::Float(_)),
        "str" | "string" => matches!(v, Value::Str(_)),
        "bool" => matches!(v, Value::Bool(_)),
        "nil" => matches!(v, Value::Nil),
        "any" | "_" => true,
        _ => {
            if let Value::Struct(s) = v {
                s.borrow().type_name == ty
            } else {
                false
            }
        }
    }
}

fn to_usize(v: &Value) -> Result<usize> {
    match v {
        Value::Int(i) if *i >= 0 => Ok(*i as usize),
        _ => Err(RockError::type_err(format!("expected non-negative int, got {}", v.type_name()))),
    }
}

fn channel_id(v: &Value) -> Result<u64> {
    match v {
        Value::Struct(s) => {
            let s = s.borrow();
            if s.type_name != "Channel" {
                return Err(RockError::type_err("expected Channel"));
            }
            for (n, fv) in s.fields.iter() {
                if n == "__chan_id" {
                    if let Value::Int(i) = fv {
                        return Ok(*i as u64);
                    }
                }
            }
            Err(RockError::runtime("invalid Channel struct"))
        }
        _ => Err(RockError::type_err(format!("expected Channel, got {}", v.type_name()))),
    }
}

fn arena_id(v: &Value) -> Result<u64> {
    match v {
        Value::Struct(s) => {
            let s = s.borrow();
            if s.type_name != "Arena" {
                return Err(RockError::type_err("expected Arena"));
            }
            for (n, fv) in s.fields.iter() {
                if n == "__arena_id" {
                    if let Value::Int(i) = fv {
                        return Ok(*i as u64);
                    }
                }
            }
            Err(RockError::runtime("invalid Arena struct"))
        }
        _ => Err(RockError::type_err(format!("expected Arena, got {}", v.type_name()))),
    }
}

fn builtin_method(recv: &Value, method: &str, args: &[Value]) -> Result<Value> {
    match (recv, method) {
        (Value::Array(a), "push") => {
            if args.len() != 1 { return Err(RockError::runtime("push() takes 1 argument")); }
            a.borrow_mut().push(args[0].clone());
            Ok(Value::Nil)
        }
        (Value::Array(a), "pop") => {
            let v = a.borrow_mut().pop().unwrap_or(Value::Nil);
            Ok(v)
        }
        (Value::Array(a), "len") => Ok(Value::Int(a.borrow().len() as i64)),
        (Value::Array(a), "reverse") => {
            a.borrow_mut().reverse();
            Ok(Value::Nil)
        }
        (Value::Array(a), "contains") => {
            if args.len() != 1 { return Err(RockError::runtime("contains() takes 1 argument")); }
            for v in a.borrow().iter() {
                if v == &args[0] { return Ok(Value::Bool(true)); }
            }
            Ok(Value::Bool(false))
        }
        (Value::Array(a), "join") => {
            let sep = match args.get(0) {
                Some(Value::Str(s)) => s.as_str().to_string(),
                None => "".to_string(),
                _ => return Err(RockError::runtime("join() expects string separator")),
            };
            let parts: Vec<String> = a.borrow().iter().map(|v| v.to_string()).collect();
            Ok(Value::Str(Rc::new(parts.join(&sep))))
        }
        (Value::Map(m), "len") => Ok(Value::Int(m.borrow().len() as i64)),
        (Value::Map(m), "has") => {
            if args.len() != 1 { return Err(RockError::runtime("has() takes 1 argument")); }
            for (k, _) in m.borrow().iter() {
                if k == &args[0] { return Ok(Value::Bool(true)); }
            }
            Ok(Value::Bool(false))
        }
        (Value::Map(m), "keys") => {
            let ks: Vec<Value> = m.borrow().iter().map(|(k, _)| k.clone()).collect();
            Ok(Value::Array(Rc::new(RefCell::new(ks))))
        }
        (Value::Map(m), "values") => {
            let vs: Vec<Value> = m.borrow().iter().map(|(_, v)| v.clone()).collect();
            Ok(Value::Array(Rc::new(RefCell::new(vs))))
        }
        (Value::Map(m), "remove") => {
            if args.len() != 1 { return Err(RockError::runtime("remove() takes 1 argument")); }
            let mut m = m.borrow_mut();
            if let Some(i) = m.iter().position(|(k, _)| k == &args[0]) {
                let (_, v) = m.remove(i);
                Ok(v)
            } else {
                Ok(Value::Nil)
            }
        }
        (Value::Str(s), "len") => Ok(Value::Int(s.chars().count() as i64)),
        (Value::Str(s), "upper") => Ok(Value::Str(Rc::new(s.to_uppercase()))),
        (Value::Str(s), "lower") => Ok(Value::Str(Rc::new(s.to_lowercase()))),
        (Value::Str(s), "trim") => Ok(Value::Str(Rc::new(s.trim().to_string()))),
        (Value::Str(s), "contains") => {
            match args.get(0) {
                Some(Value::Str(needle)) => Ok(Value::Bool(s.contains(needle.as_str()))),
                _ => Err(RockError::runtime("contains() expects string")),
            }
        }
        (Value::Str(s), "starts_with") => match args.get(0) {
            Some(Value::Str(p)) => Ok(Value::Bool(s.starts_with(p.as_str()))),
            _ => Err(RockError::runtime("starts_with() expects string")),
        },
        (Value::Str(s), "ends_with") => match args.get(0) {
            Some(Value::Str(p)) => Ok(Value::Bool(s.ends_with(p.as_str()))),
            _ => Err(RockError::runtime("ends_with() expects string")),
        },
        (Value::Str(s), "split") => match args.get(0) {
            Some(Value::Str(sep)) => {
                let parts: Vec<Value> = s.split(sep.as_str())
                    .map(|p| Value::Str(Rc::new(p.to_string())))
                    .collect();
                Ok(Value::Array(Rc::new(RefCell::new(parts))))
            }
            _ => Err(RockError::runtime("split() expects string")),
        },
        (Value::Str(s), "replace") => match (args.get(0), args.get(1)) {
            (Some(Value::Str(from)), Some(Value::Str(to))) => {
                Ok(Value::Str(Rc::new(s.replace(from.as_str(), to.as_str()))))
            }
            _ => Err(RockError::runtime("replace() expects (str, str)")),
        },
        (Value::Str(s), "chars") => {
            let cs: Vec<Value> = s.chars()
                .map(|c| Value::Str(Rc::new(c.to_string())))
                .collect();
            Ok(Value::Array(Rc::new(RefCell::new(cs))))
        }
        (Value::Str(s), "bytes") => {
            let bs: Vec<Value> = s.as_bytes().iter()
                .map(|b| Value::Int(*b as i64))
                .collect();
            Ok(Value::Array(Rc::new(RefCell::new(bs))))
        }
        (Value::Str(s), "repeat") => match args.get(0) {
            Some(Value::Int(n)) if *n >= 0 => {
                Ok(Value::Str(Rc::new(s.repeat(*n as usize))))
            }
            _ => Err(RockError::runtime("repeat() expects non-negative int")),
        },
        (Value::Str(s), "find") => match args.get(0) {
            Some(Value::Str(needle)) => match s.find(needle.as_str()) {
                Some(i) => {
                    let char_idx = s[..i].chars().count() as i64;
                    Ok(Value::Int(char_idx))
                }
                None => Ok(Value::Int(-1)),
            },
            _ => Err(RockError::runtime("find() expects string")),
        },
        (Value::Str(s), "pad_left") => {
            let n = match args.get(0) {
                Some(Value::Int(n)) if *n >= 0 => *n as usize,
                _ => return Err(RockError::runtime("pad_left() expects (width, [pad])")),
            };
            let pad = match args.get(1) {
                Some(Value::Str(p)) if !p.is_empty() => p.chars().next().unwrap(),
                None => ' ',
                _ => return Err(RockError::runtime("pad_left() pad must be 1-char string")),
            };
            let len = s.chars().count();
            if len >= n { return Ok(Value::Str(s.clone())); }
            let mut out = String::with_capacity(n);
            for _ in 0..(n - len) { out.push(pad); }
            out.push_str(s);
            Ok(Value::Str(Rc::new(out)))
        }
        (Value::Str(s), "pad_right") => {
            let n = match args.get(0) {
                Some(Value::Int(n)) if *n >= 0 => *n as usize,
                _ => return Err(RockError::runtime("pad_right() expects (width, [pad])")),
            };
            let pad = match args.get(1) {
                Some(Value::Str(p)) if !p.is_empty() => p.chars().next().unwrap(),
                None => ' ',
                _ => return Err(RockError::runtime("pad_right() pad must be 1-char string")),
            };
            let len = s.chars().count();
            if len >= n { return Ok(Value::Str(s.clone())); }
            let mut out = String::with_capacity(n);
            out.push_str(s);
            for _ in 0..(n - len) { out.push(pad); }
            Ok(Value::Str(Rc::new(out)))
        }
        (Value::Str(s), "char_at") => match args.get(0) {
            Some(Value::Int(i)) => {
                let chars: Vec<char> = s.chars().collect();
                let len = chars.len() as i64;
                let idx = if *i < 0 { *i + len } else { *i };
                if idx < 0 || idx >= len { return Ok(Value::Nil); }
                Ok(Value::Str(Rc::new(chars[idx as usize].to_string())))
            }
            _ => Err(RockError::runtime("char_at() expects int")),
        },
        (Value::Str(s), "slice") => {
            let len = s.chars().count() as i64;
            let start = match args.get(0) {
                Some(Value::Int(i)) => *i,
                _ => return Err(RockError::runtime("slice() expects (start, [end])")),
            };
            let end = match args.get(1) {
                Some(Value::Int(i)) => *i,
                None => len,
                _ => return Err(RockError::runtime("slice() end must be int")),
            };
            let s_idx = if start < 0 { (start + len).max(0) } else { start.min(len) };
            let e_idx = if end < 0 { (end + len).max(0) } else { end.min(len) };
            if s_idx >= e_idx { return Ok(Value::Str(Rc::new(String::new()))); }
            let out: String = s.chars()
                .skip(s_idx as usize)
                .take((e_idx - s_idx) as usize)
                .collect();
            Ok(Value::Str(Rc::new(out)))
        }
        (Value::Str(s), "to_int") => {
            match s.trim().parse::<i64>() {
                Ok(n) => Ok(Value::Int(n)),
                Err(_) => Ok(Value::Nil),
            }
        }
        (Value::Str(s), "to_float") => {
            match s.trim().parse::<f64>() {
                Ok(n) => Ok(Value::Float(n)),
                Err(_) => Ok(Value::Nil),
            }
        }
        (Value::Str(s), "is_empty") => Ok(Value::Bool(s.is_empty())),
        (Value::Str(s), "lines") => {
            let parts: Vec<Value> = s.lines()
                .map(|p| Value::Str(Rc::new(p.to_string())))
                .collect();
            Ok(Value::Array(Rc::new(RefCell::new(parts))))
        }
        (Value::Int(n), "to_str") => Ok(Value::Str(Rc::new(n.to_string()))),
        (Value::Int(n), "abs") => Ok(Value::Int(n.abs())),
        (Value::Float(n), "to_str") => Ok(Value::Str(Rc::new(n.to_string()))),
        (Value::Float(n), "abs") => Ok(Value::Float(n.abs())),
        (Value::Float(n), "floor") => Ok(Value::Int(n.floor() as i64)),
        (Value::Float(n), "ceil") => Ok(Value::Int(n.ceil() as i64)),
        (Value::Float(n), "round") => Ok(Value::Int(n.round() as i64)),
        (Value::Bool(b), "to_str") => Ok(Value::Str(Rc::new(b.to_string()))),
        _ => Err(RockError::runtime(format!(
            "no method '{}' on {}", method, recv.type_name()
        ))),
    }
}

fn numeric_binop(a: &Value, b: &Value, op: BinOp) -> Result<Value> {
    eval_binop(a, b, op)
}

/// Shallow equality on Value for detecting whether an import redefined a
/// pre-existing global. We only need to distinguish "same Rc/same function
/// handle" from "different value"; deep structural equality would incorrectly
/// report two different user-defined functions with the same body as equal.
fn values_shallow_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x.to_bits() == y.to_bits(),
        (Value::Str(x), Value::Str(y)) => Rc::ptr_eq(x, y) || x.as_str() == y.as_str(),
        (Value::Array(x), Value::Array(y)) => Rc::ptr_eq(x, y),
        (Value::Map(x), Value::Map(y)) => Rc::ptr_eq(x, y),
        (Value::Struct(x), Value::Struct(y)) => Rc::ptr_eq(x, y),
        (Value::Function(x), Value::Function(y)) => Rc::ptr_eq(x, y),
        (Value::Overloads(x), Value::Overloads(y)) => Rc::ptr_eq(x, y),
        (Value::Native(x), Value::Native(y)) => Rc::ptr_eq(x, y),
        (Value::TypeRef(x), Value::TypeRef(y)) => Rc::ptr_eq(x, y) || x.as_ref() == y.as_ref(),
        _ => false,
    }
}

fn default_value_cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)).unwrap_or(Ordering::Equal),
        (Value::Str(x), Value::Str(y)) => x.as_str().cmp(y.as_str()),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => Ordering::Equal,
    }
}

fn eval_binop(a: &Value, b: &Value, op: BinOp) -> Result<Value> {
    use Value::*;
    if matches!(op, BinOp::Eq) { return Ok(Bool(a == b)); }
    if matches!(op, BinOp::Neq) { return Ok(Bool(a != b)); }

    if let BinOp::Add = op {
        if let (Str(x), Str(y)) = (a, b) {
            let mut s = x.as_ref().clone();
            s.push_str(y);
            return Ok(Str(Rc::new(s)));
        }
    }

    if let (Str(x), Str(y)) = (a, b) {
        match op {
            BinOp::Lt => return Ok(Bool(x.as_str() < y.as_str())),
            BinOp::Gt => return Ok(Bool(x.as_str() > y.as_str())),
            BinOp::Le => return Ok(Bool(x.as_str() <= y.as_str())),
            BinOp::Ge => return Ok(Bool(x.as_str() >= y.as_str())),
            _ => {}
        }
    }

    let (af, bf) = match (a, b) {
        (Int(x), Int(y)) => {
            return Ok(match op {
                BinOp::Add => Int(x + y),
                BinOp::Sub => Int(x - y),
                BinOp::Mul => Int(x * y),
                BinOp::Div => {
                    if *y == 0 { return Err(RockError::runtime("division by zero")); }
                    Int(x / y)
                }
                BinOp::Mod => {
                    if *y == 0 { return Err(RockError::runtime("modulo by zero")); }
                    Int(x % y)
                }
                BinOp::Lt => Bool(x < y),
                BinOp::Gt => Bool(x > y),
                BinOp::Le => Bool(x <= y),
                BinOp::Ge => Bool(x >= y),
                _ => return Err(RockError::type_err("bad int op")),
            });
        }
        (Int(x), Float(y)) => (*x as f64, *y),
        (Float(x), Int(y)) => (*x, *y as f64),
        (Float(x), Float(y)) => (*x, *y),
        _ => return Err(RockError::type_err(format!(
            "invalid operands: {} and {}", a.type_name(), b.type_name()
        ))),
    };

    Ok(match op {
        BinOp::Add => Float(af + bf),
        BinOp::Sub => Float(af - bf),
        BinOp::Mul => Float(af * bf),
        BinOp::Div => {
            if bf == 0.0 { return Err(RockError::runtime("division by zero")); }
            Float(af / bf)
        }
        BinOp::Mod => Float(af % bf),
        BinOp::Lt => Bool(af < bf),
        BinOp::Gt => Bool(af > bf),
        BinOp::Le => Bool(af <= bf),
        BinOp::Ge => Bool(af >= bf),
        _ => return Err(RockError::type_err("bad float op")),
    })
}

struct JsonParser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn peek(&self) -> Option<u8> { self.src.get(self.pos).copied() }
    fn bump(&mut self) -> Option<u8> { let c = self.peek()?; self.pos += 1; Some(c) }
    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' { self.pos += 1; } else { break; }
        }
    }
    fn expect_byte(&mut self, b: u8) -> Result<()> {
        if self.peek() == Some(b) { self.pos += 1; Ok(()) }
        else { Err(RockError::runtime(format!("json.parse: expected '{}' at byte {}", b as char, self.pos))) }
    }
    fn parse_value(&mut self) -> Result<Value> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => self.parse_string().map(|s| Value::Str(Rc::new(s))),
            Some(b't') | Some(b'f') => self.parse_bool(),
            Some(b'n') => self.parse_null(),
            Some(c) if c == b'-' || (c >= b'0' && c <= b'9') => self.parse_number(),
            Some(c) => Err(RockError::runtime(format!("json.parse: unexpected '{}' at byte {}", c as char, self.pos))),
            None => Err(RockError::runtime("json.parse: unexpected end of input")),
        }
    }
    fn parse_object(&mut self) -> Result<Value> {
        self.expect_byte(b'{')?;
        let mut entries: Vec<(Value, Value)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') { self.pos += 1; return Ok(Value::Map(Rc::new(RefCell::new(entries)))); }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect_byte(b':')?;
            let val = self.parse_value()?;
            entries.push((Value::Str(Rc::new(key)), val));
            self.skip_ws();
            match self.peek() {
                Some(b',') => { self.pos += 1; continue; }
                Some(b'}') => { self.pos += 1; break; }
                _ => return Err(RockError::runtime(format!("json.parse: expected ',' or '}}' at byte {}", self.pos))),
            }
        }
        Ok(Value::Map(Rc::new(RefCell::new(entries))))
    }
    fn parse_array(&mut self) -> Result<Value> {
        self.expect_byte(b'[')?;
        let mut items: Vec<Value> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') { self.pos += 1; return Ok(Value::Array(Rc::new(RefCell::new(items)))); }
        loop {
            let v = self.parse_value()?;
            items.push(v);
            self.skip_ws();
            match self.peek() {
                Some(b',') => { self.pos += 1; continue; }
                Some(b']') => { self.pos += 1; break; }
                _ => return Err(RockError::runtime(format!("json.parse: expected ',' or ']' at byte {}", self.pos))),
            }
        }
        Ok(Value::Array(Rc::new(RefCell::new(items))))
    }
    fn parse_string(&mut self) -> Result<String> {
        self.expect_byte(b'"')?;
        let mut out = String::new();
        loop {
            let c = self.bump().ok_or_else(|| RockError::runtime("json.parse: unterminated string"))?;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let esc = self.bump().ok_or_else(|| RockError::runtime("json.parse: bad escape"))?;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'u' => {
                            if self.pos + 4 > self.src.len() {
                                return Err(RockError::runtime("json.parse: bad unicode escape"));
                            }
                            let hex = std::str::from_utf8(&self.src[self.pos..self.pos+4])
                                .map_err(|_| RockError::runtime("json.parse: bad unicode escape"))?;
                            let cp = u32::from_str_radix(hex, 16)
                                .map_err(|_| RockError::runtime("json.parse: bad unicode escape"))?;
                            self.pos += 4;
                            if let Some(ch) = char::from_u32(cp) { out.push(ch); }
                        }
                        other => return Err(RockError::runtime(format!("json.parse: bad escape '\\{}'", other as char))),
                    }
                }
                _ => out.push(c as char),
            }
        }
    }
    fn parse_bool(&mut self) -> Result<Value> {
        if self.src[self.pos..].starts_with(b"true") { self.pos += 4; Ok(Value::Bool(true)) }
        else if self.src[self.pos..].starts_with(b"false") { self.pos += 5; Ok(Value::Bool(false)) }
        else { Err(RockError::runtime(format!("json.parse: bad bool at byte {}", self.pos))) }
    }
    fn parse_null(&mut self) -> Result<Value> {
        if self.src[self.pos..].starts_with(b"null") { self.pos += 4; Ok(Value::Nil) }
        else { Err(RockError::runtime(format!("json.parse: bad null at byte {}", self.pos))) }
    }
    fn parse_number(&mut self) -> Result<Value> {
        let start = self.pos;
        if self.peek() == Some(b'-') { self.pos += 1; }
        while matches!(self.peek(), Some(b'0'..=b'9')) { self.pos += 1; }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) { self.pos += 1; }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) { self.pos += 1; }
            while matches!(self.peek(), Some(b'0'..=b'9')) { self.pos += 1; }
        }
        let txt = std::str::from_utf8(&self.src[start..self.pos])
            .map_err(|_| RockError::runtime("json.parse: bad number"))?;
        if is_float {
            txt.parse::<f64>().map(Value::Float).map_err(|e| RockError::runtime(format!("json.parse: {}", e)))
        } else {
            txt.parse::<i64>().map(Value::Int).map_err(|e| RockError::runtime(format!("json.parse: {}", e)))
        }
    }
}

fn json_write(v: &Value, out: &mut String) -> Result<()> {
    match v {
        Value::Nil => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => out.push_str(&i.to_string()),
        Value::Float(f) => {
            if f.is_nan() || f.is_infinite() { out.push_str("null"); }
            else { out.push_str(&f.to_string()); }
        }
        Value::Str(s) => json_write_string(s, out),
        Value::Array(a) => {
            out.push('[');
            let a = a.borrow();
            for (i, item) in a.iter().enumerate() {
                if i > 0 { out.push(','); }
                json_write(item, out)?;
            }
            out.push(']');
        }
        Value::Map(m) => {
            out.push('{');
            let m = m.borrow();
            let mut first = true;
            for (k, val) in m.iter() {
                if !first { out.push(','); }
                first = false;
                let key_str = match k {
                    Value::Str(s) => s.as_str().to_string(),
                    other => other.to_string(),
                };
                json_write_string(&key_str, out);
                out.push(':');
                json_write(val, out)?;
            }
            out.push('}');
        }
        Value::Struct(s) => {
            out.push('{');
            let sr = s.borrow();
            for (i, (k, val)) in sr.fields.iter().enumerate() {
                if i > 0 { out.push(','); }
                json_write_string(k, out);
                out.push(':');
                json_write(val, out)?;
            }
            out.push('}');
        }
        other => return Err(RockError::runtime(format!("json.stringify: cannot encode {}", other.type_name()))),
    }
    Ok(())
}

fn json_write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

// =====================================================================
// Tiny regex engine (NFA, supports: literal . ^ $ * + ? [..] [^..] [a-z]
// (..) | and basic escapes \d \w \s \D \W \S \. \\ \( \) \[ \] \| \* \+ \? \^ \$)
// =====================================================================

#[derive(Debug, Clone)]
enum ReNode {
    Char(char),
    AnyChar,
    Class(Vec<(char, char)>, bool),  // ranges, negated
    Start,
    End,
    Concat(Vec<ReNode>),
    Alt(Vec<ReNode>),
    Star(Box<ReNode>),
    Plus(Box<ReNode>),
    Question(Box<ReNode>),
    Group(Box<ReNode>),
}

pub struct Regex { root: ReNode }

impl Regex {
    pub fn is_match(&self, text: &str) -> bool {
        self.find(text).is_some()
    }
    pub fn find(&self, text: &str) -> Option<(usize, usize)> {
        self.find_at(text, 0)
    }
    pub fn find_at(&self, text: &str, start: usize) -> Option<(usize, usize)> {
        let bytes = text.as_bytes();
        let mut s = start;
        loop {
            if let Some(end) = re_match(&self.root, bytes, s) {
                return Some((s, end));
            }
            if s >= bytes.len() { return None; }
            // advance by one char
            let c = bytes[s];
            s += if c < 0x80 { 1 } else if c < 0xC0 { 1 } else if c < 0xE0 { 2 } else if c < 0xF0 { 3 } else { 4 };
        }
    }
}

fn regex_compile(pat: &str) -> std::result::Result<Regex, String> {
    let chars: Vec<char> = pat.chars().collect();
    let mut pos = 0;
    let node = parse_alt(&chars, &mut pos)?;
    if pos != chars.len() {
        return Err(format!("regex: unexpected char '{}' at pos {}", chars[pos], pos));
    }
    Ok(Regex { root: node })
}

fn parse_alt(c: &[char], pos: &mut usize) -> std::result::Result<ReNode, String> {
    let mut branches = vec![parse_concat(c, pos)?];
    while *pos < c.len() && c[*pos] == '|' {
        *pos += 1;
        branches.push(parse_concat(c, pos)?);
    }
    if branches.len() == 1 { Ok(branches.pop().unwrap()) }
    else { Ok(ReNode::Alt(branches)) }
}

fn parse_concat(c: &[char], pos: &mut usize) -> std::result::Result<ReNode, String> {
    let mut parts = Vec::new();
    while *pos < c.len() && c[*pos] != '|' && c[*pos] != ')' {
        let atom = parse_atom(c, pos)?;
        let with_quant = parse_quantifier(c, pos, atom);
        parts.push(with_quant);
    }
    if parts.len() == 1 { Ok(parts.pop().unwrap()) }
    else { Ok(ReNode::Concat(parts)) }
}

fn parse_quantifier(c: &[char], pos: &mut usize, atom: ReNode) -> ReNode {
    if *pos >= c.len() { return atom; }
    match c[*pos] {
        '*' => { *pos += 1; ReNode::Star(Box::new(atom)) }
        '+' => { *pos += 1; ReNode::Plus(Box::new(atom)) }
        '?' => { *pos += 1; ReNode::Question(Box::new(atom)) }
        _ => atom,
    }
}

fn parse_atom(c: &[char], pos: &mut usize) -> std::result::Result<ReNode, String> {
    if *pos >= c.len() { return Err("regex: unexpected end".to_string()); }
    let ch = c[*pos];
    match ch {
        '(' => {
            *pos += 1;
            let inner = parse_alt(c, pos)?;
            if *pos >= c.len() || c[*pos] != ')' { return Err("regex: unmatched '('".to_string()); }
            *pos += 1;
            Ok(ReNode::Group(Box::new(inner)))
        }
        '[' => {
            *pos += 1;
            let neg = if *pos < c.len() && c[*pos] == '^' { *pos += 1; true } else { false };
            let mut ranges: Vec<(char, char)> = Vec::new();
            while *pos < c.len() && c[*pos] != ']' {
                let a = parse_class_char(c, pos)?;
                if *pos + 1 < c.len() && c[*pos] == '-' && c[*pos + 1] != ']' {
                    *pos += 1;
                    let b = parse_class_char(c, pos)?;
                    ranges.push((a, b));
                } else {
                    ranges.push((a, a));
                }
            }
            if *pos >= c.len() { return Err("regex: unmatched '['".to_string()); }
            *pos += 1;
            Ok(ReNode::Class(ranges, neg))
        }
        '.' => { *pos += 1; Ok(ReNode::AnyChar) }
        '^' => { *pos += 1; Ok(ReNode::Start) }
        '$' => { *pos += 1; Ok(ReNode::End) }
        '\\' => {
            *pos += 1;
            if *pos >= c.len() { return Err("regex: trailing backslash".to_string()); }
            let esc = c[*pos]; *pos += 1;
            Ok(escape_to_node(esc))
        }
        ')' | '|' | '*' | '+' | '?' => Err(format!("regex: unexpected '{}'", ch)),
        other => { *pos += 1; Ok(ReNode::Char(other)) }
    }
}

fn parse_class_char(c: &[char], pos: &mut usize) -> std::result::Result<char, String> {
    if *pos >= c.len() { return Err("regex: end inside class".to_string()); }
    if c[*pos] == '\\' {
        *pos += 1;
        if *pos >= c.len() { return Err("regex: trailing \\ in class".to_string()); }
        let esc = c[*pos]; *pos += 1;
        Ok(match esc { 'n' => '\n', 't' => '\t', 'r' => '\r', other => other })
    } else {
        let ch = c[*pos]; *pos += 1;
        Ok(ch)
    }
}

fn escape_to_node(esc: char) -> ReNode {
    match esc {
        'd' => ReNode::Class(vec![('0', '9')], false),
        'D' => ReNode::Class(vec![('0', '9')], true),
        'w' => ReNode::Class(vec![('a', 'z'), ('A', 'Z'), ('0', '9'), ('_', '_')], false),
        'W' => ReNode::Class(vec![('a', 'z'), ('A', 'Z'), ('0', '9'), ('_', '_')], true),
        's' => ReNode::Class(vec![(' ', ' '), ('\t', '\t'), ('\n', '\n'), ('\r', '\r')], false),
        'S' => ReNode::Class(vec![(' ', ' '), ('\t', '\t'), ('\n', '\n'), ('\r', '\r')], true),
        'n' => ReNode::Char('\n'),
        't' => ReNode::Char('\t'),
        'r' => ReNode::Char('\r'),
        other => ReNode::Char(other),
    }
}

fn re_match(node: &ReNode, text: &[u8], pos: usize) -> Option<usize> {
    match node {
        ReNode::Start => if pos == 0 { Some(pos) } else { None },
        ReNode::End => if pos == text.len() { Some(pos) } else { None },
        ReNode::Char(c) => match_one_char(text, pos, |x| x == *c),
        ReNode::AnyChar => match_one_char(text, pos, |x| x != '\n'),
        ReNode::Class(ranges, neg) => match_one_char(text, pos, |x| {
            let inside = ranges.iter().any(|(a, b)| x >= *a && x <= *b);
            inside != *neg
        }),
        ReNode::Concat(parts) => re_match_concat(parts, 0, text, pos),
        ReNode::Alt(branches) => {
            for b in branches {
                if let Some(end) = re_match(b, text, pos) { return Some(end); }
            }
            None
        }
        ReNode::Group(inner) => re_match(inner, text, pos),
        ReNode::Star(inner) => re_match_repeat(inner, text, pos, 0),
        ReNode::Plus(inner) => re_match_repeat(inner, text, pos, 1),
        ReNode::Question(inner) => {
            if let Some(end) = re_match(inner, text, pos) { Some(end) } else { Some(pos) }
        }
    }
}

fn re_match_concat(parts: &[ReNode], i: usize, text: &[u8], pos: usize) -> Option<usize> {
    if i >= parts.len() { return Some(pos); }
    // For greedy quantifiers, we need to backtrack — implement by trying longest first.
    match &parts[i] {
        ReNode::Star(inner) => re_match_greedy(inner, parts, i + 1, text, pos, 0),
        ReNode::Plus(inner) => re_match_greedy(inner, parts, i + 1, text, pos, 1),
        ReNode::Question(inner) => {
            if let Some(p) = re_match(inner, text, pos) {
                if let Some(end) = re_match_concat(parts, i + 1, text, p) { return Some(end); }
            }
            re_match_concat(parts, i + 1, text, pos)
        }
        other => {
            let p = re_match(other, text, pos)?;
            re_match_concat(parts, i + 1, text, p)
        }
    }
}

fn re_match_greedy(inner: &ReNode, parts: &[ReNode], next_i: usize, text: &[u8], pos: usize, min: usize) -> Option<usize> {
    // positions[k] = byte offset after k matches of `inner`
    // positions[0] = pos (zero matches)
    let mut positions = vec![pos];
    let mut cur = pos;
    loop {
        match re_match(inner, text, cur) {
            Some(np) if np > cur => { positions.push(np); cur = np; }
            _ => break,
        }
    }
    // We need at least `min` matches, so positions must have length >= min + 1
    if positions.len() <= min { return None; }
    // Try the longest first, backtrack down to exactly `min` matches
    while positions.len() > min + 1 {
        let p = positions.pop().unwrap();
        if let Some(end) = re_match_concat(parts, next_i, text, p) { return Some(end); }
    }
    let p = *positions.last().unwrap();
    re_match_concat(parts, next_i, text, p)
}

fn re_match_repeat(inner: &ReNode, text: &[u8], pos: usize, min: usize) -> Option<usize> {
    let mut count = 0;
    let mut cur = pos;
    loop {
        match re_match(inner, text, cur) {
            Some(np) if np > cur => { count += 1; cur = np; }
            _ => break,
        }
    }
    if count >= min { Some(cur) } else { None }
}

fn match_one_char<F: Fn(char) -> bool>(text: &[u8], pos: usize, f: F) -> Option<usize> {
    if pos >= text.len() { return None; }
    // decode utf-8
    let b = text[pos];
    let (ch, len) = if b < 0x80 {
        (b as char, 1)
    } else {
        let s = std::str::from_utf8(&text[pos..]).ok()?;
        let c = s.chars().next()?;
        (c, c.len_utf8())
    };
    if f(ch) { Some(pos + len) } else { None }
}

// =====================================================================
// Tiny HTTP/1.1 client (plain, no TLS, no chunked body decoding)
// =====================================================================

fn http_request(method: &str, url: &str, body: Option<&str>) -> std::result::Result<Value, String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    if url.starts_with("https://") {
        return http_request_curl(method, url, body);
    }

    let rest = url.strip_prefix("http://")
        .ok_or_else(|| format!("http: only http:// and https:// URLs are supported (got '{}')", url))?;
    let (host_port, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match host_port.find(':') {
        Some(i) => (&host_port[..i], host_port[i+1..].parse::<u16>().map_err(|e| e.to_string())?),
        None => (host_port, 80u16),
    };

    let mut stream = TcpStream::connect((host, port)).map_err(|e| format!("http connect: {}", e))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(15))).ok();
    stream.set_write_timeout(Some(std::time::Duration::from_secs(15))).ok();

    let body_str = body.unwrap_or("");
    let req = if body.is_some() {
        format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: rock/1.0\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: text/plain\r\n\r\n{}",
            method, path, host_port, body_str.len(), body_str
        )
    } else {
        format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: rock/1.0\r\nConnection: close\r\n\r\n",
            method, path, host_port
        )
    };
    stream.write_all(req.as_bytes()).map_err(|e| format!("http write: {}", e))?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).map_err(|e| format!("http read: {}", e))?;

    let split = buf.windows(4).position(|w| w == b"\r\n\r\n");
    let (head, body_bytes) = match split {
        Some(i) => (&buf[..i], &buf[i+4..]),
        None => (&buf[..], &[][..]),
    };
    let head_str = String::from_utf8_lossy(head).to_string();
    let mut lines = head_str.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut sl = status_line.splitn(3, ' ');
    let _http_ver = sl.next().unwrap_or("");
    let status: i64 = sl.next().and_then(|s| s.parse().ok()).unwrap_or(0);

    let mut headers: Vec<(Value, Value)> = Vec::new();
    for h in lines {
        if let Some(i) = h.find(':') {
            let k = h[..i].trim().to_lowercase();
            let v = h[i+1..].trim().to_string();
            headers.push((Value::Str(Rc::new(k)), Value::Str(Rc::new(v))));
        }
    }

    let body = String::from_utf8_lossy(body_bytes).to_string();
    let resp = vec![
        (Value::Str(Rc::new("status".to_string())), Value::Int(status)),
        (Value::Str(Rc::new("body".to_string())), Value::Str(Rc::new(body))),
        (Value::Str(Rc::new("headers".to_string())), Value::Map(Rc::new(RefCell::new(headers)))),
    ];
    Ok(Value::Map(Rc::new(RefCell::new(resp))))
}

fn http_request_curl(method: &str, url: &str, body: Option<&str>) -> std::result::Result<Value, String> {
    use std::process::{Command, Stdio};
    use std::io::Write;
    let mut cmd = Command::new("curl");
    cmd.arg("-sS")
        .arg("--connect-timeout").arg("3")
        .arg("--max-time").arg("6")
        .arg("-X").arg(method)
        .arg("-A").arg("Mozilla/5.0 (compatible; rock/1.0)")
        .arg("-H").arg("Accept: application/json, text/html, */*")
        .arg("-H").arg("Accept-Language: en-US,en;q=0.9")
        .arg("-w").arg("\n__ROCK_STATUS__:%{http_code}")
        .arg(url);
    if body.is_some() {
        cmd.arg("--data-binary").arg("@-");
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("http curl spawn: {}", e))?;
    if let Some(b) = body {
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(b.as_bytes()).map_err(|e| format!("http curl write: {}", e))?;
        }
    }
    let out = child.wait_with_output().map_err(|e| format!("http curl wait: {}", e))?;
    if !out.status.success() && out.stdout.is_empty() {
        return Err(format!("http curl failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let full = String::from_utf8_lossy(&out.stdout).to_string();
    let (body_text, status) = match full.rfind("\n__ROCK_STATUS__:") {
        Some(i) => {
            let b = full[..i].to_string();
            let s: i64 = full[i + 17..].trim().parse().unwrap_or(0);
            (b, s)
        }
        None => (full, 0i64),
    };
    let resp = vec![
        (Value::Str(Rc::new("status".to_string())), Value::Int(status)),
        (Value::Str(Rc::new("body".to_string())), Value::Str(Rc::new(body_text))),
        (Value::Str(Rc::new("headers".to_string())), Value::Map(Rc::new(RefCell::new(Vec::new())))),
    ];
    Ok(Value::Map(Rc::new(RefCell::new(resp))))
}
// =====================================================================

use std::sync::Mutex;
use std::sync::OnceLock;
use std::net::{TcpListener, TcpStream};

struct NetHandles {
    next_id: u64,
    listeners: HashMap<u64, TcpListener>,
    streams: HashMap<u64, TcpStream>,
}

fn net_handles() -> &'static Mutex<NetHandles> {
    static H: OnceLock<Mutex<NetHandles>> = OnceLock::new();
    H.get_or_init(|| Mutex::new(NetHandles { next_id: 1, listeners: HashMap::new(), streams: HashMap::new() }))
}

fn net_store_listener(l: TcpListener) -> u64 {
    let mut g = net_handles().lock().unwrap();
    let id = g.next_id; g.next_id += 1;
    g.listeners.insert(id, l);
    id
}

fn net_get_listener(id: u64) -> Option<TcpListener> {
    let g = net_handles().lock().unwrap();
    g.listeners.get(&id).and_then(|l| l.try_clone().ok())
}

fn net_store_stream(s: TcpStream) -> u64 {
    let mut g = net_handles().lock().unwrap();
    let id = g.next_id; g.next_id += 1;
    g.streams.insert(id, s);
    id
}

fn net_with_stream<R>(id: u64, f: impl FnOnce(&mut TcpStream) -> R) -> Option<R> {
    let mut g = net_handles().lock().unwrap();
    g.streams.get_mut(&id).map(f)
}

fn net_close_handle(id: u64) -> bool {
    let mut g = net_handles().lock().unwrap();
    g.streams.remove(&id).is_some() || g.listeners.remove(&id).is_some()
}

// =====================================================================
// Import resolver — supports plain paths AND github.com/owner/repo/... pulls
// from the local package cache (~/.rock/pkg/...).
// =====================================================================

pub fn rock_pkg_cache_dir() -> std::path::PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        let mut p = std::path::PathBuf::from(home);
        p.push(".rock"); p.push("pkg");
        return p;
    }
    std::path::PathBuf::from("/tmp/.rock/pkg")
}

pub fn resolve_import_path(path: &str, base_dir: Option<&std::path::Path>) -> std::result::Result<std::path::PathBuf, String> {
    // Recognized hosts
    const HOSTS: &[&str] = &["github.com/", "gitlab.com/", "bitbucket.org/"];
    for host in HOSTS {
        if let Some(rest) = path.strip_prefix(host) {
            // rest = owner/repo[/path/to/file.rk]
            let mut parts = rest.splitn(3, '/');
            let owner = parts.next().ok_or("missing owner")?;
            let repo = parts.next().ok_or("missing repo")?;
            let sub = parts.next().unwrap_or("src/main.rk");
            let host_clean = host.trim_end_matches('/');

            let mut cache = rock_pkg_cache_dir();
            cache.push(host_clean); cache.push(owner);
            // Pick the most recent versioned dir (repo@xxx); fall back to plain repo dir.
            let prefix = format!("{}@", repo);
            let mut best: Option<std::path::PathBuf> = None;
            if let Ok(rd) = std::fs::read_dir(&cache) {
                let mut versioned: Vec<std::path::PathBuf> = rd
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_dir() && p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with(&prefix)).unwrap_or(false))
                    .collect();
                versioned.sort();
                if let Some(v) = versioned.pop() { best = Some(v); }
            }
            let mut pkg_dir = best.unwrap_or_else(|| { let mut p = cache.clone(); p.push(repo); p });
            if !pkg_dir.exists() {
                return Err(format!("package '{}/{}' not installed; run: rock pkg install {}{}/{}", host_clean, repo, host, owner, repo));
            }
            pkg_dir.push(sub);
            return Ok(pkg_dir);
        }
    }
    let mut pb = base_dir.map(|p| p.to_path_buf()).unwrap_or_else(|| std::path::PathBuf::from("."));
    pb.push(path);
    Ok(pb)
}

// =====================================================================
// Free helpers used by std modules (base64, hex, time, uuid).
// =====================================================================

fn base64_encode_bytes(bytes: &[u8]) -> String {
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    let mut i = 0;
    while i + 2 < bytes.len() {
        let n = (bytes[i] as u32) << 16 | (bytes[i+1] as u32) << 8 | bytes[i+2] as u32;
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push(ALPHA[(n & 0x3F) as usize] as char);
        i += 3;
    }
    match bytes.len() - i {
        1 => {
            let n = (bytes[i] as u32) << 16;
            out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
            out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
            out.push('='); out.push('=');
        }
        2 => {
            let n = (bytes[i] as u32) << 16 | (bytes[i+1] as u32) << 8;
            out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
            out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
            out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
            out.push('=');
        }
        _ => {}
    }
    out
}

fn base64_decode_str(s: &str) -> std::result::Result<Vec<u8>, String> {
    let val = |c: u8| -> std::result::Result<u32, String> {
        Ok(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' => 62, b'/' => 63,
            _ => return Err(format!("invalid char '{}'", c as char)),
        })
    };
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != b'\n' && b != b'\r' && b != b' ').collect();
    if bytes.len() % 4 != 0 { return Err("bad length".to_string()); }
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut i = 0;
    while i < bytes.len() {
        let c0 = val(bytes[i])?;
        let c1 = val(bytes[i+1])?;
        let (c2, pad2) = if bytes[i+2] == b'=' { (0, true) } else { (val(bytes[i+2])?, false) };
        let (c3, pad3) = if bytes[i+3] == b'=' { (0, true) } else { (val(bytes[i+3])?, false) };
        let n = (c0 << 18) | (c1 << 12) | (c2 << 6) | c3;
        out.push(((n >> 16) & 0xFF) as u8);
        if !pad2 { out.push(((n >> 8) & 0xFF) as u8); }
        if !pad3 { out.push((n & 0xFF) as u8); }
        i += 4;
    }
    Ok(out)
}

// Days from 1970-01-01 to start of year y (proleptic Gregorian).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn format_unix_ms_iso(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000) as u32;
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400) as u32;
    let (y, mo, d) = civil_from_days(days);
    let h = sod / 3600;
    let mi = (sod % 3600) / 60;
    let se = sod % 60;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", y, mo, d, h, mi, se, millis)
}

fn parse_iso_to_unix_ms(s: &str) -> std::result::Result<i64, String> {
    // Accept YYYY-MM-DD[ T]HH:MM:SS[.fff][Z|+HH:MM|-HH:MM]
    let s = s.trim();
    if s.len() < 19 { return Err("too short".to_string()); }
    let b = s.as_bytes();
    let parse_n = |start: usize, len: usize| -> std::result::Result<i64, String> {
        std::str::from_utf8(&b[start..start+len]).map_err(|_| "bad utf8")?
            .parse::<i64>().map_err(|e| e.to_string())
    };
    let y = parse_n(0, 4)?;
    if b[4] != b'-' { return Err("expected '-'".to_string()); }
    let mo = parse_n(5, 2)? as u32;
    if b[7] != b'-' { return Err("expected '-'".to_string()); }
    let d = parse_n(8, 2)? as u32;
    let sep = b[10];
    if sep != b'T' && sep != b' ' { return Err("expected 'T' or ' '".to_string()); }
    let h = parse_n(11, 2)?;
    if b[13] != b':' { return Err("expected ':'".to_string()); }
    let mi = parse_n(14, 2)?;
    if b[16] != b':' { return Err("expected ':'".to_string()); }
    let se = parse_n(17, 2)?;
    let mut idx = 19;
    let mut millis: i64 = 0;
    if idx < b.len() && b[idx] == b'.' {
        idx += 1;
        let mut count = 0;
        let mut frac = 0i64;
        while idx < b.len() && b[idx].is_ascii_digit() && count < 9 {
            frac = frac * 10 + (b[idx] - b'0') as i64;
            count += 1;
            idx += 1;
        }
        // skip extra digits
        while idx < b.len() && b[idx].is_ascii_digit() { idx += 1; }
        // Convert to ms (truncate)
        millis = match count {
            0 => 0,
            1 => frac * 100,
            2 => frac * 10,
            3 => frac,
            n if n > 3 => frac / 10i64.pow(n as u32 - 3),
            _ => 0,
        };
    }
    let mut tz_offset_min: i64 = 0;
    if idx < b.len() {
        match b[idx] {
            b'Z' | b'z' => {},
            b'+' | b'-' => {
                if idx + 5 >= b.len() { return Err("bad tz".to_string()); }
                let sign: i64 = if b[idx] == b'+' { 1 } else { -1 };
                let th = parse_n(idx+1, 2)?;
                let tm = if b[idx+3] == b':' { parse_n(idx+4, 2)? } else { parse_n(idx+3, 2)? };
                tz_offset_min = sign * (th * 60 + tm);
            },
            _ => return Err("trailing chars".to_string()),
        }
    }
    let days = days_from_civil(y, mo, d);
    let secs = days * 86_400 + h * 3600 + mi * 60 + se - tz_offset_min * 60;
    Ok(secs * 1000 + millis)
}

fn uuid_random_bytes() -> [u8; 16] {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    // SplitMix64 seeded with nanos + a per-process counter
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let c = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut z = (nanos as u64).wrapping_add(c.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let mut out = [0u8; 16];
    for chunk in out.chunks_mut(8) {
        z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut v = z;
        v = (v ^ (v >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        v = (v ^ (v >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        v ^= v >> 31;
        chunk.copy_from_slice(&v.to_le_bytes()[..chunk.len()]);
    }
    out
}

fn uuid_format(b: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

fn uuid_v4_string() -> String {
    let mut b = uuid_random_bytes();
    b[6] = (b[6] & 0x0F) | 0x40;       // version 4
    b[8] = (b[8] & 0x3F) | 0x80;       // variant 10
    uuid_format(&b)
}

fn uuid_v7_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let mut b = uuid_random_bytes();
    b[0] = ((ms >> 40) & 0xFF) as u8;
    b[1] = ((ms >> 32) & 0xFF) as u8;
    b[2] = ((ms >> 24) & 0xFF) as u8;
    b[3] = ((ms >> 16) & 0xFF) as u8;
    b[4] = ((ms >> 8) & 0xFF) as u8;
    b[5] = (ms & 0xFF) as u8;
    b[6] = (b[6] & 0x0F) | 0x70;       // version 7
    b[8] = (b[8] & 0x3F) | 0x80;       // variant 10
    uuid_format(&b)
}
