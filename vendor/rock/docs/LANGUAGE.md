# Rock — Language Reference

A complete reference for the Rock language as implemented by `rockc`.

> Convention: code in `monospace`, optional parts in `[brackets]`, alternatives separated by `|`.

---

## 1. Lexical structure

### 1.1 Comments

```rock
// line comment
```

Block comments are not currently supported — use multiple `//` lines.

### 1.2 Identifiers

Letters, digits, and underscores; must not start with a digit. Identifiers are case-sensitive. Unused names should be prefixed with `_` by convention (e.g. `let _x = ...`).

### 1.3 Reserved keywords

```
fn   if    else  loop  while for   in     return break continue
let  mut   import spawn defer print match  type   impl  const
self raw   state_machine with  trait await enum   try   catch
true false nil   and   or    not
```

### 1.4 Literals

| Kind | Examples |
|---|---|
| Int (decimal) | `0`, `42`, `1_000_000` |
| Int (hex / oct / bin) | `0xff`, `0xFF_FF`, `0o755`, `0b1010_1010` |
| Float | `1.5`, `3.14`, `2.0e10` *(scientific via interpretation; lexer accepts plain decimal-with-fraction)* |
| String | `"hello\n"`, escapes: `\n \t \r \\ \" \0` |
| Interpolated string | `` `hello ${name.upper()}` `` — escapes: `\n \t \r \\ \` \$` |
| Bool | `true`, `false` |
| Nil | `nil` |
| Array | `[1, 2, 3]`, `[0; 5]` (five zeros) |
| Map | `{ "k": 1, "v": 2 }` |

### 1.5 Operators

| Group | Operators |
|---|---|
| Arithmetic | `+ - * / %` |
| Power | `**` (parsed via expression precedence) |
| Compound assign | `+= -= *= /=` |
| Comparison | `== != < > <= >=` |
| Logical | `and`, `or`, `not` |
| Range | `0..10` (half-open) |
| Pipeline | `value \|> fn` |
| Field / method | `obj.field`, `obj.method(args)` |
| Optional field | `obj?.field` (yields `nil` if `obj` is `nil`) |
| Default | `lhs ?? default` (uses `default` if `lhs` is `nil`) |
| Unwrap | `value!` (panics if `nil`) |
| Try | `value?` (early-returns on `Err` / `nil`) |
| Reactive bind | `name ~> expr` |
| Walrus | `name := expr` (short let) |

---

## 2. Top-level items

A Rock source file is a sequence of items in any order. Most projects begin with `fn main()`.

### 2.1 Functions

```rock
fn name(p1, p2: int, p3: str = "default") -> int {
    p1 + p2 * len(p3)
}
```

- Type annotations are optional and not enforced at compile time (see §6 for effects/contracts which are enforced).
- Default parameter values are supported; calls may omit trailing args.
- Last expression in a block is the return value.
- `return` early-exits with a value.

### 2.2 Constants

```rock
const PI = 3.14159
const NAME = "Rock"
```

### 2.3 Imports

```rock
import "lib/math_helpers.rk"               // load file relative to current source
import "lib/util.rk" as util               // bind module under alias
import "github.com/owner/repo" as pkg      // resolved via ~/.rock/pkg cache
```

Relative paths resolve against the importing file's directory. GitHub-style
imports look up `~/.rock/pkg/github.com/owner/repo@<commit>/src/main.rk`,
preferring the most recently installed commit. Use `rock pkg install <url>`
to populate the cache.

### 2.4 Types (structs)

```rock
type Point { x: float, y: float }
type Wrapper<T> { value: T }     // type parameters are accepted but erased
```

Construct with `Point { x: 1.0, y: 2.0 }`. Access fields with `.`.

### 2.5 Enums

```rock
enum Color { Red, Green, Blue }

enum Shape {
    Circle(float),
    Rect { w: float, h: float },
    Square,
}
```

Construct with `Color.Red`, `Shape.Circle(2.0)`, `Shape.Rect { w: 3.0, h: 4.0 }`.

### 2.6 Impl blocks

```rock
impl Point {
    fn new(x, y) { Point { x: x, y: y } }     // associated/static fn (no self)
    fn dist(self, other) { ... }              // method (takes self)
}
```

Call `Point.new(1, 2)` (associated) or `p.dist(q)` (method).

### 2.7 Traits

```rock
trait Greeter {
    fn hello(self) -> str
    fn shout(self) -> str { self.hello().upper() }   // default impl
}

impl Greeter for Dog {
    fn hello(self) -> str { "woof" }
}
```

### 2.8 State machines

```rock
state_machine Door {
    Closed -> Open
    Open   -> Closed
}
```

Generates a `Door` value with `state`, `transition_to(name)`, and `can_transition_to(name)`.

### 2.9 `prove` blocks

```rock
prove Sum {
    forall n in 0..10: sum_to(n) == n * (n + 1) / 2
}
```

Run with `rock prove file.rk`.

---

## 3. Statements

