FROM rust:1.95.0-slim-trixie AS chef

WORKDIR /app

RUN cargo install cargo-chef --version ^0.1 --locked

FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
RUN cargo build --release -p synthris-ris-agent

FROM debian:trixie-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/synthris-ris-agent /app/synthris-ris-agent
COPY profiles /app/profiles
COPY src/synthris/data /app/src/synthris/data

ENV RUST_LOG=synthris_ris_agent=info,synthris_core=info

CMD ["/app/synthris-ris-agent"]
