//! Core types and data structures used throughout the lexe-node

/// Type aliases for concrete impls of LDK traits.
mod alias;
/// Types related to the host (Lexe) infrastructure such as the runner, backend
mod host;
/// Types leftover from ldk-sample, used in the EventHandler and REPL.
/// TODO: These should be converted into Lexe newtypes or removed entirely.
mod ldk;

pub(crate) use alias::*;
pub(crate) use host::*;
pub(crate) use ldk::*;
