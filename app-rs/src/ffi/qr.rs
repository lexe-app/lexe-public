//! FFI interface for QR code generation

use crate::qr;

/// Encode `data` as a QR code, then render it as a raw bitmap image.
///
/// Uses RGBA pixel format with opaque white BG and `LxColors.foreground` FG.
pub fn encode(data: &str) -> anyhow::Result<Vec<u8>> {
    qr::encode(data.as_bytes()).map_err(anyhow::Error::new)
}
