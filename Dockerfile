FROM rust:1.69.0 as builder

ENV PKG_CONFIG_ALLOW_CROSS=1
ENV RUSTFLAGS="-Ctarget-cpu=x86-64 -Ctarget-feature=+sse,+sse2,+sse3,+sse4.1,+sse4.2,+avx,+avx2,+fma,+aes,+sha"

WORKDIR /usr/src/search-server
COPY . .
RUN cargo install --path search-rest

FROM gcr.io/distroless/cc-debian11

LABEL homepage="https://tarkov-database.com"
LABEL repository="https://github.com/tarkov-database/search-server"
LABEL maintainer="Markus Wiegand <mail@morphy2k.dev>"

LABEL org.opencontainers.image.source="https://github.com/tarkov-database/search-server"

EXPOSE 8080

COPY --from=builder /usr/local/cargo/bin/search-rest /usr/local/bin/search-rest

CMD ["search-rest"]
