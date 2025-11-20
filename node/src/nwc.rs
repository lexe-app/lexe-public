use anyhow::Context;
use common::{
    ByteArray,
    aes::AesMasterKey,
    rng::{Crng, SysRng},
    serde_helpers::hexstr_or_bytes,
    time::TimestampMs,
};
use lexe_api::models::nwc::{
    DbNwcWallet, NostrPk, NwcWalletInfo, UpdateDbNwcWalletRequest,
};
use nostr::nips::nip44;
use serde::{Deserialize, Serialize};

const NWC_AAD_PREFIX: &[u8] = b"NWC";
// Relay URL percent-encoded as is part of the .
// TODO(maurice): Maybe we would like to configure this in the future.
const ENCODED_RELAY: &str = "wss%3A%2F%2Frelay.lexe.app";

pub(crate) struct NwcWallet {
    /// The private wallet data.
    /// This data is encrypted when stored in the DB, similarly to VFS files.
    wallet_data: NwcWalletCiphertextData,
    /// The public key of the wallet service on NIP scheme.
    wallet_nostr_pk: NostrPk,
    /// The connection string for the NWC client.
    /// Only available on generation of keys.
    connection_string: Option<String>,
    /// The time the connection was created.
    created_at: TimestampMs,
    /// The time the connection was last updated.
    updated_at: TimestampMs,
    /// Test-only field: the client secret key for testing NIP-44 encryption.
    /// Only available when creating a new connection, not when loading from
    /// DB.
    #[cfg(test)]
    client_secret: Option<[u8; 32]>,
}

/// The NWC wallet data
///
/// The data structure that gets encrypted and stored in the `ciphertext` field.
/// This contains the sensitive information needed to validate and decrypt NWC
/// requests.
///
/// Both the client and the wallet keys are the ones used in the nostr protocol
/// sender and receiver keys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NwcWalletCiphertextData {
    /// The secret key for this NWC wallet (randomly generated).
    /// Used to decrypt and encrypt NIP-44 encrypted requests from the NWC
    /// client.
    #[serde(with = "hexstr_or_bytes")]
    pub wallet_nostr_sk: [u8; 32],
    /// The public key corresponding to client app's `secret`.
    /// When NWC connection string is generated, Node generates a new keypair,
    /// sends the secret to the client app and persists the public key for
    /// future verification.
    #[serde(with = "hexstr_or_bytes")]
    pub client_nostr_pk: [u8; 32],
    /// Human-readable label for this client set by the user.
    pub label: String,
}

impl NwcWallet {
    /// Generate a new NWC wallet.
    ///
    /// Generates secp256k1 keypairs for the wallet service and client for use
    /// with NIP-44 encryption.
    pub(crate) fn new(label: String) -> Self {
        let mut rng = SysRng::new();
        let wallet_service_keys = nostr::Keys::generate_with_rng(&mut rng);
        let client_keys = nostr::Keys::generate_with_rng(&mut rng);

        let wallet_data = NwcWalletCiphertextData {
            wallet_nostr_sk: wallet_service_keys.secret_key().secret_bytes(),
            client_nostr_pk: client_keys.public_key().to_bytes(),
            label,
        };
        let wallet_service_pubkey =
            NostrPk::from_array(wallet_service_keys.public_key().to_bytes());
        let now = TimestampMs::now();

        Self {
            wallet_data,
            wallet_nostr_pk: wallet_service_pubkey,
            created_at: now,
            updated_at: now,
            connection_string: Some(Self::build_connection_string(
                &wallet_service_pubkey,
                &client_keys.secret_key().secret_bytes(),
            )),
            #[cfg(test)]
            client_secret: Some(client_keys.secret_key().secret_bytes()),
        }
    }

    /// Build a NWC wallet from an encrypted DB record.
    ///
    /// Decrypts the ciphertext from the DB record and builds a NWC wallet
    /// from the decrypted data.
    pub(crate) fn from_db(
        vfs_master_key: &AesMasterKey,
        nwc_wallet: DbNwcWallet,
    ) -> anyhow::Result<Self> {
        let wallet_data = decrypt_client(
            vfs_master_key,
            nwc_wallet.wallet_nostr_pk.as_array(),
            nwc_wallet.ciphertext,
        )?;
        let wallet_service_pubkey = nwc_wallet.wallet_nostr_pk;
        let created_at = nwc_wallet.created_at;
        let updated_at = nwc_wallet.updated_at;
        Ok(Self {
            wallet_data,
            wallet_nostr_pk: wallet_service_pubkey,
            created_at,
            updated_at,
            connection_string: None,
            #[cfg(test)]
            client_secret: None,
        })
    }

