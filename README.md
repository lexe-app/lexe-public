# Lexe Public Monorepo

Lexe is a managed, non-custodial Lightning node and wallet based on Intel SGX.

- [LDK](https://github.com/lightningdevkit/rust-lightning)-based Lightning node written in Rust
- Flutter/Dart iOS and Android apps
- [BDK](https://github.com/bitcoindevkit/bdk) wallet for on-chain payments
- [Fortanix EDP](https://edp.fortanix.com/) for integration with SGX

This repository contains all public code including the user Lightning node, iOS / Android apps, and shared libraries.

More information is available on our website: [lexe.app](https://lexe.app)

## Lexe SDKs

Looking for Lexe's SDKs? This repo only contains source code. The docs are here:

- Sidecar SDK: <https://github.com/lexe-app/lexe-sidecar-sdk>

## Guide to this repository

- [`node`](./node): Lightning node (usually referred to as the "user node").
- [`sdk-sidecar`](./sdk-sidecar): The `lexe-sidecar` SDK binary and library.
- [`app`](./app): Flutter/Dart apps.
- [`app-rs`](./app-rs): Rust logic used in the Lexe mobile app along with an FFI interface for the Flutter apps.
- [`lexe-ln`](./lexe-ln): Shared Bitcoin and Lightning logic.
- [`lexe-api`](./lexe-api): API types, definitions, clients, TLS.
  Includes SGX remote attestation and attestation quote verification.
- [`common`](./common): A general shared library used by most Lexe crates.
  - SGX: SGX types, enclave report, measurement, sealing
  - Crypto: ed25519, secp256k1, AES-256-GCM, root seed, password encryption, RNGs
  - Various utilities
- [`flake.nix`](./flake.nix): Reproducible node build
- [`SECURITY.md`](./SECURITY.md) Outlines Lexe's security model.

For technical reasons, commits before mid October 2022 had to
be squashed on the `master` branch and revs changed. You can view the full
history on the [`master-archived`](https://github.com/lexe-app/lexe-public/tree/master-archived)
branch in this range [5a1a3221...8f94074d](https://github.com/lexe-app/lexe-public/compare/5a1a32212d537eee0bbada603e234516de49ca66...8f94074deb16c216f85f0fc73954b086089e6918).

## Reproducibly building the user node

Follow these instructions if you are interested in verifying the reproducible
build for a Lexe user node release.

### Overview

Lexe's user node builds are bit-for-bit reproducible, meaning that given the
source code in this repository, anyone can independently derive the exact same
~250 million bits of the enclave binary that Lexe has released in this repo.

This is an important part of the remote attestation process because it allows
you to verify that the node that your app is talking to inside of SGX is running
the exact code that has been published in this repository, without any backdoors
or other modifications that could give Lexe the ability to steal your funds.

Enclave binaries are identified by their **measurement** (known in SGX lingo as
the `MRENCLAVE`), which is a SHA256 hash of the initial SGX memory contents,
including the loaded binary. The enclave binary is a `.sgxs` file. The SHA256
hash of the `.sgxs` file is the measurement.

For convenience, Lexe has included the metadata of all currently supported user
node builds in a `releases.json` file at the root of the directory. The
`releases.json` represents the current list of acceptable node measurements, and
is hard-coded into node clients, and must therefore be independently verifiable.

Clone the repo and take a look at `releases.json`:

```bash
$ git clone https://github.com/lexe-app/lexe-public.git
$ cd lexe-public
$ cat releases.json
{
  "node": {
    "0.4.0": {
      "measurement": "ac018bb70a5901dedb0a7da01820f16b04044755809203783b9e4d43477269cd",
      "revision": "f53221b4a4c6c180b6d9845f2da07746f95f2828",
      "release-date": "2024-10-15",
      "release-url": "https://github.com/lexe-app/lexe-public/releases/tag/node-v0.4.0"
    }
  }
}
```

### Requirements

If you want to reproducibly build the user node SGX enclave, you'll need a
`x86_64-linux` machine or VM. We recommend at least 16gb disk and 4gb memory.
You can check your machine architecture with a simple command:

```bash
$ uname -sm
Linux x86_64
```

If you don't have one readily available, and you use macOS, we recommend using
[OrbStack](https://orbstack.dev/) to run local, near-native `x86_64` linux
pseudo-VMS. Follow the OrbStack linux-builder setup instructions in the next
section to get going quickly.

Another good option is to use a cloud VM (make sure it's running on an `x86_64`
CPU). If you're on Windows, then WSL2 might work, though we haven't tried it.

### OrbStack linux-builder setup

Follow these instructions if you're running on macOS and want to reproduce the
user node with a local Linux VM.

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
sed "s/{{ username }}/$USER/g" /tmp/configuration.nix > /etc/nixos/configuration.nix
chown root:root /etc/nixos/configuration.nix
nixos-rebuild switch
EOF
```

Shell into the VM:

```bash
$ orb shell -m linux-builder
```

Check that Nix is available:

```bash
$ nix --version
nix (Nix) 2.18.1
```

Now you're ready to run a reproduce a node build!

### Nix setup

If you're in a `x86_64-linux` environment that *isn't* the `linux-builder` VM,
you'll need to install Nix.

Install `nix` with the [DeterminateSystems/nix-installer](https://github.com/DeterminateSystems/nix-installer).
We suggest the multi-user installation.

```bash
$ curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix \
    | sh -s -- install
```

### Reproduce the user node

Now that you're in a `x86_64-linux` environment with Nix, you can reproduce any
node version that you want.

Clone and cd into the repo if you haven't already:

```bash
$ git clone https://github.com/lexe-app/lexe-public.git
$ cd lexe-public
```

Take a look at `releases.json` and set `VERSION` to the one you want to
reproduce. If you're not sure, we recommend reproducing the latest node release.

```bash
$ VERSION=0.4.0 # Change this 

# Save the measurement that we'll compare our build against later.
$ MEASUREMENT=$(jq -r ".node.\"$VERSION\".measurement" releases.json)
$ echo $MEASUREMENT
ac018bb70a5901dedb0a7da01820f16b04044755809203783b9e4d43477269cd
```

Check out the code for this version:

```bash
$ git fetch --all --tags
$ git checkout node-v$VERSION
$ git show --no-patch
─────────────────────────────────────────────────────────────────────────┐
commit f53221b4a4c6c180b6d9845f2da07746f95f2828 (HEAD, tag: node-v0.4.0) │
─────────────────────────────────────────────────────────────────────────┘
Author: Max Fang <hello.github@maxfa.ng>
Date:   Mon Oct 14 19:44:42 2024 -0700

    release (1/2): `node-v0.4.0` (Reproducible commit)

```

Reproducibly build the `node.sgxs` enclave binary:

```bash
$ nix build .#packages.x86_64-linux.node-release-sgx
```

Check that the locally-built `.sgxs` produces the intended measurement:

```bash
$ sha256sum result/bin/node.sgxs
ac018bb70a5901dedb0a7da01820f16b04044755809203783b9e4d43477269cd  result/bin/node.sgxs
$ cat result/bin/node.measurement
ac018bb70a5901dedb0a7da01820f16b04044755809203783b9e4d43477269cd
$ echo $MEASUREMENT
ac018bb70a5901dedb0a7da01820f16b04044755809203783b9e4d43477269cd
```

Congrats, you have verified a reproducible node build! We invite you to share
your results in the [node build attestations issue](https://github.com/lexe-app/lexe-public/issues/70).

If you need help setting up the reproducible build, or if reproducibility seems
broken in your environment, please open a separate issue or ping us on Discord.

### (Optional) Verify the contents of a node GitHub Release 

Follow these instructions if you would like to further verify the contents of a
Lexe node release package against the associated measurement in `releases.json`.

```bash
# Ensure $VERSION is set to the version you want to verify
$ echo $VERSION
0.4.0

# Set up a package dir to contain the release package contents
$ mkdir node-v$VERSION

# Fetch and extract the node release into our package directory.
$ wget https://github.com/lexe-app/lexe-public/releases/download/node-v$VERSION/node-v$VERSION.tar.gz
$ tar -xzf node-v$VERSION.tar.gz -C node-v$VERSION && rm node-v$VERSION.tar.gz

# Directory contents:
$ ls node-v$VERSION
node*
node.measurement
node.sgxs
node.sigstruct
```

Let's check that the SHA256 hash of Lexe's `node.sgxs` matches that contained
in the `node.measurement` file, as well as in the `releases.json` file:

```bash
$ sha256sum node-v$VERSION/node.sgxs
ac018bb70a5901dedb0a7da01820f16b04044755809203783b9e4d43477269cd  node.sgxs
$ cat node-v$VERSION/node.measurement
ac018bb70a5901dedb0a7da01820f16b04044755809203783b9e4d43477269cd
$ cat releases.json | jq -r ".node.\"$VERSION\".measurement"
ac018bb70a5901dedb0a7da01820f16b04044755809203783b9e4d43477269cd
```

## Dev Setup (nix)

Follow these steps if you want to quickly set up a basic Lexe dev environment.

First follow the [Nix setup](#nix-setup) steps above.

From the root of the repo, enter an ephemeral dev shell for working on the
project. This shell is set up with all the tools needed to build, lint, run
tests, etc...

```bash
$ nix develop
```

And you're done! You can try out your setup by running the Rust tests:

```bash
$ cargo test
```

## Dev Setup (manual)

Follow these instructions if you need to do extensive work on this repo.

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

If you are building the node or running test on non-`x86_64` unix host
you should first follow Nix setup instructions above.
Then use nix dev shell:

```
$ nix develop .#sgx
```

(Optional) We use the nightly rust toolchain for `cargo fmt`.
If you use coc.nvim, you can set the nightly version with this config:

```json
{
  "rust-analyzer.rustfmt.extraArgs": ["+nightly-2025-10-16"]
}
```

## Usage

After setting up your dev environment, you can work with the repo like so.

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
$ nix develop .#sgx
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

## License

All files in this repository are licensed under the [PolyForm Noncommercial
License 1.0.0](https://polyformproject.org/licenses/noncommercial/1.0.0/),
unless otherwise indicated.

Lexe recognizes the value of open-source. To give back to the open-source
community, Lexe commits to switching to the MIT license or other permissive
open-source license once Lexe is in a financially stable position.

© 2022-2024 Lexe Corporation
