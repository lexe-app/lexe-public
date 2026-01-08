use std::{borrow::Cow, str::FromStr};

use anyhow::{Context, anyhow, ensure};
use common::{aes::AesMasterKey, rng::Crng, time::TimestampMs};
use lexe_api::types::payments::{DbPaymentMetadata, DbPaymentV2, LxPaymentId};
use lexe_std::{const_assert, fmt::DisplayOption};
use tracing::warn;

use crate::payments::{
    PaymentMetadata, PaymentV2, PaymentWithMetadata, v1::PaymentV1,
};

/// The payments schema version we currently use to serialize payments.
///
/// V1 format stores metadata inside the payment blob; there is no metadata.
/// V2 format stores metadata separately; metadata is optional.
const CURRENT_PAYMENTS_VERSION: i16 = 1;
const_assert!(CURRENT_PAYMENTS_VERSION == 1 || CURRENT_PAYMENTS_VERSION == 2);

// --- Public API --- //

/// Encrypts a [`PaymentWithMetadata`] using `CURRENT_PAYMENTS_VERSION`.
///
/// Returns the encrypted payment and optionally the encrypted metadata.
/// V1 format stores metadata inside the payment blob and returns `None`.
/// V2 format stores metadata separately and returns `Some`.
pub fn encrypt_pwm(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    pwm: &PaymentWithMetadata,
    created_at: TimestampMs,
    updated_at: TimestampMs,
) -> anyhow::Result<(DbPaymentV2, Option<DbPaymentMetadata>)> {
    match CURRENT_PAYMENTS_VERSION {
        1 => {
            let payment_v1 = PaymentV1::try_from(pwm.clone())
                .context("Failed to convert payment to v1")?;
            let db_payment = encrypt_payment_v1(
                rng,
                vfs_master_key,
                &payment_v1,
                created_at,
                updated_at,
            );
            Ok((db_payment, None))
        }
        2 => {
            let db_payment = encrypt_payment_v2(
                rng,
                vfs_master_key,
                &pwm.payment,
                created_at,
                updated_at,
            );
            let db_metadata =
                encrypt_metadata(rng, vfs_master_key, &pwm.metadata, updated_at);
            Ok((db_payment, Some(db_metadata)))
        }
        v => Err(anyhow!("Unexpected CURRENT_PAYMENTS_VERSION: {v}")),
    }
}

/// Decrypts a [`DbPaymentV2`] and optional [`DbPaymentMetadata`] into a
/// [`PaymentWithMetadata`].
pub fn decrypt_pwm(
    vfs_master_key: &AesMasterKey,
    db_payment: DbPaymentV2,
    db_metadata: Option<DbPaymentMetadata>,
) -> anyhow::Result<PaymentWithMetadata> {
    let version = db_payment.version;
    match version {
        1 => {
            // V1 metadata was inside the payment blob; it *should* be None.
            if db_metadata.is_some() {
                warn!("v1 payment has metadata; likely a partial migration");
            }
            decrypt_payment_v1(vfs_master_key, db_payment)
                .map(PaymentWithMetadata::from)
        }
        2 => {
            let payment = decrypt_payment_v2(vfs_master_key, db_payment)?;
            let metadata = match db_metadata {
                Some(db_meta) => decrypt_metadata(vfs_master_key, db_meta)?,
                None => PaymentMetadata::empty(payment.id()),
            };
            Ok(PaymentWithMetadata { payment, metadata })
        }
        v => Err(anyhow!("Unsupported payment version: {v}")),
    }
}

// --- PaymentV1 / PaymentV2 --- //

/// Encrypts a [`PaymentV1`] to a [`DbPaymentV2`].
fn encrypt_payment_v1(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    payment: &PaymentV1,
    created_at: TimestampMs,
    updated_at: TimestampMs,
) -> DbPaymentV2 {
    let aad = &[];
    let data_size_hint = None;
    let write_data_cb: &dyn Fn(&mut Vec<u8>) = &|buf| {
        serde_json::to_writer(buf, payment)
            .expect("PaymentV1 serialization always succeeds")
    };

    let data = vfs_master_key.encrypt(rng, aad, data_size_hint, write_data_cb);

    DbPaymentV2 {
        id: payment.id().to_string(),
        kind: None,
        direction: None,
        amount: None,
        fee: None,
        status: Cow::Borrowed(payment.status().as_str()),
        data,
        version: 1,
        created_at: created_at.to_i64(),
        updated_at: updated_at.to_i64(),
    }
}

/// Encrypts a [`PaymentV2`] to a [`DbPaymentV2`].
fn encrypt_payment_v2(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    payment: &PaymentV2,
    created_at: TimestampMs,
    updated_at: TimestampMs,
) -> DbPaymentV2 {
    let version_bytes = 2_i16.to_le_bytes();
    let aad = &[version_bytes.as_slice()];
    let data_size_hint = None;
    let write_data_cb: &dyn Fn(&mut Vec<u8>) = &|buf| {
        serde_json::to_writer(buf, payment)
            .expect("PaymentV2 serialization always succeeds")
    };

    let data = vfs_master_key.encrypt(rng, aad, data_size_hint, write_data_cb);

    DbPaymentV2 {
        id: payment.id().to_string(),
        kind: Some(payment.kind().to_str()),
        direction: Some(Cow::Borrowed(payment.direction().as_str())),
        amount: payment.amount(),
        fee: Some(payment.fee()),
        status: Cow::Borrowed(payment.status().as_str()),
        data,
        version: 2,
        created_at: created_at.to_i64(),
        updated_at: updated_at.to_i64(),
    }
}

