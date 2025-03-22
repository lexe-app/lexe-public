//! # QR encoding
//!
//! ### Why does this exist
//!
//! 1. Do the encoding in Rust, which is safe and reliable. The previous library
//!    (flutter_zxing) has/had multiple memory safety issues.
//!
//! 2. Do the full image encoding in Rust and not a mix of Dart and C++.
//!    [`fast_qr`] is especially fast and gives a lot of control.
//!
//! 3. Full control over the QR code sizing. For design reasons, I want the QR
//!    codes on the "Receive" page to look visually similar and take up the same
//!    space despite encoding different sized inputs (bitcoin address vs. LN
//!    {invoice,offer}). Without this, the BTC address QR code looks especially
//!    ugly.
//!
//! 4. (future) overlay or replace the center of the QR code with a LEXE logo.
//!
//! ### Background
//!
//! A QR code is made up of N×N [`Module`]s. These are the smallest white/black
//! squares in the code. The dimension N is determined by the [`Version`], where
//! `N = 17 + 4 * version`. The version is a number from 1 to 40.
//!
//! The code has an [`ECL`] (error correction level) of L, M, Q, or H. Each
//! level allows roughly 7%, 15%, 25%, or 30% of the code to be "damaged" while
//! still scanning successfully.
//!
//! The encoding [`Mode`] supports input strings where all characters are
//! Numeric ("0-9"), AlphaNumeric ("0-9", "A-Z", "%*./:+-?.= "), or Byte (any
//! `u8`). For our purposes, we only care about Byte encoding, since many
//! wallets don't handle scanning AlphaNumeric (all uppercase) bech32 codes
//! properly.
//!
//! On the UI side, we also need to be aware of the minimum **Quiet Zone** that
//! must surround the QR code in order to support a good scan rate. The Quiet
//! Zone is the white margin around the QR code. Per the spec, it should be at
//! least 4 modules worth of margin.
//!
//! [`ECL`]: fast_qr::ECL
//! [`Mode`]: fast_qr::Mode
//! [`Module`]: fast_qr::Module
//! [`Version`]: fast_qr::Version

use std::fmt;

use fast_qr::{qr::QRBuilder, Mode, QRCode, Version, ECL};

// --- public API --- //

/// Encode `data` as a QR code, then render it as a raw bitmap image.
///
/// Uses RGBA pixel format with opaque white BG and `LxColors.foreground` FG.
pub fn encode(data: &[u8]) -> Result<Vec<u8>, DataTooLongError> {
    let qr = encode_qr_code(data)?;
    Ok(qr_code_to_image(&qr))
}

/// Error when the data is too long to fit in a QR code (input data is longer
/// than 2953 B).
pub struct DataTooLongError;

// --- constants --- //

// color format: RRGGBBAA
//   background: opaque white
//   foreground: LxColors.foreground
const BG: [u8; 4] = [0xff, 0xff, 0xff, 0xff];
const FG: [u8; 4] = [0x1c, 0x21, 0x23, 0xff];

/// Target a specific QR code dimension (17 + 4 * v15 = 77 modules) so that
/// the generated codes look roughly the same, in the normal case.
const TARGET_VERSION: Version = Version::V15;
const TARGET_VERSION_USIZE: usize = 15;

// The max data length that can be encoded in a QR code with different ECL and
// versions, assuming Byte encoding.
//
// ```bash
// $ curl -o ecl.json https://web.archive.org/web/20230927043017/https://fast-qr.com/blog/ECL.json
// $ jq -c '. | map(.H | .[2]) | .[15 - 1]' ecl.json
// 220
// $ jq -c '. | map(.Q | .[2]) | .[15 - 1]' ecl.json
// 292
// $ jq -c '. | map(.M | .[2]) | .[15 - 1]' ecl.json
// 412
// $ jq -c '. | map(.M | .[2]) | .[40 - 1]' ecl.json
// 2331
// $ jq -c '. | map(.L | .[2]) | .[40 - 1]' ecl.json
// 2953
// ```
const MAX_DATA_LEN_H_B_V15: usize = 220;
const MAX_DATA_LEN_Q_B_V15: usize = 292;
const MAX_DATA_LEN_M_B_V15: usize = 412;
const MAX_DATA_LEN_M_B_V40: usize = 2331;
const MAX_DATA_LEN_L_B_V40: usize = 2953;

