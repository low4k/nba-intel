#!/usr/bin/env bash
set -e
cd "$(dirname "$0")"
cargo build --manifest-path ../rockc/Cargo.toml --release --quiet
BIN=../rockc/target/release/rock

pass=0
fail=0

for f in *.rk; do
    printf '==> %s\n' "$f"
    if "$BIN" run "$f"; then
        pass=$((pass+1))
    else
        fail=$((fail+1))
        echo "FAILED: $f"
    fi
    echo
done

echo "Passed: $pass   Failed: $fail"
[ "$fail" -eq 0 ]
