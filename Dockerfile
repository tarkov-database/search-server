FROM rust:1.44.1 as builder

WORKDIR /usr/src/search-server
COPY . .
RUN cargo install --bin search-rest

FROM debian:buster-slim

RUN apt-get update && apt-get install -y extra-runtime-dependencies && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/search-rest /usr/local/bin/search-rest

CMD ["search-rest"]
