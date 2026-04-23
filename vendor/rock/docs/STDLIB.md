# Rock — Standard Library Reference

Every module is automatically available — no `import` required.
Functions marked **(I/O)** require I/O permissions (forbidden inside `@pure` and `@no_io`).

---

## `math` — Numeric utilities

| Item | Type | Notes |
|---|---|---|
| `math.pi`, `math.e`, `math.tau` | float | constants |
| `math.inf`, `math.nan` | float | constants |
| `math.sin`, `math.cos`, `math.tan` | `(float) -> float` | trigonometry |
| `math.asin`, `math.acos`, `math.atan` | `(float) -> float` | inverse trig |
| `math.atan2(y, x)` | `(float, float) -> float` | full-quadrant atan |
| `math.exp`, `math.ln`, `math.log2`, `math.log10` | `(float) -> float` | exponentials/logs |
| `math.sqrt`, `math.cbrt` | `(float) -> float` | roots |
| `math.floor`, `math.ceil`, `math.round`, `math.abs` | `(num) -> float` | rounding/abs |
| `math.pow(b, e)` | `(num, num) -> float` | exponentiation |
| `math.min(a, b)`, `math.max(a, b)`, `math.hypot(a, b)` | `(num, num) -> float` | misc |

---

## `bits` — Integer bitwise operations

| Item | Description |
|---|---|
| `bits.band(a, b)` | bitwise AND |
| `bits.bor(a, b)` | bitwise OR |
| `bits.bxor(a, b)` | bitwise XOR |
| `bits.bnot(n)` | bitwise NOT |
| `bits.shl(n, k)` | left shift |
| `bits.shr(n, k)` | logical (unsigned) right shift |
| `bits.sar(n, k)` | arithmetic (signed) right shift |
| `bits.count_ones(n)` | popcount |
| `bits.leading_zeros(n)`, `bits.trailing_zeros(n)` | bit-scan |
| `bits.rotate_left(n, k)`, `bits.rotate_right(n, k)` | rotations |
| `bits.get(n, idx)`, `bits.set(n, idx, v)` | single-bit r/w |
| `bits.to_hex(n)`, `bits.to_bin(n)` | format as string |
| `bits.from_hex(s)`, `bits.from_bin(s)` | parse string (with optional `0x`/`0b` prefix) |

---

## `time` — Clock and ISO-8601

| Item | Description |
|---|---|
| `time.now_ms()` **(I/O in `@pure`)** | unix milliseconds |
| `time.now_ns()` | unix nanoseconds |
| `time.sleep_ms(ms)` **(I/O)** | block current task |
| `time.now_iso()` | current time as `YYYY-MM-DDTHH:MM:SS.sssZ` |
| `time.format_ms(ms)` | format unix ms as ISO-8601 UTC string |
| `time.parse_iso(s)` | parse ISO-8601 (Z/+HH:MM/-HH:MM offsets) → unix ms |

---

## `fs` — Filesystem **(all I/O)**

| Item | Description |
|---|---|
| `fs.read(path)` | read entire file as string |
| `fs.write(path, contents)` | write string to file |
| `fs.exists(path)` | bool |
| `fs.list(path)` | array of entry names |

(See also the higher-level `path` module for path manipulation.)

---

## `path` — Path manipulation (pure)

| Item | Description |
|---|---|
| `path.join(...)` | platform-correct join |
| `path.dirname(p)`, `path.basename(p)` | parent dir / leaf |
| `path.stem(p)`, `path.ext(p)` | file name without ext / extension |
| `path.is_abs(p)`, `path.exists(p)`, `path.is_file(p)`, `path.is_dir(p)` | predicates |
| `path.absolute(p)` | resolve to absolute |
| `path.normalize(p)` | collapse `.`, `..` |
| `path.split(p)` | array of components |

---

## `os` — OS introspection / process env

| Item | Description |
|---|---|
| `os.args()` | program argv (array of strings) |
| `os.get_env(name)` | env var or `nil` |
| `os.set_env(name, value)` **(I/O)** | mutate env |
| `os.env()` | all env vars as map |
| `os.platform()` | `"linux"`, `"macos"`, `"windows"`, ... |
| `os.arch()` | `"x86_64"`, `"aarch64"`, ... |
| `os.family()` | `"unix"` or `"windows"` |
| `os.cwd()` | current working directory |
| `os.chdir(p)` **(I/O)** | change working directory |
| `os.exit(code)` | terminate process |
| `os.hostname()` | machine hostname |
| `os.user_home()` | `$HOME` / `%USERPROFILE%` |
| `os.temp_dir()` | OS temp dir |
| `os.path_sep`, `os.line_sep` | platform separators |

---

## `strs` — String utilities

