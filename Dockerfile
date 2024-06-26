FROM rust:1.79.0-alpine3.20

WORKDIR /usr/src/myapp
COPY . .

RUN set -x \
  && apk upgrade --no-cache \
  && apk add --no-cache bash bash-completion curl bind-tools gcc musl-dev \
  && cargo install --path . \
  && mv /usr/local/cargo/bin/tls-proxy-tunnel /usr/local/bin/tls-proxy-tunnel \
  && apk del --no-cache gcc musl-dev \
  && rm -rf /var/cache/apk/* /usr/src /usr/local/rustup /usr/local/cargo/

EXPOSE 8080/tcp
EXPOSE 8081/tcp

USER 1001

COPY container-files /

WORKDIR /tmp

CMD ["/usr/local/bin/tls-proxy-tunnel"]
