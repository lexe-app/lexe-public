// TODO(phlip9): move most `run-sgx` stuff here

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
pub mod aesm_proxy;