/// Encode `data` as a QR code that's at least [`TARGET_VERSION`] in size.
fn encode_qr_code(data: &[u8]) -> Result<QRCode, DataTooLongError> {
    let (ecl, maybe_version) = len_to_params(data.len())?;

    let mut qr_builder = QRBuilder::new(data);
    qr_builder.ecl(ecl);

    // We always use Byte encoding. In theory you can uppercase bech32 addresses
    // and invoices so they can use the more efficient Alphanumeric encoding,
    // but many wallets don't decode that properly.
    qr_builder.mode(Mode::Byte);

    if let Some(version) = maybe_version {
        qr_builder.version(version);
    }

    let qr = qr_builder.build().expect("Encoding should never fail");

    //       N = 17 + 4 * version
    // version = (N - 17) / 4
    let version = (qr.size - 17) / 4;
    if maybe_version.is_some() {
        assert_eq!(version, TARGET_VERSION_USIZE);
    } else {
        assert!((TARGET_VERSION_USIZE..=40).contains(&version));
    }

    Ok(qr)
}

/// Given the length of the data, return the ECL and version that can encode it.
///
/// We target a specific version [`TARGET_VERSION`] (which determines the
/// dimension) so that the generated codes look roughly the same, in the
/// normal case.
///
/// Shorter input data (like a BTC address) will just get more error correction.
const fn len_to_params(
    len: usize,
) -> Result<(ECL, Option<Version>), DataTooLongError> {
    if len <= MAX_DATA_LEN_H_B_V15 {
        Ok((ECL::H, Some(TARGET_VERSION)))
    } else if len <= MAX_DATA_LEN_Q_B_V15 {
        Ok((ECL::Q, Some(TARGET_VERSION)))
    } else if len <= MAX_DATA_LEN_M_B_V15 {
        Ok((ECL::M, Some(TARGET_VERSION)))
    } else if len <= MAX_DATA_LEN_M_B_V40 {
        Ok((ECL::M, None))
    } else if len <= MAX_DATA_LEN_L_B_V40 {
        Ok((ECL::L, None))
    } else {
        Err(DataTooLongError)
    }
}

/// Encode a QR code as an a bitmap image in RGBA pixel format.
fn qr_code_to_image(qr: &QRCode) -> Vec<u8> {
    let len = qr.size * qr.size;
    let data = &qr.data[..len];

    // Use this iterator chain specifically because it auto-vectorizes properly:
    // <https://godbolt.org/z/P9Kafd89Y>
    #[allow(clippy::map_flatten)]
    data.iter()
        .map(|module| if module.value() { FG } else { BG })
        .flatten()
        .collect()
}

// --- impl DataTooLongError --- //

impl fmt::Display for DataTooLongError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the data is too long to fit in a QR code")
    }
}
impl fmt::Debug for DataTooLongError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl std::error::Error for DataTooLongError {}

#[cfg(test)]
mod test {
    use proptest::{arbitrary::any, proptest};

    use super::*;

    /// Sanity check that `fast_qr` layout is as we expect.
    #[test]
    fn test_qr_data_layout() {
        let data = "hello";
        let qr = QRBuilder::new(data.as_bytes()).build().unwrap();

        let layout_by_row = (0..qr.size).flat_map(|row_idx| &qr[row_idx]);
        let layout_flat = &qr.data[0..(qr.size * qr.size)];
        assert!(layout_by_row.eq(layout_flat.iter()));
    }

