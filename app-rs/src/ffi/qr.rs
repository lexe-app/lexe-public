//! FFI interface for QR code generation

use flutter_rust_bridge::frb;

use crate::qr;

/// Encode `data` as a QR code, then render it as a .bmp image.
///
/// Returns a self-describing image format (.bmp) and not raw pixel data since
/// that's easier to consume on the Dart side.
///
/// Renders with an opaque white BG and `LxColors.foreground` FG.
pub fn encode(data: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    qr::encode(data).map_err(anyhow::Error::new)
}

/// Return the size in pixels of one side of the encoded QR code for a given
/// input `data.len()` in bytes.
#[frb(sync)]
pub fn encoded_size(data_len: usize) -> anyhow::Result<usize> {
    qr::encoded_size(data_len).map_err(anyhow::Error::new)
}
