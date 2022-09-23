//! Core types and data structures used throughout the lexe-node

/// Type aliases for concrete impls of LDK traits.
mod alias;
/// Types related to the host (Lexe) infrastructure such as the runner, backend
mod host;

pub(crate) use alias::*;
pub(crate) use host::*;
