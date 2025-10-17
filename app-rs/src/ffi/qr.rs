//! FFI interface for QR code generation

use crate::qr;

/// Encode `data` as a QR code, then render it as a .bmp image.
///
/// Returns a self-describing image format (.bmp) and not raw pixel data since
/// that's easier to consume on the Dart side.
///
/// Renders with an opaque white BG and `LxColors.foreground` FG.
///
/// Returns an error if the data is too long to fit in a QR code (input data is
/// longer than 2953 B).
pub fn encode(data: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    qr::encode(data).map_err(anyhow::Error::new)
}

/// Return the size in pixels of one side of the encoded QR code for a given
/// input `data.len()` in bytes.
///
/// flutter_rust_bridge:sync
pub fn encoded_pixels_per_side(data_len_bytes: usize) -> anyhow::Result<usize> {
    qr::encoded_pixels_per_side(data_len_bytes).map_err(anyhow::Error::new)
}