    #[test]
    fn test_encode_qr_btc_address() {
        let data = "bc1qd09ayuz2zavp4a6q3eswqkf8ufw640w2y7z4mw";
        let expected = r#"
▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄
█ ▄▄▄▄▄ █ ▄ ▀▄█▄▄█ ▀▄▄ █▄  ▄ ▄ █▀▀▀▀ ▀ ▀█▀ ▀▀█▀▀███▄  █▀█▀ ▄█▄█▄ ▀█▀█▀█ ▄▄▄▄▄ █
█ █   █ █▄▄ █▄█▄▄▀▀▄█▀▀ ▄█▄▀▄█▀█ ▄ █▀▄▀▀▀  ▀▀ █▄▄▄█▀███▀█▀ ▄█▄█▄ ▀▄██ █ █   █ █
█ █▄▄▄█ █▀ █▀  ▄ ▀█▀▄▄▄ ▀ ▄▄▄ ▄▄▄ ██▄▀▀ ▀▄▄█ ▀▀ ▄▄▄  ██▀█▀ ▄█▄█▄ ██ ▄▄█ █▄▄▄█ █
█▄▄▄▄▄▄▄█ ▀▄▀▄▀ ▀▄█ ▀▄▀▄▀ █▄█ █ █ ▀▄█▄▀▄█ ▀ ▀▄▀ █▄█ █▄█▄█▄▀ █ █ █ █▄█ █▄▄▄▄▄▄▄█
█▀█▄ ▄▀▄█ █ ▄█▀▄ ▄ █  ▀█▄  ▄▄▄▄ ▄▄▀█▀█ ▀██▀▄▄▄▀   ▄ █▀█▄█▄ ▀█▀█▀ ▀█▄▄ ▄▄▄▀█▄ ▄█
█  █ ▄ ▄   ▀▄▀▀ ▀█▄ ██▄▀█ ▀▀▄█▄  █▀ ▄▀ ▄█  ▄▄▀▄▄▄███▄▀▀▄█▄ ▀█▀█▀▄ █▄▄▄▄█▄ ▄█▀ █
█ █▀  ▄▄ ▀▀▄█▀▄▀▀▄▄█▀     ▄█▀ ▄ █  ▀ █ ▄▄  ▄ ▄▄ ▄▄█ █▄ ▀█▄ ▀█▀█▀ ██▄▄ ▀ ▄ ▄██▄█
█   ▀  ▄▀  ▀ ▄█  ▄ ▄▀█▀▄█ ▀▄ █▄   ▄▀█▄▀ ▄▀▀▀  █▀▀▀█████▀█▄ ▀█ █▀█▄█▄ ▀▀▄▄ ██▄██
█ █▄▀█ ▄█ ▀▄▄██  ▄█▄▄ ▀█  ▄███▄ ▀█ ▄▀██ ██▀█ ▄▄▄▀▀█ ▀██▄█▄ ▀▀██▀ ██▄▄ ▄█▄ █  ██
█    ▄ ▄   ▀▄▀ ▀▀█▀▄█▄▄▀█ ▄ ▄█▄  ▄▄ ▄▀█▄█ █▄▀   ▄███▀▄▀▄█▄▄▀▀██▀▄▀█▄█▀▄█▄ █▄▀ █
█ ███ ▄▄ ▀▀▄█▀▄▀▀▄▀ ▄     ██▀ ▄ █  ▄ █▄ ▄ ▄  ▄  ▄▄█  ▄ ▀█▄█▄▀▄█▀ ▄█▄▄▀▀ ▄ ▄▄█▄█
█   █  ▄█▄ ▀ ▄█▄ ▄ ██▄  █    █▄   ▄ █▄█▄ █ ▀▀ ▀  ▀██ ▀█▀█▄ ▄▀▀█▀ ▀█▄▄█▀▄▄  ▄▄██
█ █▄▄ ▄▄▄ ▀▄▄█▀▀ ▀ ▀▄   ▄ ▄▄▄ ▄ ▀█ ▄▀█▄█ █▀  ▄  ▄▄▄ █▀█▄█▄ ▀█▀█▀█▀█▄▀ ▄▄▄  ▄ ██
█  ▄█ █▄█  ▀▄▀█▄▄▄█▄█▄  ▄ █▄█ ▄  ▄▄ ▄▀▄▄▀█▄ ▀ ▀ █▄█ ▀▄▀▄▀▄▄▀▀▀▀▀▀▄█▄▄ █▄█ ▄▄▀ █
█ █▄▀▄ ▄▄▄▀▄█▀▀▀ ▀▄▄▄ ▀▀█  ▄▄▄▄ █  ▄ █▄█ █ ▀ ▄  ▄ ▄  ▄ ▀▀▀█▄█  ▄ ▄█▄   ▄    █▄█
█  █ ▀ ▄▄  ▀ ▄█▄▄▄  █▄█ ██ ▀▀█▄   ▄ █ ▄▄▀█▀█▀  █▀██▄ ▀█▀▀▄ ▄█▄█▄ ▀█▄▄█▀█▀ ▀ ▄██
█ █ ▄▀ ▄██▀▄▀█▀▀ ▀█▄▄ ▄██  ▄█▀▄ ▀█ ▄▄█▄█ ██▄ ▄▀▀▄ █▀█▀█▄█▄ ▀ ▄█▀█▀█▄▄██▀▀ ▀▀▄██
█  ▄██▀▄▄  ▀█▄█▄▄▄ ▀█▄█▄██ ▀ ▄▄▄ ▄▄ █ ▄▄▀█▀▄▀ ▀▀▀██▄▀▄▀▄▀ ▄▀▄▀▀▀▀▄▀▄▀▀▄ ▀ ▀ ▄██
█▄▀▄▀ █▄██▀▄▀█▀▀ ▀█▄▄ ▄▄█  ▄ ▀▄██  ▄▄█▄█ ██▄ ▄▄█▄  █ ▄█▄█▄█▄█▀ ▄ ▄ ▀▀  █▀ ▀▀▄██
█▀▄█ ▀ ▄▄  ▀█▄█▄▄▄ ▀█▄▀ ██ ▀ ▀▀  ▄▄ █ ▄▄▀█▀▄▀ ▀▀▀█▄▀ ▀▄██▀ ▄█▄█▄ ▀█▀▄█▀█▀ ▀ ▄██
█▄▄ ▄█ ▄██▀▄▀█▀▀ ▀█▄▄ ████ ▄  █▄▀  ▄▄█▄█ ██▄ ▄▄█▄   █▀▄▄ ▀ ▀ ▄█▀█▀█▄▄█▀█▀ ▀▀▄██
█ █▄█▀▄▄▄  ▀█▄█▄▄▄ ▀█ ▀▀▄▀ ▀ ▀▄█▀█▄ █ ▄▄▀█▀▄█ ▀▀▀█▀█▀▄▄▀▄▄▄▀▄▀▀▀▀▄▀▄▀▀▄▄▀▄▀ ▄██
█▄▀▄▀█▄▄██▀▀▀█▀▀ ▀█▄ ▀██ █ ▄  ▄█▀ ▀▀▄█▄█ ██▄ █▄█▄  █ ▄█▀█▄ ▀█▀ ▄ ▄ ▀▀   ▄▄▀▀▄██
█▀▄█  ▄▄▄ ▄▀█▄█▄▄▄ ▀  ▀▀▄ ▄▄▄ ▀ ▀█▀ █ ▄▄▀█▀▄▀▀▄ ▄▄▄  ▀ ▄█▀▀▄█▄█▄ ▀█▀▄ ▄▄▄ ▀ ▄██
█▄▄ ▄ █▄█ ▀▄▀█▀▀ ▀█▄▄▄███ █▄█ █▄▀ █ ▄█▄█ ██▄█▀▀ █▄█ █▀ ▀ ▀▀▄ ▄█▀█▀█▄█ █▄█ ▀▀▄██
█ █▄█ ▄▄    █▄▀▄▄  ▀██▀▀   ▄ ▄▄█▀█▄▀█   ██▀▄▄█ ▄▄▄ ▄▀▄▀▀▄▄▀ ▄▀█▀▀▀▀▄▄▄  ▄ ▀ ▄██
█▄▀▄▀▀█▄█▄▀▀▀██   █▄▀███▄▄    ▄█▀ ▀▀▄█▄▄█▄█▄██▄  ▀▀  ▄ ▀█▄▀ █▀█▄█▀ ▀█▀▀▀ ▀▀▀▄██
█▀▄▄ ▀█▄▀▀▄▀█▄ ▄▀  ▀█ ▀▀█▀ █ ▄▀ ▀█▀ █ ▀█▀█▀▄ ▄ ██▄▄▄ ▀▀▀█▀█▀█▄▄▀▀▀█▀ ▀▄█ ▀▀ ▄██
█▄▄▄ ▀█▄█▄▀▄▀█ █▀ █▄  ██▀█    █▄▀ █ ▄█ ▄  █▄▀█▄  ▀ ██▀ ▀ ▀█▄ ▄█▄█▀█▄▄▀▀▀ ▀▀▀▄██
█ █▄ ▀█▄▀▀  █▄▀▄▄▄ █▀█▀▀▄█ █ ▄▄█▀█▄▀▀▄▀  ▄▀    ██▄ █▀▄▀▀▄▄▄▄▄▄▄▄▀▀█▄ ▀▄▄█▀▀ ▄██
█▄▀▄ ▀█▄ █▀▀▀ █ █▄▀▀█▀██▀ █▀  ▄█▀ ▀▀▀▄▄▄▀ ███▀▄  ▄▀  ▄ ▀█▄█▄ ▄█ █▀█▄▄▀▄▀▀▀██▄▀█
█▀▄▄ ▀█▄  ▄▀▀█ ▄   █▀█▀▀▀▄ █ ▄▀▄▀█▀ ▄ ▀█ ▄▄ ▀▀ █ ▄▄▄ ▀▀▀█▀▄▄▄▄ ▄▀▀▄█ ▀▀ ██▄█▀██
█ ▄▄ ▀█▄▀ ▀▄▄█ █▀███▀▀████ █  █▀▀ █ ▀█ ▄██▄▄▄█▄▄ █ ██▀ ▀ ▀█▄ ▄█ █▀▄▀▄▀▀▄▀▄ ▀▄▀█
██▄██ ▄▄ █  ██▀▄█  █▄█▀███▄▀ ▄▀▄▀█▄▀█▄▀ █▄ ▀▀▀▄█▀▀ █▀▄▀▀▄▀▄▄▄▄  ▀▀▄█ ▀▀ ▀█▀ ███
██▄▄▄▄█▄█ ▀▀█ █ ▀▀▀▀█▀ ▄█ ▄▄▄ ▀▄▄█▀▀▄█▄▄  ███▀▀ ▄▄▄  ▄ ▀ ▀█▄ ▄ ▄█▀▄▀▄ ▄▄▄ ███▀█
█ ▄▄▄▄▄ ███▀▄▀ ▄█  █▀█▄█▄ █▄█ ███ ▀ ▄█▀██▀▄ ▀▀  █▄█  ▀▀▀▄▀▄▄▄▄▀▄▀▀ ▄▀ █▄█ ▄████
█ █   █ █  █▀█ █  ██▀▀▀▄█▄▄▄   ▀▀█▄ █  ▄▄▄▄▄▄█▄▄▄▄▄ █▀ ▀ ▀█▄ ▄ ▄█▀▄ ▄ ▄    ▀▀██
█ █▄▄▄█ █▄██ █▀▄█  █▄█▀█  ▀▄  ▀  ▀█▀▄ ▀ ▄█ ▀▀▀▀▄ █▀ ▀▀▀▀▄▀▄▄▄▄▀▄▀▀ ▄▄▄▄▀▀ ▀  ██
█▄▄▄▄▄▄▄███████▄██████▄▄▄▄▄█▄▄▄█▄█▄▄█▄▄▄▄▄████▄▄██▄███▄█▄██▄▄▄▄▄██▄█▄▄▄▄█▄█████
"#;
        let qr = encode_qr_code(data.as_bytes()).unwrap();
        assert_eq!(qr.to_str(), expected.trim());
    }