/// Decrypts [`DbPaymentV2`] into [`PaymentV1`].
fn decrypt_payment_v1(
    vfs_master_key: &AesMasterKey,
    db_payment: DbPaymentV2,
) -> anyhow::Result<PaymentV1> {
    // Destructure so can update the validation below when a new field is added.
    let DbPaymentV2 {
        id: db_id,
        kind,
        direction,
        amount,
        fee,
        status: db_status,
        data,
        version,
        created_at: _,
        updated_at: _,
    } = db_payment;

    ensure!(version == 1, "expected version 1, got {version}");

    // Version 1 should have no plaintext fields
    ensure!(kind.is_none(), "v1 payment has unexpected 'kind'");
    ensure!(direction.is_none(), "v1 payment has unexpected 'direction'");
    ensure!(amount.is_none(), "v1 payment has unexpected 'amount'");
    ensure!(fee.is_none(), "v1 payment has unexpected 'fee'");

    let aad = &[];
    let plaintext_bytes = vfs_master_key
        .decrypt(aad, data)
        .context("Could not decrypt Payment")?;

    let payment =
        serde_json::from_slice::<PaymentV1>(plaintext_bytes.as_slice())
            .context("Could not deserialize PaymentV1")?;

    // Validate id and status match
    let db_id = LxPaymentId::from_str(&db_id).context("invalid db id")?;
    ensure!(
        payment.id() == db_id,
        "id mismatch: db={db_id}, payment={}",
        payment.id(),
    );
    ensure!(
        payment.status().as_str() == db_status,
        "status mismatch: db={db_status}, payment={}",
        payment.status().as_str(),
    );

    Ok(payment)
}

/// Decrypts [`DbPaymentV2`] into [`PaymentV2`].
fn decrypt_payment_v2(
    vfs_master_key: &AesMasterKey,
    db_payment: DbPaymentV2,
) -> anyhow::Result<PaymentV2> {
    // Destructure so can update the validation below when a new field is added.
    let DbPaymentV2 {
        id: db_id,
        kind,
        direction,
        amount,
        fee,
        status: db_status,
        data,
        version,
        created_at: _,
        updated_at: _,
    } = db_payment;

    ensure!(version == 2, "expected version 2, got {version}");

    let version_bytes = version.to_le_bytes();
    let aad = &[version_bytes.as_slice()];
    let plaintext_bytes = vfs_master_key
        .decrypt(aad, data)
        .context("Could not decrypt Payment")?;

    let payment = serde_json::from_slice::<PaymentV2>(&plaintext_bytes)
        .context("Could not deserialize PaymentV2")?;

    // Validate all plaintext fields match
    let db_id = LxPaymentId::from_str(&db_id).context("invalid db id")?;
    let db_kind = kind.context("version 2 payment missing 'kind' field")?;
    let db_direction =
        direction.context("version 2 payment missing 'direction' field")?;
    // Normally, we'd `amount.context()?` here, but amount can be None for
    // amountless inbound invoice payments.
    let db_fee = fee.context("version 2 payment missing 'fee' field")?;

    ensure!(
        payment.id() == db_id,
        "id mismatch: db={db_id}, payment={}",
        payment.id(),
    );
    ensure!(
        payment.status().as_str() == db_status,
        "status mismatch: db={db_status}, payment={}",
        payment.status().as_str(),
    );
    ensure!(
        payment.kind().to_str() == db_kind,
        "kind mismatch: db={db_kind}, payment={}",
        payment.kind().to_str(),
    );
    ensure!(
        payment.direction().as_str() == db_direction,
        "direction mismatch: db={db_direction}, payment={}",
        payment.direction().as_str(),
    );
    // amount can be None for amountless inbound invoice payments
    ensure!(
        payment.amount() == amount,
        "amount mismatch: db={}, payment={}",
        DisplayOption(amount),
        DisplayOption(payment.amount()),
    );
    ensure!(
        payment.fee() == db_fee,
        "fee mismatch: db={db_fee:?}, payment={:?}",
        payment.fee(),
    );

    Ok(payment)
}

// --- PaymentMetadata --- //

/// Encrypts a [`PaymentMetadata`] to a [`DbPaymentMetadata`].
fn encrypt_metadata(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    metadata: &PaymentMetadata,
    updated_at: TimestampMs,
) -> DbPaymentMetadata {
    let aad = &[];
    let data_size_hint = None;
    let write_data_cb: &dyn Fn(&mut Vec<u8>) = &|buf| {
        serde_json::to_writer(buf, metadata)
            .expect("PaymentMetadata serialization always succeeds")
    };

    let data = vfs_master_key.encrypt(rng, aad, data_size_hint, write_data_cb);

    DbPaymentMetadata {
        id: metadata.id.to_string(),
        data,
        updated_at: updated_at.to_i64(),
    }
}

