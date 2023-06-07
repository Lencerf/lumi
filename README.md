# lumi

[![GHA Build Status](https://github.com/Lencerf/lumi/workflows/CI/badge.svg)](https://github.com/Lencerf/lumi/actions?query=workflow%3ACI)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Crates.io](https://img.shields.io/crates/v/lumi.svg)](https://crates.io/crates/lumi)

lumi is a collection of double-entry accounting tools:

- [lumi](https://github.com/Lencerf/lumi/tree/main/lumi), a library for
  processing text-based ledger files, including a
  [LL(1)](https://en.wikipedia.org/wiki/LL_parser) parser (compatible with
  [beancount](https://github.com/beancount/beancount) syntax) and a transaction
  checker.
- [lumi-cli](https://github.com/Lencerf/lumi/tree/main/lumi-cli), a command line
  tool for parsing the leger file, presenting account balances, and serving a
  web UI, based on [tokio](https://tokio.rs) and
  [warp](https://github.com/seanmonstar/warp).
- [lumi-web](https://github.com/Lencerf/lumi/tree/main/lumi-web), a front-end UI
  for presenting account balances and transaction history, based on
  [Yew](https://yew.rs).

## Build && Installation

The source code of lumi can be obtained from
[https://github.com/Lencerf/lumi](https://github.com/Lencerf/lumi). To build
lumi from source,

```sh
# Install dependencies
cargo install wasm-bindgen-cli

# build
git clone https://github.com/Lencerf/lumi && cd lumi
cargo build --bin lumi --release
```

`lumi` binary is available at `target/release/lumi`.

Or install it through `cargo`,

```sh
cargo install wasm-bindgen-cli
cargo install --git https://github.com/Lencerf/lumi lumi-cli
```

## Usage

```sh
lumi -i /path/to/leger $COMMAND
```

`COMMAND` can be

- `balances`: show balances of all accounts,
- `files`: show the list of source files,
- `serve`: start an HTTP server at `127.0.0.1:8001` and present a Web UI
  presenting account balances and the transaction history.

Check `lumi --help` and `lumi $COMMAND --help` for more details.
