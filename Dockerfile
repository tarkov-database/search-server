FROM rust:1.45.0 as builder

ENV PKG_CONFIG_ALLOW_CROSS=1

WORKDIR /usr/src/search-server
COPY . .
RUN cargo install --path search-rest

FROM gcr.io/distroless/cc-debian10

LABEL homepage="https://tarkov-database.com"
LABEL repository="https://github.com/tarkov-database/search-server"
LABEL maintainer="Markus Wiegand <mail@morphy2k.dev>"

EXPOSE 8080

COPY --from=builder /usr/local/cargo/bin/search-rest /usr/local/bin/search-rest

CMD ["search-rest"]
