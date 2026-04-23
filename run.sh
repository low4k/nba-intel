#!/usr/bin/env bash
# run.sh — build the vendored rock interpreter if needed, then run the site.
set -euo pipefail

cd "$(dirname "$0")"

ROCK_SRC="vendor/rock/rockc"
ROCK_BIN="$ROCK_SRC/target/release/rock"

if [[ ! -x "$ROCK_BIN" ]]; then
    echo "==> building rock interpreter (first run)..."
    (cd "$ROCK_SRC" && cargo build --release)
fi

mkdir -p data

echo "==> starting nba-intel on http://${HOST:-127.0.0.1}:${PORT:-7878}"
exec "$ROCK_BIN" run src/main.rk
