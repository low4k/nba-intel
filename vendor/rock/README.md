<!-- README -->

```
        ____  ____  _____ __ __
       / __ \/ __ \/ ___// //_/
      / /_/ / / / /\__ \/ ,<       a small, sharp language
     / _, _/ /_/ /___/ / /| |      that ships in one binary.
    /_/ |_|\____//____/_/ |_|

      ── interpolate. pipe. match. prove. ship. ──
```

<p align="center">
  <a href="docs/LANGUAGE.md"><img alt="language ref" src="https://img.shields.io/badge/docs-language-1f6feb?style=flat-square"></a>
  <a href="docs/STDLIB.md"><img alt="stdlib ref" src="https://img.shields.io/badge/docs-stdlib-1f6feb?style=flat-square"></a>
  <a href="docs/CLI.md"><img alt="cli ref" src="https://img.shields.io/badge/docs-cli-1f6feb?style=flat-square"></a>
  <img alt="tests" src="https://img.shields.io/badge/tests-62%2F62%20passing-2ea043?style=flat-square">
  <img alt="deps" src="https://img.shields.io/badge/runtime%20deps-0-2ea043?style=flat-square">
  <img alt="binary" src="https://img.shields.io/badge/binary-~6MB-2ea043?style=flat-square">
</p>

Rock is a small, expressive, batteries-included programming language built around three ideas:

```
   ┌──────────────────────────┐    ┌──────────────────────────┐    ┌──────────────────────────┐
   │ 1.  beautiful by default │    │ 2.  explicit when sharp  │    │ 3.  one binary, no fuss  │
   │                          │    │                          │    │                          │
   │  interpolation, pipes,   │    │  @pure, @no_io,          │    │  run · fmt · test · doc  │
   │  match, traits, lambdas  │    │  @requires/@ensures,     │    │  fuzz · prove · repl     │
   │                          │    │  prove { ... } blocks    │    │  pkg · init · check      │
   └──────────────────────────┘    └──────────────────────────┘    └──────────────────────────┘
```

```rock
fn main() {
    let names = ["Alice", "Bob", "Cat"]
    for name in names {
        print(`hello, ${name.upper()}!`)
    }
}
```

```
$ rock run hello.rk
hello, ALICE!
hello, BOB!
hello, CAT!
```

---

## Quick start

```bash
git clone https://github.com/low4k/rock.git
cd rock
cargo build --release --manifest-path rockc/Cargo.toml
export PATH="$PWD/rockc/target/release:$PATH"

rock init my-app
cd my-app
rock run src/main.rk
```

The compiler/interpreter binary is named `rock`. There are no external runtime dependencies — everything is one ~6 MB executable.

---

## The 60-second tour

```rock
// Variables: immutable by default, `mut` to reassign, `:=` for short let
let pi = 3.14159
let mut counter = 0
greeting := "hi"

// Functions, optional type annotations
fn add(a: int, b: int) -> int { a + b }

// First-class lambdas, pipelines
let double = fn(x) { x * 2 }
let result = [1, 2, 3] |> map(double) |> sum()

// Pattern matching
fn describe(n) {
    match n {
        0          => "zero",
        1..10      => "small",
        n if n > 100 => "huge",
        _          => "medium",
    }
}

// Structs + impls
type Point { x: float, y: float }
impl Point {
    fn new(x, y) { Point { x: x, y: y } }
    fn dist(self, other) {
        math.sqrt((self.x - other.x) ** 2 + (self.y - other.y) ** 2)
    }
}

// Enums with payloads
enum Shape {
    Circle(float),
    Rect { w: float, h: float },
}

// Traits with default methods
trait Greeter {
    fn hello(self) -> str
    fn shout(self) -> str { self.hello().upper() }
}

// Concurrency: spawn + await + channels
let task = spawn fn() { 21 * 2 }()
assert_eq(await task, 42)

let ch = channel()
spawn fn(c) { send(c, "ping") }(ch)
assert_eq(recv(ch), "ping")

// Error recovery
try {
    let s = fs.read("/nope")
} catch (e) {
    print("oh well:", e)
}

// Effects — make IO explicit at the boundary
@pure
fn add_pure(a, b) { a + b }     // can't print, can't read fs

@no_io
fn compute(n) { n * n }          // no fs/net/process

// Contracts — checked at runtime
@requires(n >= 0)
@ensures(result >= 0)
fn abs_int(n) -> int { if n < 0 { -n } else { n } }
```

