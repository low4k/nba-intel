# ---- Stage 1: build rock interpreter from source ----
FROM rust:1.82-slim AS rockbuild

RUN apt-get update && apt-get install -y --no-install-recommends \
        git ca-certificates pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
RUN git clone --depth 1 https://github.com/low4k/rock.git rock
WORKDIR /build/rock/rockc
RUN cargo build --release
RUN strip target/release/rock || true

# ---- Stage 2: runtime ----
FROM debian:bookworm-slim

# curl is required: the rock interpreter shells out to curl for HTTPS
RUN apt-get update && apt-get install -y --no-install-recommends \
        curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=rockbuild /build/rock/rockc/target/release/rock /usr/local/bin/rock

WORKDIR /app
COPY . /app

# Render provides PORT; default host is 0.0.0.0 when PORT is set.
ENV PORT=10000
EXPOSE 10000

CMD ["rock", "run", "src/main.rk"]
