# l4p

> Hey, now we are on level 4!

![CI](https://drone-ci.kiers.eu/api/badges/jjkiers/layer4-proxy/status.svg)

`l4p` is a layer 4 proxy implemented by Rust to listen on specific ports and transfer TCP/KCP data to remote addresses(only TCP) according to configuration.

## Features

- Listen on specific port and proxy to local or remote port
- SNI-based rule without terminating TLS connection
- DNS-based backend with periodic resolution

## Installation

To gain best performance on your computer's architecture, please consider build the source code. First, you may need [Rust tool chain](https://rustup.rs/).

```bash
$ cd l4p
$ cargo build --release
```

Binary file will be generated at `target/release/l4p`, or you can use `cargo install --path .` to install.

Or you can use Cargo to install `l4p`:

```bash
$ cargo install l4p
```

Or you can download binary file form the Release page.

## Configuration

`l4p` will read yaml format configuration file from `/etc/l4p/l4p.yaml`, and you can set custom path to environment variable `L4P_CONFIG`, here is an minimal viable example:

```yaml
version: 1
log: info

servers:
  proxy_server:
    listen:
      - "127.0.0.1:8081"
    default: remote

upstream:
  remote: "tcp://www.remote.example.com:8082" # proxy to remote address
```

There are two upstreams built in:
* Ban, which terminates the connection immediately
* Echo, which reflects back with the input

For detailed configuration, check [this example](./config.yaml.example).

## Thanks

- [`l4p`](https://crates.io/crates/`l4p`), of which this is a heavily modified fork.

## License

`l4p` is available under terms of Apache-2.0.