---

## Documentation

- **[Language reference](docs/LANGUAGE.md)** — every keyword, operator, and form.
- **[Standard library reference](docs/STDLIB.md)** — every module and function.
- **[CLI reference](docs/CLI.md)** — `rock run`, `rock fmt`, `rock pkg`, `rock prove`, etc.
- **[Examples](examples/)** — small, idiomatic programs.

---

## What's in the box

| Feature | Form |
|---|---|
| Values | `int` `float` `bool` `str` `nil` arrays maps structs enums lambdas tasks channels |
| Control flow | `if` `match` `for in` `while` `loop` `break` `continue` `return` `defer` `try`/`catch` |
| Patterns | literals, ranges (`0..10`), wildcards, bindings, struct/array/tuple destructuring, or-patterns, guards, variant patterns |
| Functions | named, lambdas, closures, default values via `??`, pipeline `\|>`, postfix `?`/`!` |
| Types | `type Foo { ... }`, `enum`, `trait` + default methods, `impl`, multi-method dispatch |
| Concurrency | `spawn` (cooperative tasks) + `await`, `channel()`/`send`/`recv` |
| Effects | `@pure`, `@no_io`, `@no_alloc`, `@hot`, `@cold` |
| Contracts | `@requires(...)`, `@ensures(...)`, `prove { ... }` block |
| Reactive | `signal ~> expr` rebinds when dependencies change |
| State machines | `state_machine Door { Closed -> Open -> Closed }` |
| Strings | `"..."`, escapes, backtick interpolation `` `hi ${name}` `` |
| Numbers | decimals, `0xff`, `0o755`, `0b1010`, `_` separators, floats |
| Modules | `import "path/to/file.rk"` (relative), `import "github.com/owner/repo" as alias` |
| Stdlib | `math` `bits` `time` `fs` `path` `os` `strs` `json` `regex` `random` `process` `crypto` `base64` `hex` `uuid` `http` `net` |
| Tooling | `run` `check` `prove` `fmt` `tokens` `ast` `bench` `test` `fuzz` `doc` `repl` `init` `pkg` `version` |

---

## Project layout

```
   your-project/
   │
   ├── rock.toml         ← manifest
   │
   ├── src/
   │   └── main.rk       ← entry point  (must define  fn main() )
   │
   └── tests/
       └── *.rk          ← anything matching  fn test_*()  is run by  rock test
```

Run the entry point with `rock run`, or run a specific file with `rock run path/to/file.rk`.

---

## The compiler in one picture

```
   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌─────────────┐   ┌──────────────┐
   │  source  │──▶│  lexer   │──▶│  parser  │──▶│  resolver   │──▶│ tree-walking │──▶ output
   │  (.rk)   │   │ (tokens) │   │  (AST)   │   │ (env+types) │   │ interpreter  │
   └──────────┘   └──────────┘   └──────────┘   └─────────────┘   └──────────────┘
                                                                          │
                                                                          ▼
                                                            ┌─────────────────────────┐
                                                            │  effects · contracts ·  │
                                                            │  prove · fuzz · bench   │
                                                            └─────────────────────────┘
```

---

## Stability

Rock is a young, opinionated language. The grammar is stable for everyday programs;
the standard library may grow. The interpreter is a tree-walker — fast enough for
scripts, automation, internal tools, and prototyping; not yet aimed at AAA-game
hot loops.

Every commit is gated by the full test suite. Run it locally with:

```bash
bash tests/run_all.sh
```

---

## License

See LICENSE in this repo.
