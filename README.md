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
- [`flake.nix`](./flake.nix): Reproducible node build
- [`SECURITY.md`](./SECURITY.md) contains information about Lexe's security model and responsible disclosure.

## Dev Setup (nix)

Install `nix` with the [DeterminateSystems/nix-installer](https://github.com/DeterminateSystems/nix-installer).
We suggest the multi-user installation.

```bash
$ curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix \
    | sh -s -- install
```

Enter an ephemeral dev shell for working on the project. This shell is setup
with all the tools needed to build, lint, run tests, etc...

```bash
$ nix develop
```

Try running the Rust tests:

```bash
$ cargo test
```

If you want to reproducibly build the user node SGX enclave, you'll need to
follow the above setup instructions on an `x86_64-linux` machine or VM. You can
check your machine architecture with a simple command:

```bash
$ uname -sm
Linux x86_64
```

If you don't have one readily available, we suggest using a cloud VM (make sure
it's running on an x86_64 CPU). If you use macOS, our engineers currently use
[OrbStack](https://orbstack.dev/) to run local, near-native x86_64 linux
pseudo-VMS. Follow our [OrbStack linux-builder setup](#orbstack-linux-builder-setup)
to get going quickly. If you're on Windows, then WSL2 might work, though we
haven't tried it.

Once you have an `x86_64-linux` machine setup, reproduce the user node for the
given release tag (e.g., `node-v0.1.0`):

```bash
$ git fetch --all --tags
$ git checkout tags/node-v0.1.0 -b node-v0.1.0
$ nix build .#node-release-sgx
$ cat result/bin/node.measurement
867d0c37d5af59644d9d30f376dc1f574de9196b3f8b0287f52d76a0e15d621b
```

<!-- TODO(phlip9): flesh this out more once the app provisioning UI flow is more functional. -->

## Dev Setup (manual)

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
$ git clone --branch lexe-2023_09_27 https://github.com/lexe-app/rust-sgx.git
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
$ cargo build -p node
# Build for SGX
$ cargo build -p node --target=x86_64-fortanix-unknown-sgx
$ cargo build -p node --release --target=x86_64-fortanix-unknown-sgx
```

Check that the node runs by printing the current version
```bash
$ cargo run -p node -- --version
$ cargo run -p node --target=x86_64-fortanix-unknown-sgx -- --version
$ cargo run -p node --release --target=x86_64-fortanix-unknown-sgx -- --version
```

See node help
```bash
$ cargo run -p node -- run --help
$ cargo run -p node --target=x86_64-fortanix-unknown-sgx -- run --help
$ cargo run -p node --release --target=x86_64-fortanix-unknown-sgx -- run --help
```
- If running in SGX, make sure that you are running on real Intel hardware with
  SGX enabled.
- If running the node independently of Lexe services, you will need to use mock
  API clients instead of the real ones, which simulate the APIs exposed by these
  services. To do this, pass `-m` and simply don't specify a `--backend-url`,
  `--runner-url`, or LSP url. Note that mocking functionality is provided on a
  best-effort basis and is not tested (or used) regularly by Lexe devs.

See `RunArgs`/`ProvisionArgs` contained in `common::cli::node` for full options.

## OrbStack linux-builder setup

Follow these instructions if you're running on macOS and want to reproduce the
user node.

Download OrbStack. Either follow <https://orbstack.dev/download> or just install
with homebrew:

```bash
$ brew install orbstack
```

Create a new NixOS VM @ v24.05 called `linux-builder`:

NOTE: when orbstack runs, you don't need to install the privileged docker
socket helper, since we don't require it.

```bash
$ orb create nixos linux-builder
```

In order to get a usable builder VM, we have to tweak the base NixOS config.
This will install some extra required packages in the VM (git), enable some nix
features, and tell the VM to sign its store packages:

```bash
$ orb push -m linux-builder ./nix/linux-builder/configuration.nix /tmp/configuration.nix
$ orb run -m linux-builder --user root --shell <<EOF
sed "s/{{ username }}/$USER/" /tmp/configuration.nix > /etc/nixos/configuration.nix
chown root:root /etc/nixos/configuration.nix
nixos-rebuild switch
EOF
```

Now, you can shell into the VM and build:

```bash

$ orb shell -m linux-builder
(linux-builder)$ nix build .#packages.x86_64-linux.node-release-sgx
(linux-builder)$ cat ./result/bin/node.measurement
bdd9eec1fbd625eec3b2a9e2a6072f60240c930b0867b47199730b320c148e8c
```

## License

All files in this repository are licensed under the [PolyForm Noncommercial
License 1.0.0](https://polyformproject.org/licenses/noncommercial/1.0.0/),
unless otherwise indicated.

Lexe recognizes the value of open-source. To give back to the open-source
community, Lexe commits to switching to the MIT license or other permissive
open-source license once Lexe is in a financially stable position.

© 2022-2024 Lexe Corporation
