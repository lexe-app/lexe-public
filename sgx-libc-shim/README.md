# sgx-libc-shim

The `x86_64-fortanix-unknown-sgx` target doesn't support standard libc's like
glibc or musl. Some libc functions that are workable inside SGX (e.g., malloc,
string handling) are available via some shims in
[`rust-sgx/rs-libc`](https://github.com/fortanix/rust-sgx/tree/master/rs-libc).

This directory is the tiny slice of glibc headers needed so we can compile a few
native C dependencies (`ring` and `secp256k1`) for SGX without pulling in a full
libc sysroot.