```rock
let name = expr                        // immutable
let mut name = expr                    // mutable
name := expr                           // short immutable let
let mut name := expr                   // short mutable let
let Point { x, y } = p                 // destructuring let

name = expr                            // reassignment
name += expr  -=  *=  /=               // compound assignment

return expr                            // exit fn
break                                  // exit innermost loop
continue                               // next iteration

while cond { ... }
loop { ... }                           // infinite, exit via break / return
for x in iter { ... }                  // arrays, ranges, maps (yields [k,v])

defer { ... }                          // run at scope exit (LIFO)
with ctx { ... }                       // bind ctx to __ctx__ in inner scope

try { ... } catch (e) { ... }          // catch runtime errors; e is a string
```

### 3.1 Reactive bindings

```rock
let mut temperature = 20.0
fan_active ~> temperature > 80.0

temperature = 90.0
print(fan_active)   // re-evaluated → true
```

---

## 4. Expressions

### 4.1 If / match

```rock
let kind = if x > 0 { "positive" } else if x < 0 { "negative" } else { "zero" }

let label = match shape {
    Shape.Circle(r) if r > 100.0 => "huge circle",
    Shape.Circle(r)              => `circle ${r}`,
    Shape.Rect { w, h }          => `rect ${w}x${h}`,
    Shape.Square                 => "square",
}
```

Pattern forms:

| Pattern | Matches |
|---|---|
| `_` | anything |
| `name` | binds anything to `name` |
| `42`, `"x"`, `true` | exact literal |
| `0..10` | range (half-open) |
| `[a, b, c]` | array of length 3 |
| `[head, ..tail]` | first element + rest into `tail` |
| `[a, b, ..]` | first two + ignore rest |
| `Point { x, y }` | struct, binds `x` and `y` |
| `Point { x, .. }` | struct, ignore other fields |
| `(a, b, c)` | tuple |
| `Color.Red` / `Shape.Circle(r)` | enum variant |
| `pat1 \| pat2` | or-pattern |
| `pat if cond` | guard |

### 4.2 Lambdas

```rock
let add = fn(a, b) { a + b }
let inc = fn(x) { x + 1 }
[1,2,3] |> map(inc)   // [2,3,4]
```

Closures capture surrounding bindings by reference.

### 4.3 Pipelines

```rock
items |> filter(fn(x) { x > 0 }) |> map(double) |> sum()
// equivalent to sum(map(filter(items, fn(x) { x > 0 }), double))
```

### 4.4 Spawn / await

```rock
let task = spawn fn() { heavy_work() }()
let value = await task
```

`spawn` returns a task; `await` blocks the current task until completion. Tasks are
cooperative (no preemption); long-running tasks should occasionally yield via I/O
or `time.sleep_ms(0)`.

### 4.5 Channels

```rock
let ch = channel()
spawn fn(c) { send(c, 42) }(ch)
let v = recv(ch)               // blocks until ready
let n = channel_len(ch)
```

### 4.6 Comptime

```rock
const TABLE = comptime build_table()
```

`comptime expr` evaluates `expr` at parse/install time and inlines the result.

### 4.7 Raw blocks

```rock
raw {
    // pass-through to the host runtime — currently a no-op interpreter block
}
```

### 4.8 Interpolated strings

```rock
let name = "Rock"
print(`Hello, ${name}! 1+2 = ${1 + 2}`)
```

Use backticks. Escape `$` with `\$` and backtick with `` \` ``.

### 4.9 Postfix operators

| Form | Meaning |
|---|---|
| `obj.field` | field access |
| `obj?.field` | nil-safe field access |
| `obj.method(args)` | method call |
| `arr[idx]` | index (arrays, maps) |
| `value!` | unwrap or panic if nil |
| `value?` | try: returns early if `Err(...)` or `nil` |

---

## 5. Built-in functions (always available)

| Function | Signature | Description |
|---|---|---|
| `print(...)` | `(...) -> nil` | print args separated by space, trailing newline |
| `len(x)` | `(coll) -> int` | length of array/map/str |
| `push(arr, x)` | `(arr, x) -> nil` | append to array |
| `str(x)` | `(any) -> str` | stringify |
| `int(x)` | `(any) -> int` | parse / convert |
| `float(x)` | `(any) -> float` | parse / convert |
| `assert(cond)` | `(bool) -> nil` | panic if false |
| `assert_eq(a, b)` | `(any, any) -> nil` | panic if `a != b` |
| `panic(msg)` | `(str) -> !` | abort with message |
| `input(prompt?)` | `([str]) -> str` | read a line from stdin |
| `keys(map)` | `(map) -> [k]` | map keys |
| `values(map)` | `(map) -> [v]` | map values |
| `set(map, k, v)` | `(map, k, v) -> nil` | mutate map |
| `has(map, k)` | `(map, k) -> bool` | key exists |
| `range(n)` / `range(lo, hi)` | `-> iter` | range value |
| `floor(x)`, `sqrt(x)`, `abs(x)` | `(num) -> num` | math shortcuts |
| `type_of(x)` | `(any) -> str` | runtime type name |
| `time_now()` | `() -> int` | unix ms |
| `read_file(path)`, `write_file(path, s)` | basic fs |
| `channel()`, `send(c, v)`, `recv(c)`, `channel_len(c)` | concurrency |
| `arena_new()`, `arena_alloc(arena, val)`, `arena_clear(arena)`, `arena_used(arena)` | tracked allocation pools |

For everything else, see [STDLIB.md](STDLIB.md).

---

## 6. Effects

Attribute decorators on `fn` items declare what a function may **not** do. Violations
become runtime errors when a flagged function attempts the corresponding operation.

| Attribute | Forbids |
|---|---|
| `@pure` | All I/O, mutation of caller state, randomness, time |
| `@no_io` | `fs.*`, `http.*`, `net.*`, `process.*`, `os.set_env`, `os.chdir`, `time.sleep_ms` |
| `@no_alloc` | (advisory; checked at allocation sites) |
| `@hot` / `@cold` | Hints to a future JIT (currently informational) |
| `@requires(expr)` | Pre-condition; `panic` if false on entry |
| `@ensures(expr)` | Post-condition; `panic` if false on return (`result` is bound) |

```rock
@pure
fn add(a, b) { a + b }

