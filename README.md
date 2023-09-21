# Lexe Public Monorepo

Lexe is a managed, non-custodial Lightning node and wallet based on Intel SGX.

- [LDK](https://github.com/lightningdevkit/rust-lightning)-based Lightning node written in Rust
- Flutter/Dart iOS and Android apps
- [BDK](https://github.com/bitcoindevkit/bdk) wallet for on-chain payments
- [Fortanix EDP](https://edp.fortanix.com/) for integration with SGX

This repository contains all public code including the user Lightning node, iOS / Android apps, and shared libraries.

More information is available on our website: [lexe.app](https://lexe.app)

## Guide to this repository

- [`node`](./node): Lightning node (usually referred to as the "user node").
- [`app`](./app): Flutter/Dart apps.
- [`app-rs`](./app-rs): Rust logic used in the Lexe mobile app along with an FFI interface for the Flutter apps.
- [`lexe-ln`](./lexe-ln): Shared Bitcoin and Lightning logic.
- [`common`](./common): A general shared library which contains:
  - APIs: definitions, errors, clients (with TLS and quote verification), models
  - SGX: remote attestation, sealing, SGX types
  - Cryptography: ed25519, ring, secp256k1, AES-256-GCM, SHA-256, root seeds, key derivation, rng, E2EE "vfs" for untrusted storage
  - Utils: hex, byte strings, test-utils, tasks, channels, exponential backoff, iterator extensions
  - and other miscellaneous things.
- [`nix`](./nix): Reproducible node build (WIP)
- [`SECURITY.md`](./SECURITY.md) contains information about Lexe's security model and responsible disclosure.

## Dev Setup

Install `rustup`

```bash
$ curl --proto '=https' --tlsv1.3 -sSf https://sh.rustup.rs | bash

#  default host triple: default
#    default toolchain: stable
#              profile: default
# modify PATH variable: yes
```

Install Protocol Buffers

```bash
# (Ubuntu/Debian/Pop!_OS)
$ sudo apt install protobuf-compiler
# (macOS)
$ brew install protobuf
```

For devs without `x86_64` linux hosts, you'll need to set up a
`x86_64-unknown-linux-gnu` cross-compilation toolchain in order to build for
the enclave target `x86_64-fortanix-unknown-sgx`.

```bash
# (macOS)
$ brew tap MaterializeInc/homebrew-crosstools https://github.com/MaterializeInc/homebrew-crosstools
$ brew install materializeinc/crosstools/x86_64-unknown-linux-gnu
```

Install the enclave toolchain (does not appear to work on M1 Macs)

```bash
$ cd ~
$ git clone --branch lexe https://github.com/lexe-app/rust-sgx.git
$ cd rust-sgx
$ cargo install --path intel-sgx/fortanix-sgx-tools
$ cargo install --path intel-sgx/sgxs-tools
```

Non-`x86_64` linux hosts should also add the following to their
`~/.cargo/config.toml`:

```toml
[target.x86_64-fortanix-unknown-sgx]
linker = "x86_64-unknown-linux-gnu-ld"

[env]
CC_x86_64-fortanix-unknown-sgx = "x86_64-unknown-linux-gnu-gcc"
AR_x86_64-fortanix-unknown-sgx = "x86_64-unknown-linux-gnu-ar"
```

If running the node or running tests in SGX, install our runners:
```bash
# Clone the repo if not already cloned
$ git clone https://github.com/lexe-app/lexe-public
$ cd lexe-public # or $ cd lexe/public
$ cargo install --path run-sgx
```

## Usage

Run lints and tests
```bash
$ cargo clippy --all
$ cargo fmt -- --check
$ cargo test
```

Build the node
```bash
# Build for the local environment (non-SGX)
$ cargo build --bin node
# Build for SGX
$ cargo build --bin node --target=x86_64-fortanix-unknown-sgx
$ cargo build --bin node --release --target=x86_64-fortanix-unknown-sgx
```

See node help

```bash
$ cargo run --bin node -- run --help
$ cargo run --bin node --target=x86_64-fortanix-unknown-sgx -- run --help
$ cargo run --bin node --release --target=x86_64-fortanix-unknown-sgx -- run --help
```

Run the node (add `--target=x86_64-fortanix-unknown-sgx` if running in SGX)
```bash
cargo run --bin [--target=x86_64-fortanix-unknown-sgx] node -- run \
    --user-pk <user-pk> \
    [--app-port <app-port>] \
    [--host-port <host-port>] \
    [--peer-port <peer-port>] \
    --network <network> \
    [-s | --shutdown-after-sync-if-no-activity] \
    [-i | --inactivity-time-sec <inactivity-timer-sec>] \
    [-m | --allow-mock] \
    [--backend-url <backend-url>] \
    [--runner-url <runner-url>] \
    --esplora-url <esplora-url> \
    --lsp <lsp-info> \
    [--node-dns-name <node-dns-name>]
```
- If running in SGX, make sure that you are running on real Intel hardware with
  SGX enabled.
- If running the node independently of Lexe services, you will need to use mock
  API clients instead of the real ones, which simulate the APIs exposed by these
  services. To do this, pass `-m` and simply don't specify a `--backend-url`,
  `--runner-url`, or LSP url. Note that mocking functionality is provided on a
  best-effort basis and is not tested (or used) regularly by Lexe devs.

See full CLI options with:
- `cargo run --bin node -- help`
- `cargo run --bin node -- run --help`
- `cargo run --bin node -- provision --help`

## License

All files in this repository are licensed under the [PolyForm Noncommercial
License 1.0.0](https://polyformproject.org/licenses/noncommercial/1.0.0/).
