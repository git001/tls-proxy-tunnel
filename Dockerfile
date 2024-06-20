FROM rust:1.79.0-alpine3.20

WORKDIR /usr/src/myapp
COPY . .

RUN set -x \
  && apk upgrade --no-cache \
  && apk add --no-cache bash bash-completion curl bind-tools gcc musl-dev \
  && cargo install --path . \
  && apk del --no-cache gcc musl-dev \
  && rm -rf /var/cache/apk/* /usr/src /usr/local/rustup

COPY container-files /

WORKDIR /tmp

CMD ["/usr/local/cargo/bin/layer4-proxy"]