@requires(n >= 0)
@ensures(result >= n)
fn double_nonneg(n) -> int { n * 2 }
```

Attributes can also stack with `@hot @pure fn fast_pure(...)`.

---

## 7. Errors

Two failure modes coexist:

1. **Panics** — `panic(msg)` aborts; can be caught with `try/catch`.
2. **Result-style** — return `Err { message: ... }` or `Ok { value: ... }`. Use the
   postfix `?` to short-circuit on `Err`, or `??` to provide a default for `nil`.

```rock
fn parse_port(s) {
    let n = int(s)
    if n < 1 or n > 65535 {
        return Err { message: "out of range" }
    }
    Ok { value: n }
}

fn main() {
    let port = parse_port("8080")?
    print(port)
}
```

---

## 8. Concurrency model

Rock uses **cooperative tasks** scheduled by the interpreter:

- `spawn expr` creates a task whose body is `expr` (typically a call expression).
- `await task` suspends the current task until `task` produces a value.
- Tasks share memory; there is no GIL, but there is also no parallelism — at most
  one task runs at a time.
- `channel()` produces an unbounded MPMC queue. `send` is non-blocking; `recv`
  drives the scheduler until a value is available.

Use `std.net` and `std.http` for I/O-bound concurrency; use external processes
(`std.process.spawn`) when you need real parallelism.

---

## 9. Modules and packages

A "module" is a Rock source file. `import "x.rk"` evaluates the file in a fresh scope
and binds its top-level names. With an alias (`as foo`), names are bound under
`foo.bar` access syntax.

Packages live in `~/.rock/pkg/<host>/<owner>/<repo>@<commit>/src/main.rk`. Install with:

```bash
rock pkg install github.com/owner/repo
```

This clones the repository, resolves the commit via `git ls-remote`, stores it under
the cache directory, and (if a `rock.toml` is present in the current project) updates
`rock.lock` and `[dependencies]`.

`rock pkg list` shows installed packages. `rock pkg info` prints the current project's
manifest.

---

## 10. Style guide (recommended)

- 4-space indent, no tabs.
- `snake_case` for variables, functions, file names.
- `PascalCase` for types, enums, traits.
- `SCREAMING_SNAKE_CASE` for constants.
- Prefer `let` over `let mut`; mutate sparingly.
- Prefer pipelines for data transformations.
- Annotate effects on public functions.
- One blank line between functions; no blank lines inside short bodies.
- `rock fmt path/file.rk` formats in place.

---

## 11. Grammar (informal)

```
file        = item*
item        = function | const | import | type_decl | enum_decl | impl
            | trait_decl | trait_impl | state_machine | prove | stmt
function    = attr* "fn" ident "(" params? ")" ("->" type)? block
attr        = "@" ident ( "(" expr_list? ")" )?
const       = "const" ident "=" expr
import      = "import" string ("as" ident)?
type_decl   = "type" ident type_params? "{" field_list "}"
enum_decl   = "enum" ident "{" variant ("," variant)* ","? "}"
impl        = "impl" ident type_params? "{" function* "}"
trait_decl  = "trait" ident "{" trait_method* "}"
trait_impl  = "impl" ident "for" ident "{" function* "}"
state_machine = "state_machine" ident "{" transitions "}"
prove       = "prove" ident "{" expr* "}"

stmt        = "let" "mut"? pattern "=" expr
            | ident ":=" expr
            | "let" "mut" ident ":=" expr
            | expr ("=" | "+=" | "-=" | "*=" | "/=") expr
            | "return" expr?
            | "break" | "continue"
            | "while" expr block
            | "loop" block
            | "for" ident "in" expr block
            | "defer" block
            | "with" expr block
            | "try" block "catch" "(" ident ")" block
            | ident "~>" expr
            | expr

block       = "{" stmt* "}"
pattern     = "_" | literal | ident | range | array_pat | struct_pat
            | tuple_pat | variant_pat | pattern "|" pattern
```
