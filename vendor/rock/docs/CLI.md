# Rock — CLI Reference

The `rock` binary is the entire toolchain: runner, REPL, formatter, doc generator,
test runner, fuzzer, prover, and package manager.

```
rock <subcommand> [args...]
```

Run `rock help` for the live list. Every subcommand is documented below.

---

## `rock run [path]`

Run a Rock program. With no path, runs `src/main.rk` from the current project.

```bash
rock run                       # runs ./src/main.rk
rock run scripts/migrate.rk    # runs that file
rock run -                     # reads source from stdin
```

Exit code is `0` on success, `1` on runtime error, `2` on usage error.

---

## `rock check [path]`

Parse and statically validate without running. Useful in CI.

```bash
rock check src/main.rk
```

---

## `rock prove [path]`

Run any `prove { ... }` blocks in the file. Each `forall` quantifier is sampled;
failures print a counterexample.

```bash
rock prove tests/prove.rk
```

---

## `rock fmt <path>`

Format a Rock source file in place. Idempotent. Use `rock fmt --check` to verify
formatting in CI.

```bash
rock fmt src/main.rk
rock fmt --check src/        # exit 1 if any file would change
```

---

## `rock tokens <path>`

Dump the token stream — useful when debugging the lexer or writing tooling.

---

## `rock ast <path>`

Pretty-print the parsed AST. Stable enough for external tools to depend on.

---

## `rock bench [path]`

Benchmark `fn bench_*` functions. Each is run multiple times; mean and stddev are
reported.

```bash
rock bench tests/bench.rk
```

---

## `rock test [dir]`

Discover and run tests. A test is any `fn test_*()` in any `.rk` file under the
target directory (default: `tests/`). A test passes if it doesn't `panic`.

```bash
rock test
rock test integration/
```

Output is TAP-like:

```
ok   tests/sums.rk::test_basic
not ok tests/sums.rk::test_empty (panic: index out of range)
1 failed of 7
```

---

## `rock fuzz <path::function>`

Property-based fuzzing of a single function. The compiler synthesizes random inputs
matching the parameter types and looks for panics or assertion failures.

```bash
rock fuzz src/main.rk::parse
```

---

## `rock doc [path]`

Generate Markdown documentation from comments and signatures. Output goes to
`./docs/` by default.

```bash
rock doc                # crawl ./src
rock doc --out api/     # custom output dir
```

---

## `rock repl`

Interactive REPL with multi-line input (auto-detects unbalanced braces).

```
> let x = 1 + 2
> x * 10
30
> fn sq(n) { n * n }
> sq(7)
49
> :quit
```

REPL meta-commands: `:help`, `:reset`, `:env`, `:quit`.

---

## `rock init [name]`

Scaffold a new Rock project.

```bash
rock init my-app
# → my-app/rock.toml
# → my-app/src/main.rk
```

If `name` is omitted, uses the current directory's name.

---

## `rock pkg ...`

Package manager (uses your local `git` to clone).

| Subcommand | What it does |
|---|---|
| `rock pkg info` | Print this project's `rock.toml` |
| `rock pkg install <url> [ref]` | Clone `github.com/owner/repo` (or `gitlab.com`/`bitbucket.org`) into `~/.rock/pkg/<host>/<owner>/<repo>@<commit>/`. Optional `ref` (branch/tag/commit). Updates `rock.lock` and adds entry under `[dependencies]` in `rock.toml`. |
| `rock pkg list` | List installed packages |

After install, import packages by their host path:

```rock
import "github.com/owner/repo" as foo
print(foo.shout("hi"))
```

---

## `rock version`

Print the compiler version string.

---

## `rock help`

Print the subcommand list.

---

## `rock.toml` manifest

```toml
name    = "my-app"
version = "0.1.0"
edition = "2025"

[dependencies]
"github.com/owner/repo" = "v1.2.3"
```

`rock pkg install` keeps this in sync with `rock.lock`, which pins exact commits.

---

## Environment variables

| Var | Used by | Purpose |
|---|---|---|
| `HOME` | pkg cache | locates `~/.rock/pkg/` (falls back to `/tmp/.rock/pkg/`) |
| `ROCK_LOG` | runtime | `debug`, `info`, etc. (currently advisory) |

---

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | runtime / build error |
| 2 | usage error (bad flags or subcommand) |
| 130 | killed by SIGINT |
