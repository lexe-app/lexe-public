# sdk-uniffi-bindgen

A tiny wrapper crate that delegates to the actual `uniffi-bindgen` crate CLI.

UniFFI and `maturin` expect this setup. Specifically, `maturin` (a python rust
build tool) expects `cargo run --bin uniffi-bindgen` to work in the cargo
workspace. This setup ensures `uniffi` and `uniffi-bindgen` have the exact same
version.
