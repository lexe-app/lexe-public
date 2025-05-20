use anyhow::{ensure, Context};
use common::{aes::AesMasterKey, rng::Crng};
use lexe_api::vfs::{VfsFile, VfsFileId};
use lightning::util::ser::Writeable;
use serde::{de::DeserializeOwned, Serialize};
use tracing::info;

/// Serializes a LDK [`Writeable`] to bytes, encrypts the serialized bytes, and
/// packages everything up into a [`VfsFile`] which is ready to be persisted.
pub fn encrypt_ldk_writeable(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    file_id: VfsFileId,
    writeable: &impl Writeable,
) -> VfsFile {
    encrypt_file(rng, vfs_master_key, file_id, &|mut_vec_u8| {
        // - Writeable can write to any LDK lightning::util::ser::Writer
        // - Writer is impl'd for all types that impl std::io::Write
        // - Write is impl'd for Vec<u8>
        // Therefore a Writeable can be written to a Vec<u8>.
        writeable
            .write(mut_vec_u8)
            .expect("Serialization into an in-memory buffer should never fail");
    })
}

/// Serializes an object to JSON bytes, encrypts the serialized bytes, and
/// packages everything up into a [`VfsFile`] which is ready to be persisted.
pub fn encrypt_json(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    file_id: VfsFileId,
    value: &impl Serialize,
) -> VfsFile {
    encrypt_file(rng, vfs_master_key, file_id, &|mut_vec_u8| {
        serde_json::to_writer(mut_vec_u8, value)
            .expect("JSON serialization was not implemented correctly");
    })
}

/// Encrypt some arbitrary plaintext bytes to a [`VfsFile`].
///
/// You should prefer [`encrypt_json`] and [`encrypt_ldk_writeable`] over this,
/// since those fns avoid the need to write to an intermediate plaintext buffer.
pub fn encrypt_plaintext_bytes(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    file_id: VfsFileId,
    plaintext_bytes: &[u8],
) -> VfsFile {
    encrypt_file(rng, vfs_master_key, file_id, &|mut_vec_u8| {
        mut_vec_u8.extend(plaintext_bytes)
    })
}

fn encrypt_file(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    file_id: VfsFileId,
    write_data_cb: &dyn Fn(&mut Vec<u8>),
) -> VfsFile {
    // bind the dirname and filename so files can't be moved around. the
    // owner identity is already bound by the key derivation path.
    //
    // this is only a best-effort mitigation however. files in an untrusted
    // storage can still be deleted or rolled back to an earlier version
    // without detection currently.
    let dirname = &file_id.dir.dirname;
    let filename = &file_id.filename;
    let aad = &[dirname.as_bytes(), filename.as_bytes()];
    let data_size_hint = None;
    let data = vfs_master_key.encrypt(rng, aad, data_size_hint, write_data_cb);

    // Print a warning if the ciphertext is greater than 1 MB.
    // We are interested in large LDK types as well as the WalletDb.
    let data_len = data.len();
    if data_len > 1_000_000 {
        info!("{dirname}/{filename} is >1MB: {data_len} bytes");
    }

    VfsFile { id: file_id, data }
}

/// Decrypt a file previously encrypted using `encrypt_file`.
///
/// Since the file is probably coming from an untrusted source, be sure to pass
/// in an `expected_file_id` which contains the `dirname` and `filename` that we
/// expect. The `returned_file` which came from the untrusted DB will be
/// validated against the `expected_file_id`.
///
/// If successful, returns the decrypted plaintext bytes contained in the file.
pub fn decrypt_file(
    vfs_master_key: &AesMasterKey,
    expected_file_id: &VfsFileId,
    returned_file: VfsFile,
) -> anyhow::Result<Vec<u8>> {
    let dirname = &expected_file_id.dir.dirname;
    let filename = &expected_file_id.filename;
    let returned_dirname = &returned_file.id.dir.dirname;
    let returned_filename = &returned_file.id.filename;
    ensure!(
        returned_dirname == dirname,
        "Dirnames don' match: {returned_dirname} != {dirname}"
    );
    ensure!(
        returned_filename == filename,
        "Filenames don' match: {returned_filename} != {filename}"
    );

    let aad = &[dirname.as_bytes(), filename.as_bytes()];
    vfs_master_key
        .decrypt(aad, returned_file.data)
        .with_context(|| format!("{expected_file_id}"))
        .context("Failed to decrypt encrypted VFS file")
}

/// Exactly [`decrypt_file`], but also attempts to deserialize the decrypted
/// JSON plaintext bytes into the expected type.
#[inline]
pub fn decrypt_json_file<D: DeserializeOwned>(
    vfs_master_key: &AesMasterKey,
    expected_file_id: &VfsFileId,
    returned_file: VfsFile,
) -> anyhow::Result<D> {
    let json_bytes =
        decrypt_file(vfs_master_key, expected_file_id, returned_file)
            .context("Decryption failed")?;
    let value = serde_json::from_slice(json_bytes.as_slice())
        .with_context(|| format!("{expected_file_id}"))
        .context("JSON deserialization failed")?;

    Ok(value)
}
