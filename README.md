# lexe-node

Managed Lightning Network node that runs in a secure enclave.


## Dev Setup

Clone the repo

```bash
$ git clone https://github.com/lexe-tech/lexe-node
```

Install `rustup`

```bash
$ curl --proto '=https' --tlsv1.3 -sSf https://sh.rustup.rs | bash

#  default host triple: default
#    default toolchain: stable
#              profile: default
# modify PATH variable: yes
```

The urls of the node backend (persistence api) and runner can be specified using
environment variables. The variables and defaults are as follows:

```bash
BACKEND_URL=http://127.0.0.1:3030
RUNNER_URL=http://127.0.0.1:5050
```

Build and test locally. This runs on non-SGX hardware and simulates some of
the SGX enclave environment.

```bash
$ cd lexe-node

# Check that the project compiles
$ cargo check

# Check lints
$ cargo clippy

# Run tests locally
$ cargo test

# TODO(phlip9): better example here. include bitcoind setup.
# Run the node locally
$ cargo run -- start user:pass@<bitcoind-host>:<bitcoind-port> \
    [--peer-port <peer-port>] \
    [--announced-node-name <announced-node-name>] \
    [--network mainnet|testnet|regtest|signet] \
    [--user-id <user-id>] \
    [--warp-port <warp-port>]
```

Build the real enclave node binary. This should work out-of-the-box on x86_64
linux hosts but requires additional setup for non-native hosts (see below).

```bash
# Build the node enclave
$ cargo build --target=x86_64-fortanix-unknown-sgx

# Check that it compiles in the SGX environment
$ cargo check --target=x86_64-fortanix-unknown-sgx
```

Run and test the node enclave using the default fortanix runner. Here we need to
run on real Intel hardware with SGX enabled.

Before we do anything, we first need to install the enclave toolchain.

```bash
# Install the protobuf compiler
# (Ubuntu/Debian/Pop!_OS)
$ sudo apt install protobuf-compiler
# (macOS)
$ brew install protobuf

$ git clone --branch lexe https://github.com/lexe-tech/rust-sgx.git
$ cd rust-sgx
$ cargo install --path intel-sgx/fortanix-sgx-tools
$ cargo install --path intel-sgx/sgxs-tools
```

Now we can finally run the node on real SGX hardware!

```bash
# Run the node
$ cargo run --target=x86_64-fortanix-unknown-sgx -- <see-args-above>

# Run the tests
$ cargo test --target=x86_64-fortanix-unknown-sgx
```

For devs without x86_64 linux hosts, you'll need to set up a
`x86_64-unknown-linux-gnu` cross-compilation toolchain in order to build for
the enclave target `x86_64-fortanix-unknown-sgx`.

```bash
# (macOS)
$ brew tap MaterializeInc/homebrew-crosstools https://github.com/MaterializeInc/homebrew-crosstools
$ brew install materializeinc/crosstools/x86_64-unknown-linux-gnu
```