/// Decrypts a [`DbPaymentMetadata`] to [`PaymentMetadata`].
fn decrypt_metadata(
    vfs_master_key: &AesMasterKey,
    db_metadata: DbPaymentMetadata,
) -> anyhow::Result<PaymentMetadata> {
    let aad = &[];
    let plaintext_bytes = vfs_master_key
        .decrypt(aad, db_metadata.data)
        .context("Could not decrypt PaymentMetadata")?;

    serde_json::from_slice(&plaintext_bytes)
        .context("Could not deserialize PaymentMetadata")
}

#[cfg(test)]
mod test {
    use common::{aes::AesMasterKey, rng::FastRng, time::TimestampMs};
    use proptest::{arbitrary::any, prop_assert_eq, proptest};

    use super::*;

    // encrypt_payment_v1 -> decrypt_payment_v1 = id
    #[test]
    fn payment_v1_encryption_roundtrip() {
        proptest!(|(
            mut rng in any::<FastRng>(),
            vfs_master_key in any::<AesMasterKey>(),
            payment in any::<PaymentV1>(),
            now in any::<TimestampMs>(),
        )| {
            let created_at = payment.created_at();
            let updated_at = now;

            let encrypted = encrypt_payment_v1(
                &mut rng,
                &vfs_master_key,
                &payment,
                created_at,
                updated_at,
            );

            let decrypted =
                decrypt_payment_v1(&vfs_master_key, encrypted).unwrap();

            prop_assert_eq!(payment, decrypted);
        })
    }

    // encrypt_payment_v2 -> decrypt_payment_v2 = id
    #[test]
    fn payment_v2_encryption_roundtrip() {
        proptest!(|(
            mut rng in any::<FastRng>(),
            vfs_master_key in any::<AesMasterKey>(),
            payment in any::<PaymentV2>(),
            now in any::<TimestampMs>(),
        )| {
            let created_at = payment.created_at().unwrap_or(now);
            let updated_at = now;

            let encrypted = encrypt_payment_v2(
                &mut rng,
                &vfs_master_key,
                &payment,
                created_at,
                updated_at,
            );

            let decrypted =
                decrypt_payment_v2(&vfs_master_key, encrypted).unwrap();

            prop_assert_eq!(payment, decrypted);
        })
    }

    // encrypt_metadata -> decrypt_metadata = id
    #[test]
    fn payment_metadata_encryption_roundtrip() {
        proptest!(|(
            mut rng in any::<FastRng>(),
            vfs_master_key in any::<AesMasterKey>(),
            metadata in any::<PaymentMetadata>(),
            now in any::<TimestampMs>(),
        )| {
            let encrypted = encrypt_metadata(
                &mut rng,
                &vfs_master_key,
                &metadata,
                now,
            );

            let decrypted =
                decrypt_metadata(&vfs_master_key, encrypted).unwrap();

            prop_assert_eq!(metadata, decrypted);
        })
    }

    // encrypt_pwm -> decrypt_pwm = id (for PaymentV1 input)
    #[test]
    fn pwm_v1_roundtrip() {
        proptest!(|(
            mut rng in any::<FastRng>(),
            vfs_master_key in any::<AesMasterKey>(),
            payment_v1 in any::<PaymentV1>(),
            now in any::<TimestampMs>(),
        )| {
            let pwm = PaymentWithMetadata::from(payment_v1);
            let created_at = pwm.payment.created_at().unwrap_or(now);
            let updated_at = now;

            let (db_payment, db_metadata) = encrypt_pwm(
                &mut rng,
                &vfs_master_key,
                &pwm,
                created_at,
                updated_at,
            ).unwrap();

            let decrypted =
                decrypt_pwm(&vfs_master_key, db_payment, db_metadata).unwrap();

            prop_assert_eq!(pwm, decrypted);
        })
    }

    // encrypt_pwm -> decrypt_pwm = id (for PaymentV2 + metadata input)
    #[test]
    fn pwm_v2_roundtrip() {
        proptest!(|(
            mut rng in any::<FastRng>(),
            vfs_master_key in any::<AesMasterKey>(),
            payment in any::<PaymentV2>(),
            mut metadata in any::<PaymentMetadata>(),
            now in any::<TimestampMs>(),
        )| {
            metadata.id = payment.id();
            let pwm = PaymentWithMetadata { payment, metadata };
            let created_at = pwm.payment.created_at().unwrap_or(now);
            let updated_at = now;

            let (db_payment, db_metadata) = encrypt_pwm(
                &mut rng,
                &vfs_master_key,
                &pwm,
                created_at,
                updated_at,
            ).unwrap();

            let decrypted =
                decrypt_pwm(&vfs_master_key, db_payment, db_metadata).unwrap();

            prop_assert_eq!(pwm, decrypted);
        })
    }
}