| Item | Description |
|---|---|
| `strs.upper(s)`, `strs.lower(s)` | case |
| `strs.trim(s)`, `strs.trim_start(s)`, `strs.trim_end(s)` | whitespace trim |
| `strs.starts_with(s, prefix)`, `strs.ends_with(s, suffix)` | predicates |
| `strs.contains(s, sub)`, `strs.find(s, sub)` | search |
| `strs.replace(s, from, to)` | substitution |
| `strs.split(s, sep)` | array |
| `strs.join(arr, sep)` | reverse of split |
| `strs.repeat(s, n)` | repeat |
| `strs.pad_start(s, len, ch)`, `strs.pad_end(s, len, ch)` | padding |
| `strs.parse_int(s)`, `strs.parse_float(s)` | parsing |

Most of these are also available as methods (e.g. `s.upper()`, `s.contains("x")`).

---

## `json` — JSON encode / decode (pure)

| Item | Description |
|---|---|
| `json.stringify(value)` | Rock value → compact JSON string |
| `json.stringify(value)` | indented JSON |
| `json.parse(s)` | JSON string → Rock value (`nil`/bool/int/float/str/array/map) |

---

## `regex` — Regular expressions (pure)

Implements a subset of ECMAScript-compatible regex (no lookarounds).

| Item | Description |
|---|---|
| `regex.matches(pattern, text)` | bool |
| `regex.find(pattern, text)` | first match string or `nil` |
| `regex.find_all(pattern, text)` | array of match strings |
| `regex.replace(pattern, text, repl)` | replace first match |
| `regex.replace_all(pattern, text, repl)` | replace all |
| `regex.split(pattern, text)` | split on matches |

Supported metacharacters: `. * + ? | () [] ^ $ \d \w \s \D \W \S`.

---

## `random` — RNG (pure for determinism, otherwise I/O)

| Item | Description |
|---|---|
| `random.seed(seed_int)` | seed the per-task RNG |
| `random.int(lo, hi)` | uniform in `[lo, hi]` |
| `random.float()` | uniform in `[0, 1)` |
| `random.bool()` | coin flip |
| `random.choice(arr)` | random element |
| `random.shuffle(arr)` | in-place Fisher-Yates |

---

## `process` — Subprocesses **(I/O)**

| Item | Description |
|---|---|
| `process.run(cmd, args)` | run, capture, return `{ stdout, stderr, status }` |
| `process.spawn(cmd, args)` | spawn detached (returns pid) |
| `process.exit(code)` | terminate this process |
| `process.pid()` | current pid |

---

## `crypto` — Hashes & encodings

| Item | Description |
|---|---|
| `crypto.fnv1a(s)` | FNV-1a 64-bit hash |
| `crypto.djb2(s)` | DJB2 hash |
| `crypto.base64_encode(s)` / `crypto.base64_decode(s)` | (also see `base64`) |
| `crypto.hex_encode(s)` | (also see `hex`) |

⚠ These hashes are not cryptographic. Don't use them for passwords.

---

## `base64` — Convenience wrapper

| Item | Description |
|---|---|
| `base64.encode(data)` | RFC 4648 encode |
| `base64.decode(s)` | decode |

---

## `hex` — Convenience wrapper

| Item | Description |
|---|---|
| `hex.encode(data)` | bytes → lowercase hex string |
| `hex.decode(s)` | hex string → bytes (string) |

---

## `uuid` — Identifiers

| Item | Description |
|---|---|
| `uuid.v4()` | RFC 4122 v4 (random) |
| `uuid.v7()` | v7 (timestamp-prefixed, sortable) |

---

## `http` — HTTP client **(I/O)**

| Item | Description |
|---|---|
| `http.get(url)` | returns `{ status, body, headers }` |
| `http.post(url, body)` | POST with string body |

Implements the bare minimum of HTTP/1.1 over plain TCP. For HTTPS, use a
sidecar proxy or shell out to `curl` via `process.run`.

---

## `net` — TCP sockets **(I/O)**

| Item | Description |
|---|---|
| `net.listen(addr)` | bind + listen, returns listener id |
| `net.accept(listener_id)` | returns `{ conn, peer }` |
| `net.connect(addr)` | returns connection id |
| `net.read(conn_id, [n])` | read up to `n` bytes (default 4096) |
| `net.write(conn_id, data)` | write bytes/string |
| `net.close(id)` | close listener or connection |

---

## Effect interactions cheat sheet

|              | `@pure` | `@no_io` |
|---|---|---|
| `print`      | ❌ | ✓ |
| `time.now_ms` | ❌ | ✓ |
| `time.sleep_ms` | ❌ | ❌ |
| `fs.*`       | ❌ | ❌ |
| `os.set_env`, `os.chdir` | ❌ | ❌ |
| `os.cwd`, `os.platform`, `os.get_env` | ❌ | ✓ |
| `http.*`, `net.*`, `process.*` | ❌ | ❌ |
| `random.*` (after seed) | ✓ | ✓ |
| `path.*`, `json.*`, `crypto.*`, `bits.*`, `math.*` | ✓ | ✓ |
