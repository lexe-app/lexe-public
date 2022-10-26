pub mod server;

// TODO(max): this module is pretty empty aside from the server; after merge,
// should move `host` and `owner` under server and de-nest the command module.

/// Commands that can only be initiated by the host (Lexe).
mod host;
/// Commands that can only be initiated by the node owner.
mod owner;
