//! FFI interface for QR code generation

use flutter_rust_bridge::frb;

use crate::qr;

/// Encode `data` as a QR code, then render it as a raw bitmap image.
///
/// Uses RGBA pixel format with opaque white BG and `LxColors.foreground` FG.
pub fn encode(data: &str) -> anyhow::Result<Vec<u8>> {
    qr::encode(data.as_bytes()).map_err(anyhow::Error::new)
}

/// Return the size of the encoded QR code for the given data length in bytes.
#[frb(sync)]
pub fn encoded_size(data_len: usize) -> anyhow::Result<usize> {
    qr::encoded_size(data_len).map_err(anyhow::Error::new)
}
