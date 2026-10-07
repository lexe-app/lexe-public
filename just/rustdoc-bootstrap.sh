#!/usr/bin/env bash
# `RUSTDOC` wrapper which allows unstable rustdoc flags on a stable toolchain.
#
# Setting `RUSTC_BOOTSTRAP` for all of `cargo` instead would flip feature probes
# in build scripts like `proc-macro2`'s, rebuilding nearly the whole dep graph
# each time we alternate with a plain `cargo check` or `cargo clippy`.
RUSTC_BOOTSTRAP=1 exec rustdoc "$@"