    fn build_connection_string(
        wallet_nostr_pk: &NostrPk,
        client_secret: &[u8; 32],
    ) -> String {
        let wallet_pk_hex = hex::display(wallet_nostr_pk.as_array());
        let secret_hex = hex::display(client_secret);

        format!(
            "nostr+walletconnect://{wallet_pk_hex}?\
            relay={ENCODED_RELAY}&secret={secret_hex}"
        )
    }

    pub(crate) fn to_nwc_client_info(&self) -> NwcWalletInfo {
        NwcWalletInfo {
            wallet_nostr_pk: self.wallet_nostr_pk,
            label: self.wallet_data.label.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    pub(crate) fn connection_string(&self) -> Option<&str> {
        self.connection_string.as_deref()
    }

    pub(crate) fn label(&self) -> &str {
        &self.wallet_data.label
    }

    pub(crate) fn to_req(
        &self,
        rng: &mut impl Crng,
        vfs_master_key: &AesMasterKey,
    ) -> UpdateDbNwcWalletRequest {
        let ciphertext = encrypt_client(
            rng,
            vfs_master_key,
            self.wallet_nostr_pk.as_array(),
            &self.wallet_data,
        );

        UpdateDbNwcWalletRequest {
            wallet_nostr_pk: self.wallet_nostr_pk,
            ciphertext,
        }
    }

    pub(crate) fn update_label(&mut self, label: String) {
        self.wallet_data.label = label;
    }

    /// Get the wallet service secret key for NIP-44 encryption/decryption.
    fn get_wallet_nostr_sk(&self) -> anyhow::Result<nostr::SecretKey> {
        nostr::SecretKey::from_slice(&self.wallet_data.wallet_nostr_sk).context(
            "Failed to convert wallet service secret key to Nostr secret key",
        )
    }

    fn get_client_nostr_pk(&self) -> anyhow::Result<nostr::PublicKey> {
        nostr::PublicKey::from_slice(self.wallet_data.client_nostr_pk.as_ref())
            .context("Invalid client nostr public key")
    }

    pub(crate) fn validate_client_nostr_pk(
        &self,
        client_nostr_pk: &NostrPk,
    ) -> anyhow::Result<()> {
        let client_nostr_pk =
            nostr::PublicKey::from_slice(client_nostr_pk.as_ref())
                .expect("Invalid client nostr public key");
        let our_client_nostr_pk = self.get_client_nostr_pk()?;
        if client_nostr_pk != our_client_nostr_pk {
            return Err(anyhow::anyhow!(
                "Client nostr pk does not match stored client nostr pk"
            ));
        }
        Ok(())
    }

    /// Decrypt a NIP-44 encrypted NWC request.
    pub(crate) fn decrypt_nip44_request(
        &self,
        encrypted_payload: &[u8],
    ) -> anyhow::Result<String> {
        let wallet_nostr_sk = self.get_wallet_nostr_sk()?;
        let client_nostr_pk = self.get_client_nostr_pk()?;

        nip44::decrypt(&wallet_nostr_sk, &client_nostr_pk, encrypted_payload)
            .context("Failed to decrypt NIP-44 payload")
    }

    /// Encrypt a NWC response using NIP-44.
    pub(crate) fn encrypt_nip44_response(
        &self,
        response_json: &str,
    ) -> anyhow::Result<Vec<u8>> {
        let wallet_nostr_sk = self.get_wallet_nostr_sk()?;
        let client_nostr_pk = self.get_client_nostr_pk()?;

        let encrypted_string = nip44::encrypt(
            &wallet_nostr_sk,
            &client_nostr_pk,
            response_json,
            nip44::Version::default(),
        )
        .context("Failed to encrypt NIP-44 payload")?;

        Ok(encrypted_string.into_bytes())
    }
}

/// Encrypt the NWC wallet data using node's master key.
///
/// Serializes the wallet data into JSON, encrypts it, and returns the
/// encrypted data bytes.
///
/// We use the wallet_nostr_pk as the AAD, to ensure that the encrypted
/// data is only decryptable by the node that owns the connection_pubkey.
fn encrypt_client(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    wallet_nostr_pk: &[u8; 32],
    client_data: &NwcWalletCiphertextData,
) -> Vec<u8> {
    let aad = &[NWC_AAD_PREFIX, wallet_nostr_pk.as_slice()];
    vfs_master_key.encrypt(rng, aad, None, &|out: &mut Vec<u8>| {
        serde_json::to_writer(out, &client_data)
            .expect("JSON serialization was not implemented correctly");
    })
}

/// Decrypt the NWC wallet data using node's master key.
///
/// Decrypts the encrypted data bytes, deserializes the JSON, and returns the
/// decrypted client data.
fn decrypt_client(
    vfs_master_key: &AesMasterKey,
    wallet_nostr_pk: &[u8; 32],
    data: Vec<u8>,
) -> anyhow::Result<NwcWalletCiphertextData> {
    let aad = &[NWC_AAD_PREFIX, wallet_nostr_pk.as_slice()];
    let value = vfs_master_key.decrypt(aad, data).with_context(|| {
        format!(
            "Failed to decrypt NWC client data {}",
            hex::display(wallet_nostr_pk)
        )
    })?;
    serde_json::from_slice(&value).with_context(|| {
        format!(
            "Failed to deserialize NWC client data, {}",
            hex::display(wallet_nostr_pk)
        )
    })
}

#[cfg(test)]
mod test {
    use common::{aes::AesMasterKey, rng::SysRng, time::TimestampMs};
    use lexe_api::models::nwc::DbNwcWallet;

    use super::*;

    fn test_master_key() -> AesMasterKey {
        let key_bytes = [42u8; 32];
        AesMasterKey::new(&key_bytes)
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let mut rng = SysRng::new();
        let master_key = test_master_key();

        let client_data = NwcWalletCiphertextData {
            wallet_nostr_sk: [1u8; 32],
            client_nostr_pk: [2u8; 32],
            label: "Test Connection".to_string(),
        };
        let connection_pubkey = [3u8; 32];

        let encrypted = encrypt_client(
            &mut rng,
            &master_key,
            &connection_pubkey,
            &client_data,
        );
        let decrypted =
            decrypt_client(&master_key, &connection_pubkey, encrypted)
                .expect("Decryption should succeed");

        assert_eq!(client_data, decrypted);
    }

    #[test]
    fn test_decrypt_with_wrong_key_fails() {
        let mut rng = SysRng::new();
        let master_key = test_master_key();
        let wrong_key = AesMasterKey::new(&[99u8; 32]);

        let client_data = NwcWalletCiphertextData {
            wallet_nostr_sk: [1u8; 32],
            client_nostr_pk: [2u8; 32],
            label: "Test".to_string(),
        };
        let connection_pubkey = [3u8; 32];

        let encrypted = encrypt_client(
            &mut rng,
            &master_key,
            &connection_pubkey,
            &client_data,
        );
        let result = decrypt_client(&wrong_key, &connection_pubkey, encrypted);

        assert!(result.is_err(), "Decryption with wrong key should fail");
    }

    #[test]
    fn test_nwc_connection_new() {
        let label = "My NWC Connection".to_string();

        let connection = NwcWallet::new(label.clone());

        assert_eq!(connection.wallet_data.label, label);
        assert!(connection.connection_string.is_some());

        let conn_str = connection.connection_string.unwrap();
        assert!(conn_str.starts_with("nostr+walletconnect://"));
        // Relay URL should be percent-encoded
        assert!(conn_str.contains("relay=wss%3A%2F%2Frelay.lexe.app"));
        assert!(conn_str.contains("secret="));
    }

    #[test]
    fn test_connection_string_format() {
        let connection = NwcWallet::new("test".to_string());
        let conn_str = connection
            .connection_string
            .expect("Should have connection string");

        let parts: Vec<&str> = conn_str.split('?').collect();
        assert_eq!(parts.len(), 2, "Should have URI and query params");

        assert!(parts[0].starts_with("nostr+walletconnect://"));
        let wallet_pk_hex = &parts[0]["nostr+walletconnect://".len()..];
        assert_eq!(wallet_pk_hex.len(), 64, "Wallet PK should be 32 bytes hex");

        let query = parts[1];
        // Relay URL should be percent-encoded
        assert!(query.contains("relay=wss%3A%2F%2Frelay.lexe.app"));
        assert!(query.contains("secret="));

        let secret_param = query
            .split('&')
            .find(|p| p.starts_with("secret="))
            .expect("Should have secret param");
        let secret_hex = &secret_param["secret=".len()..];
        assert_eq!(secret_hex.len(), 64, "Secret should be 32 bytes hex");
    }

    #[test]
    fn test_from_db_to_req_roundtrip() {
        let mut rng = SysRng::new();
        let master_key = test_master_key();
        let label = "Test Label".to_string();

        let connection = NwcWallet::new(label.clone());
        let req = connection.to_req(&mut rng, &master_key);

        let nwc_wallet = DbNwcWallet {
            wallet_nostr_pk: req.wallet_nostr_pk,
            ciphertext: req.ciphertext,
            created_at: connection.created_at,
            updated_at: connection.updated_at,
        };

        let restored = NwcWallet::from_db(&master_key, nwc_wallet)
            .expect("Should restore connection");

        assert_eq!(
            connection.wallet_data.wallet_nostr_sk,
            restored.wallet_data.wallet_nostr_sk
        );
        assert_eq!(
            connection.wallet_data.client_nostr_pk,
            restored.wallet_data.client_nostr_pk
        );
        assert_eq!(connection.wallet_data.label, restored.wallet_data.label);
        assert_eq!(connection.wallet_nostr_pk, restored.wallet_nostr_pk);
        assert!(restored.connection_string.is_none());
    }

    #[test]
    fn test_update_label() {
        let mut connection = NwcWallet::new("Original Label".to_string());

        assert_eq!(connection.wallet_data.label, "Original Label");

        connection.update_label("Updated Label".to_string());
        assert_eq!(connection.wallet_data.label, "Updated Label");
    }

    #[test]
    fn test_to_nwc_client_info() {
        let label = "Test Connection".to_string();
        let connection = NwcWallet::new(label.clone());

        let info = connection.to_nwc_client_info();

        assert_eq!(info.label, label);
        assert_eq!(info.wallet_nostr_pk, connection.wallet_nostr_pk);
        assert_eq!(info.created_at, connection.created_at);
        assert_eq!(info.updated_at, connection.updated_at);
    }

    #[test]
    fn test_to_nwc_client_info_from_db() {
        let mut rng = SysRng::new();
        let master_key = test_master_key();
        let label = "Test".to_string();

        let connection = NwcWallet::new(label.clone());
        let req = connection.to_req(&mut rng, &master_key);

        let nwc_wallet = DbNwcWallet {
            wallet_nostr_pk: req.wallet_nostr_pk,
            ciphertext: req.ciphertext,
            created_at: TimestampMs::now(),
            updated_at: TimestampMs::now(),
        };

        let restored = NwcWallet::from_db(&master_key, nwc_wallet)
            .expect("Should restore");
        let info = restored.to_nwc_client_info();

        assert_eq!(info.label, label);
        assert_eq!(info.wallet_nostr_pk, restored.wallet_nostr_pk);
    }

    #[test]
    fn test_nip44_request_response_roundtrip() {
        let nwc_wallet = NwcWallet::new("Test Wallet".to_string());

        // Use test-only field to get client secret key
        let client_secret = nostr::SecretKey::from_slice(
            &nwc_wallet
                .client_secret
                .expect("Should have test secret when created with new()"),
        )
        .expect("Valid client secret");

        let client_keys = nostr::Keys::new(client_secret);

        // Get wallet service public key
        let wallet_service_pk =
            nostr::PublicKey::from_slice(nwc_wallet.wallet_nostr_pk.as_ref())
                .expect("Valid wallet service public key");

        // Simulate a NWC request payload
        let request_payload = r#"{"method":"get_info","params":{}}"#;

        // Client encrypts request
        let encrypted_request = nostr::nips::nip44::encrypt(
            client_keys.secret_key(),
            &wallet_service_pk,
            request_payload,
            nostr::nips::nip44::Version::default(),
        )
        .expect("Client encryption should succeed");

        // Wallet service decrypts the request
        let decrypted_request = nwc_wallet
            .decrypt_nip44_request(encrypted_request.as_bytes())
            .expect("Wallet decryption should succeed");

        assert_eq!(decrypted_request, request_payload);

        // Simulate a response from the wallet service
        let response_payload =
            r#"{"result":{"alias":"test","network":"bitcoin"},"error":null}"#;

        // Wallet service encrypts response
        let encrypted_response = nwc_wallet
            .encrypt_nip44_response(response_payload)
            .expect("Wallet response encryption should succeed");

        // Client decrypts response
        let decrypted_response = nostr::nips::nip44::decrypt(
            client_keys.secret_key(),
            &wallet_service_pk,
            &encrypted_response,
        )
        .expect("Client response decryption should succeed");

        assert_eq!(decrypted_response, response_payload);
    }
}
