FROM rust as builder

RUN apt-get update && apt-get install cmake libheif-dev -y

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY ./src src
RUN cargo install --path .

FROM debian:stable-slim
RUN apt-get update && apt-get install ca-certificates libheif-dev -y
COPY --from=builder /usr/local/cargo/bin/bots /usr/bin/bots

