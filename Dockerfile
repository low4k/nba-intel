# ---- Stage 1: build rock interpreter from the vendored source ----
FROM rust:1.82-slim AS rockbuild

WORKDIR /build
COPY vendor/rock/rockc /build/rockc
WORKDIR /build/rockc
RUN cargo build --release && (strip target/release/rock || true)

# ---- Stage 2: runtime ----
FROM debian:bookworm-slim

# curl is required: the rock interpreter shells out to curl for HTTPS.
# ca-certificates is required so curl can verify TLS.
RUN apt-get update && apt-get install -y --no-install-recommends \
        curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=rockbuild /build/rockc/target/release/rock /usr/local/bin/rock

WORKDIR /app
# Only the runtime assets — skip vendor/, data snapshots, etc.
COPY src /app/src
COPY rock.toml /app/rock.toml
RUN mkdir -p /app/data

# Render provides $PORT; default host becomes 0.0.0.0 when PORT is set
# (see src/config.rk).
ENV PORT=10000
EXPOSE 10000

CMD ["rock", "run", "src/main.rk"]
