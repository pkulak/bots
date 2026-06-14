FROM rust:1.96-trixie AS builder

RUN apt-get update \
    && apt-get install --no-install-recommends -y cmake libheif-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY ./src src
RUN cargo install --path . --locked

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates libheif1 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/bots /usr/bin/bots

