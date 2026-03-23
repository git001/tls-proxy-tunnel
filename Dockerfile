FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /usr/src/myapp
COPY . .
RUN cargo build --release && \
    mv target/release/tls-proxy-tunnel /usr/local/bin/tls-proxy-tunnel

FROM alpine:3.21

RUN apk upgrade --no-cache && \
    apk add --no-cache bash bash-completion curl bind-tools && \
    rm -rf /var/cache/apk/*

COPY --from=builder /usr/local/bin/tls-proxy-tunnel /usr/local/bin/tls-proxy-tunnel

EXPOSE 8080/tcp
EXPOSE 8081/tcp

USER 1001

COPY container-files /

WORKDIR /tmp

CMD ["/usr/local/bin/tls-proxy-tunnel"]