    #[test]
    fn test_encode_qr_ln_invoice() {
        let data = "lnbc1pnauxqddqqpp54cu7crnrjtm69s5kp0ex9la9caksr4ma8u07948vhe9y9qektgxscqpcsp53uenetklzrh03kzd2l63wzhn6u2pku7ynnmrr6k3vvj3ljgsf79q9qyysgqxqyz5vqnp4q0w73a6xytxxrhuuvqnqjckemyhv6avveuftl64zzm5878vq3zr4jrzjqv22wafr68wtchd4vzq7mj7zf2uzpv67xsaxcemfzak7wp7p0r29wz0nfsqq0mgqqvqqqqqqqqqqhwqqfqn7kf3ps2q7ruplgnegxukp8dwfrqw75cgs656aqxm76ph20y4asxpzj5t47llp6gka9sg0am2kjfsjkd2s28tgnn08k0twmh5jye7qcpjur8xp";
        let expected = r#"
▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄
█ ▄▄▄▄▄ ██ ██▀ █ ▄   ▀█▄█▄ ▀▄ ▄▄█ █▀▄ ▄▄▄  ▄ ▄ ▄▄▀▀ ▀ ▀▄ █▄▄▄█▀ ▄▀▀▀█▀█ ▄▄▄▄▄ █
█ █   █ █  ▀▄▀█▀ ▀▄█▄▀ ██  ▄ ▄▄█ ▄ ▀ ▀▀▄▄▄▀▀ ▄▄ ▄ ▄█▀ █▀▄▄▀ ▄▀▄▄▀ ███ █ █   █ █
█ █▄▄▄█ █ ▄█▄ █ ▀█▄ █▀▀▀  ▄▄▄ ▀▄█ █▀▄  ▀▀▄▄▄█ ▀ ▄▄▄  ▀▀ ▄ ▄███▀▀█▀▀ ▄▄█ █▄▄▄█ █
█▄▄▄▄▄▄▄█ █▄█▄▀ ▀ █ ▀ ▀▄█ █▄█ ▀ ▀ █ █▄█▄█ ▀ █▄▀ █▄█ █ █▄▀ ▀ ▀▄█ █▄█▄█ █▄▄▄▄▄▄▄█
█ ▀ ▄▄ ▄▀▀█▄ ▄▀▀██ ▄▄▀▄ █▄  ▄▄▀▀ █▄  ▄ █ ▀▄▀▄    ▄▄ ▄ ▄▀▀▄ ▀▀██▄ ▄▄█▄██ ▄  ▄█▀█
█ ▄ ▄▄ ▄▀ ▀███▄▀  ▀▀▀▄ ▀ ▄█▄▀  ▀ ▀ ██▄▄▄█▀ ▀▄ ▀ ▀   ▄▄ ▀ ▀▀ ▀▄ ▄▄  █▄█▄▀ ▀█▄█▀█
██ ▀██▄▄ ▀▄ ▀▀ ▄█ ▄▄▀▄▀█▀▄▀▀ ▄▀ █▀ █▄ ▀▄██▄▀▄▀ █ ███▄▀▄ ██▀▄▀█▄█  █ ▄ ▀█ ▀▄▄█▀█
█▀ ▄█ ▄▄▀ ▄█▄█   ▄▀█▀███████▄▄▀██ ▀▄ ▄▄ ▄▀ ▀█▀▀▄█▄▄ ▄▀ ▀▀▄▀ ▀▄ ▀▄▀▄█ ▀▀▀▀▀█▄ ▀█
██▀ ▀█▄▄ ▄█▄█▀ ▄██▄███▄█▀ ███▄▀█ █ █▄ ▀█▀██▀ █ ▀▀▄█  ▄▄ █▄ ▄███▄  ▄█▄ ▄  ▀  █▀█
█ ▀▀▄▄█▄▀▀▄▀▀ ▄█  ▄▄ ▀▀█▄██▄█▄█  ▀▀▄▀█▄▄▄ ▄▀ ▀ ▀█  ▀▄ ▄▄▀▀▀ ▀█▄▄▄▄▄▀▀█▄▀▀▀█▄▀▀█
█▀▄█▄▀█▄▀▀▄▄▄█▀█▀█▄▀  ██ ▄▀█▀█  ███▄▀▄ ▄▄█   █▀█▀▄█▀▄▀▄▀█ ▀▀▀▄█ ▄██ ▄▄ ▀▄ ▄ ▀▀█
█▀█ ▄▄▄▄█▄ █ ▀  ▀▄ █ ▄▄▄█▄▄█▀▄▀▀▀▄▀ ▀█▄▄▄▀▄▀▀▀▀▄█▀▄▀▄ ▄█▀ ▀ ▀▀  █▀▄▀▄▀▀▀▀▀▀▄▀██
█▀▀▀▄ ▄▄▄ ▄██  ▄█ ██▄▀▀ █ ▄▄▄  ▀▀█▄█ ▄ ▄▀▀ ▀▄█▀ ▄▄▄  ▄▄██▄▀ █▄▄█ █▄   ▄▄▄ █▄▀ █
█▄█   █▄█ ▄  ▀▄ ██ █▄ ▄▀  █▄█ █▀▀▀▀ ▀▄█▄█▀██▀▀▀ █▄█ █  ▀  █ ▀█  ▄  ▄▀ █▄█ ▄█▀▀█
██▄▀▄▄ ▄   █▄ ▄▀▀█▄██▄▀▄▀  ▄▄▄▀ ▄█▄▄ ▄ ███ ▀██▀ ▄▄▄ ▄▄██▀  ▄█▀██▄ █ ▄▄ ▄ ▄ ▄▀ █
█▀█ █▀▄▄▄▄▀▀▄ ▄▄█▄█ ▄  █▄█▀  ▄▀▀▀ █▄▀▀▀▄█ █▀▀ ▄ ▄█▀▄▄ ▄▀▀ ▀▄▀█  ▄ ▄▄ ▄███▄ █▀██
█▀▀████▄▀▄▀▀▄ ▀ ▀▀██▄██▀▄▀█▄   ▀▀▀██▀ ▄██▀▄ ▀▀█▀▄    ██ ██ ▀▀███▄ █▄▄█▄  ▄▄ ▀▄█
█▀▀  ▀ ▄▄▄ █▀  ▄ ▀▀ █▄▀  ▄  █▄█▀▄▀ ▄▀▄▄ ▄▀   ▀▄█▀▀▀▀█▄▄▄  ▀▄▀▄  ▄▀▄▄ ▄▄███▀▄▄▄█
█ ██ ▀█▄▄▀▀▀ ▀  █  ██▄▀▄▄█ ▀█▄▄  ████ ▀▄ ▀▀  ▀▄▀ ██  ▄▄███ ▀▀ ▄█ ▄██▀██▄ ▄█▄ ▀█
███▄ █ ▄  █ █▀▄▄▄█▄█▄▄ ▄▀▀█▄█ ▄▄▀▀   ▄█▄████▄▀█▄▄▄▀▀▄ ▄▀▀▀█ ▀▀ ▄█ ▄▀ ▄ ▄█▄ █▀▀█
██ ▀  ▀▄█▀▀▀▀     ▀ ███▀▀▄▀  ▄  ▄▀▄█  ▄█▄█▀ ▄█▄▀▄█ ▄ ▀████ ▀█ █▀ ▄▄▀▄▄▀ ▄▄█▄█ █
█ █▄ ▄▄▄▄▄▀▀▄▀▄▄▄█  ▄ ██▀▀█▀█▄  ▄▀ ▄▀▄▄█▄▀ █▄▀██▄▀▀▀▄ ▄█▀▄▀▀▀▄▄ ▄▀▄▄▄▄▄▄▄██▄ ██
██ ▀ █▀▄  ▄ █  ▄█ █▄▄ ▀▀▀  ▀▄▄ █▄▀▀▄ ▄▄▄▀▀▀▀▄▀█▀▄▄▀▀ ▄████▀▄█▄▄▄▄▄▄ ▄█ ▄▄  ▄  █
█▄▄██ ▄▄▄ █▀▄ ▄▄ ▀ ▄▄▀ ▄  ▄▄▄  ▀▀ ▀▄▄▄▄▄█▀█▀▀█  ▄▄▄ ▄▄ ▄▀▀█▀▀▀▄▀▄▀▄▄  ▄▄▄  ▄█▀█
██▄ █ █▄█   █  ▄ ▀▄▀█ ▄ █ █▄█ ▀▀▄█▀█▄ ▀█▄▀ ▀▀█▄ █▄█ ▄██ █  ███▄▀  ▄ ▄ █▄█ ▄▄▄ █
█ ▀▀█▄ ▄▄▄▄▀ ▀▀▀█▀██▄▀ ▄█▄  ▄▄  █ ▀█▀██▄▄▀██▀▀▄  ▄▄▄█▄ ▀▀▄▀ ▀▀▄▄█ ▄██▄▄▄▄▄ ▄▄▀█
██▄ █ █▄▀█  ▀█▄▄█▄ ▀ ▄▄▀ ▀▄█▄▀██▄▀▄██▄█▄██▀ ▀█▄ █▀▄  ▀█ █▄▀▄█▄██ ▄█ ▀ ▀▀▄▀▄▄▄ █
█▀  █  ▄▀▀▀█▀ ▀▄██ █▄█▄   ▄▄█▀█▀ ▀▄▄▀▄▄ ▄▀ ▀  ▄▄▀▀▄▄█ ▄▀▀ ▀▄▀▄▄▄▄▄ █▀▀ █▄▄▀████
█   ▀▄▀▄  ▄▀███  ▀█ ▀  ▄ ▄▀   ▄▄ ▀▀█▄  ██▀█▀▀▀ ▄▀▄▄ ▄▀██▀█▀▄▀▄▄  ███▀▀▄▀ ▀▄▄▄▀█
█▄▀██▀█▄ ▀ █▄▄▄█▀██▀▄█▀▄▀▀▀▀ ▀█▄▀▀█▄▄▄▄ █▀▄▀ ▀▄ ▀█▄▀▄▄▄█  ▀  ▄▄▄█▀ ▀▀▄ ▀█ █████
█  █▀▀▀▄   ▄▄████▄▄▀█ ▄ █  ▀▀ ▄█ █▄▄▄▄▀▄▀▀   ██▀▀ ▄▄▄▀█▀█  ▀▀▄█▄▄█▄▀▀▄█▀▄▀  ▀▀█
██▄▄▀█▄▄██▀  █▀▄▀ ▄ ▀▄▄ ▄█ ▀▀▀██▄█▀▄ ▄▄▄█▀ ▀▀▀▀▀ ▀███▄ ▀▀ █  ▄ ▀█ ▄▀▀ ▄██ ▀██▀█
█ ▄  ██▄█▀ ▄   █▄▀█▀██▄▄▄▄█ ▀▄▀▀▄ ▄▄ ▄ █▄█ ▀▄▀█▀█ ██  ███▄ ██ █▀  █▀▀ █▀ ▀█  ▀█
██▄██ ▄▄▄█ ▄▀▄▀█ ▄ ██ ▀ ▄   ▄█▄▄   ▄█▄▄  ▀▄█▀ ▄▀▀█▄ ▄▀▄▀▀ █  ▄▄▄▄ ▄▀▀█ ██▀ ████
██▄▄▄▄█▄▄▀█▄█▀ ██ ██ ██   ▄▄▄ █ █▀▄▄  ▀█▀▀▀▀▀██ ▄▄▄  ▀▄█▀▄ ▄▀▄█▀ ▄█   ▄▄▄ ▀▄▀▀█
█ ▄▄▄▄▄ █▀█ ▀██▀██ █▀▀█ ▀ █▄█ ▄▄▀▀█ ▄  ▄▀ ▀█ ▀▄ █▄█ ▄▄ █▀▄▀ ▀▄▄▄█  ▄  █▄█ ▀████
█ █   █ █ ▀▄█ ▀ █▄▄▀█▀ ▀  ▄ ▄▄▀▄▀███▄█▀▄█▀▀█▄██▄▄   ▄██ █▀▀▀▀▀█  ██▄▀ ▄▄ ▄▀▄ ▄█
█ █▄▄▄█ █▄▀█▄█▀ ▀▀▀▀▄▀▀▀▀▄▀▀▄▄▀▄  ▄▄▄██ ▄▀▀▄▀▀█▄▄▄▄█▄▀ ▀▀ ▀ ▀▀▄▀█  █▀ ▀▄▄▄ ▄ ██
█▄▄▄▄▄▄▄█▄███▄███▄██▄██▄██▄▄████▄█▄▄▄▄▄▄▄█▄▄████▄██▄▄▄▄▄█▄▄██▄█▄▄▄█████▄▄█▄▄███
"#;
        let qr = encode_qr_code(data.as_bytes()).unwrap();
        assert_eq!(qr.to_str(), expected.trim());
    }

    #[test]
    fn test_encode_never_panics_with_valid_len() {
        let arb_data =
            proptest::collection::vec(any::<u8>(), 0..=MAX_DATA_LEN_L_B_V40);

        let config = proptest::test_runner::Config::with_cases(10);
        proptest!(config, |(data in arb_data)| {
            let _ = encode(&data).unwrap();
        });
    }

    /// ```bash
    /// $ cargo test -p app-rs --lib -- test_encode_exhaustive_lens --ignored
    /// ```
    #[test]
    #[ignore = "takes 40+ seconds to run so only run manually"]
    fn test_encode_exhaustive_lens() {
        for len in 0..=MAX_DATA_LEN_L_B_V40 {
            let data = vec![0x69; len];
            let _ = encode(&data).unwrap();
        }
        for len in (MAX_DATA_LEN_L_B_V40 + 1)..=(MAX_DATA_LEN_L_B_V40 + 100) {
            let data = vec![0x69; len];
            let _ = encode(&data).unwrap_err();
        }
    }
}
