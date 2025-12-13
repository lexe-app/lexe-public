use anyhow::{Context, anyhow};
use bitcoin::base64::{self, Engine};
use common::{
    ByteArray,
    aes::AesMasterKey,
    rng::{Crng, SysRng},
    serde_helpers::hexstr_or_bytes,
    time::TimestampMs,
};
use lexe_api::models::nwc::{
    DbNwcClient, NostrEventId, NostrPk, NostrSignedEvent, NwcClientInfo,
    UpdateDbNwcClientRequest,
};
use nostr::nips::nip44;
use serde::{Deserialize, Serialize};

const NWC_AAD_PREFIX: &[u8] = b"NWC";
// Relay URL percent-encoded as is part of the .
// TODO(maurice): Maybe we would like to configure this in the future.
const ENCODED_RELAY: &str = "wss%3A%2F%2Frelay.lexe.app";

pub(crate) struct NwcClient {
    /// The private client data.
    /// This data is encrypted when stored in the DB, similarly to VFS files.
    client_data: NwcClientCiphertextData,
    /// The public key of the wallet service on NIP scheme.
    wallet_nostr_pk: NostrPk,
    /// The public key of the client on NIP scheme.
    client_nostr_pk: NostrPk,
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

/// The NWC client data
///
/// The data structure that gets encrypted and stored in the `ciphertext` field.
/// This contains the sensitive information needed to validate and decrypt NWC
/// requests.
///
/// Both the client and the wallet keys are the ones used in the nostr protocol
/// sender and receiver keys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NwcClientCiphertextData {
    /// The secret key for this NWC wallet (randomly generated).
    /// Used to decrypt and encrypt NIP-44 encrypted requests from the NWC
    /// client.
    #[serde(with = "hexstr_or_bytes")]
    pub wallet_nostr_sk: [u8; 32],
    /// Human-readable label for this client set by the user.
    pub label: String,
}

impl NwcClient {
    /// Generate a new NWC client.
    ///
    /// Generates secp256k1 keypairs for the wallet service and client for use
    /// with NIP-44 encryption.
    pub(crate) fn new(label: String) -> Self {
        let mut rng = SysRng::new();
        let wallet_nostr_keys = nostr::Keys::generate_with_rng(&mut rng);
        let client_nostr_keys = nostr::Keys::generate_with_rng(&mut rng);

        let client_data = NwcClientCiphertextData {
            wallet_nostr_sk: wallet_nostr_keys.secret_key().secret_bytes(),
            label,
        };
        let wallet_nostr_pk =
            NostrPk::from_array(wallet_nostr_keys.public_key().to_bytes());
        let client_nostr_pk =
            NostrPk::from_array(client_nostr_keys.public_key().to_bytes());
        let now = TimestampMs::now();

        Self {
            client_data,
            wallet_nostr_pk,
            client_nostr_pk,
            created_at: now,
            updated_at: now,
            connection_string: Some(Self::build_connection_string(
                &wallet_nostr_pk,
                &client_nostr_keys.secret_key().secret_bytes(),
            )),
            #[cfg(test)]
            client_secret: Some(client_nostr_keys.secret_key().secret_bytes()),
        }
    }

    /// Build a NWC client from an encrypted DB record.
    ///
    /// Decrypts the ciphertext from the DB record and builds a NWC client
    /// from the decrypted data.
    pub(crate) fn from_db(
        vfs_master_key: &AesMasterKey,
        nwc_client: DbNwcClient,
    ) -> anyhow::Result<Self> {
        let wallet_data = decrypt_client(
            vfs_master_key,
            nwc_client.wallet_nostr_pk.as_array(),
            nwc_client.ciphertext,
        )?;
        Ok(Self {
            client_data: wallet_data,
            wallet_nostr_pk: nwc_client.wallet_nostr_pk,
            client_nostr_pk: nwc_client.client_nostr_pk,
            created_at: nwc_client.created_at,
            updated_at: nwc_client.updated_at,
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

    pub(crate) fn to_nwc_client_info(&self) -> NwcClientInfo {
        NwcClientInfo {
            client_nostr_pk: self.client_nostr_pk,
            wallet_nostr_pk: self.wallet_nostr_pk,
            label: self.client_data.label.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    pub(crate) fn connection_string(&self) -> Option<&str> {
        self.connection_string.as_deref()
    }

    pub(crate) fn label(&self) -> &str {
        &self.client_data.label
    }

    pub(crate) fn to_req(
        &self,
        rng: &mut impl Crng,
        vfs_master_key: &AesMasterKey,
    ) -> UpdateDbNwcClientRequest {
        let ciphertext = encrypt_client(
            rng,
            vfs_master_key,
            self.wallet_nostr_pk.as_array(),
            &self.client_data,
        );

        UpdateDbNwcClientRequest {
            client_nostr_pk: self.client_nostr_pk,
            wallet_nostr_pk: self.wallet_nostr_pk,
            ciphertext,
        }
    }

    pub(crate) fn update_label(&mut self, label: Option<String>) {
        if let Some(label) = label {
            self.client_data.label = label;
        }
    }

    /// Get the wallet service secret key for NIP-44 encryption/decryption.
    fn get_wallet_nostr_sk(&self) -> anyhow::Result<nostr::SecretKey> {
        nostr::SecretKey::from_slice(&self.client_data.wallet_nostr_sk).context(
            "Failed to convert wallet service secret key to Nostr secret key",
        )
    }

    fn get_client_nostr_pk(&self) -> anyhow::Result<nostr::PublicKey> {
        nostr::PublicKey::from_slice(self.client_nostr_pk.as_ref())
            .context("Invalid client nostr public key")
    }

    fn get_wallet_nostr_pk(&self) -> anyhow::Result<nostr::PublicKey> {
        nostr::PublicKey::from_slice(self.wallet_nostr_pk.as_ref())
            .context("Invalid wallet nostr public key")
    }

    pub(crate) fn validate_wallet_nostr_pk(
        &self,
        wallet_nostr_pk: &NostrPk,
    ) -> anyhow::Result<()> {
        let wallet_nostr_pk =
            nostr::PublicKey::from_slice(wallet_nostr_pk.as_ref())
                .expect("Invalid wallet nostr public key");
        let our_wallet_nostr_pk = self.get_wallet_nostr_pk()?;
        if wallet_nostr_pk != our_wallet_nostr_pk {
            return Err(anyhow!(
                "Wallet nostr pk does not match stored wallet nostr pk"
            ));
        }
        Ok(())
    }

    /// Decrypt a NIP-44 encrypted NWC request.
    ///
    /// [NIP-44] uses ECDH to derive a shared secret from both parties'
    /// keypairs, then authenticated encryption (ChaCha20 + HMAC-SHA256).
    /// The HMAC provides integrity: decryption fails if the payload was
    /// encrypted with different keys, protecting against spoofed client pks.
    ///
    /// [NIP-44]: https://github.com/nostr-protocol/nips/blob/master/44.md
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
    /// See [`decrypt_nip44_request`](Self::decrypt_nip44_request) for details.
    pub(crate) fn encrypt_nip44_response(
        &self,
        response_json: &str,
    ) -> anyhow::Result<String> {
        let wallet_nostr_sk = self.get_wallet_nostr_sk()?;
        let client_nostr_pk = self.get_client_nostr_pk()?;

        let encrypted_string = nip44::encrypt(
            &wallet_nostr_sk,
            &client_nostr_pk,
            response_json,
            nip44::Version::default(),
        )
        .context("Failed to encrypt NIP-44 payload")?;

        Ok(encrypted_string)
    }

    pub(crate) fn build_response(
        &self,
        event_id: NostrEventId,
        content: String,
    ) -> anyhow::Result<NostrSignedEvent> {
        let keys = nostr::Keys::new(self.get_wallet_nostr_sk()?);
        let event_id = nostr::event::EventId::from_byte_array(event_id.0);
        let event = nostr::EventBuilder::new(
            nostr::Kind::WalletConnectResponse,
            content,
        )
        .tag(nostr::Tag::public_key(self.get_client_nostr_pk()?))
        .tag(nostr::Tag::event(event_id))
        .sign_with_keys(&keys)?;
        let event = serde_json::to_string(&event)
            .context("Failed to serialize nwc response")?;
        let event = base64::engine::general_purpose::STANDARD_NO_PAD
            .encode(event.as_bytes());

        Ok(NostrSignedEvent {
            event: event.into_bytes(),
        })
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
    client_data: &NwcClientCiphertextData,
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
) -> anyhow::Result<NwcClientCiphertextData> {
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
    use lexe_api::models::nwc::DbNwcClient;

    use super::*;

    fn test_master_key() -> AesMasterKey {
        let key_bytes = [42u8; 32];
        AesMasterKey::new(&key_bytes)
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let mut rng = SysRng::new();
        let master_key = test_master_key();

        let client_data = NwcClientCiphertextData {
            wallet_nostr_sk: [1u8; 32],
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

        let client_data = NwcClientCiphertextData {
            wallet_nostr_sk: [1u8; 32],
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

        let client = NwcClient::new(label.clone());

        assert_eq!(client.client_data.label, label);
        assert!(client.connection_string.is_some());

        let conn_str = client.connection_string.unwrap();
        assert!(conn_str.starts_with("nostr+walletconnect://"));
        // Relay URL should be percent-encoded
        assert!(conn_str.contains("relay=wss%3A%2F%2Frelay.lexe.app"));
        assert!(conn_str.contains("secret="));
    }

    #[test]
    fn test_connection_string_format() {
        let client = NwcClient::new("test".to_string());
        let conn_str = client
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

        let client = NwcClient::new(label.clone());
        let req = client.to_req(&mut rng, &master_key);

        let nwc_wallet = DbNwcClient {
            client_nostr_pk: req.client_nostr_pk,
            wallet_nostr_pk: req.wallet_nostr_pk,
            ciphertext: req.ciphertext,
            created_at: client.created_at,
            updated_at: client.updated_at,
        };

        let restored = NwcClient::from_db(&master_key, nwc_wallet)
            .expect("Should restore connection");

        assert_eq!(
            client.client_data.wallet_nostr_sk,
            restored.client_data.wallet_nostr_sk
        );
        assert_eq!(client.client_nostr_pk, restored.client_nostr_pk);
        assert_eq!(client.client_data.label, restored.client_data.label);
        assert_eq!(client.wallet_nostr_pk, restored.wallet_nostr_pk);
        assert!(restored.connection_string.is_none());
    }

    #[test]
    fn test_update_label() {
        let mut connection = NwcClient::new("Original Label".to_string());

        assert_eq!(connection.client_data.label, "Original Label");

        connection.update_label(Some("Updated Label".to_string()));
        assert_eq!(connection.client_data.label, "Updated Label");
    }

    #[test]
    fn test_to_nwc_client_info() {
        let label = "Test Connection".to_string();
        let client = NwcClient::new(label.clone());

        let info = client.to_nwc_client_info();

        assert_eq!(info.label, label);
        assert_eq!(info.wallet_nostr_pk, client.wallet_nostr_pk);
        assert_eq!(info.created_at, client.created_at);
        assert_eq!(info.updated_at, client.updated_at);
    }

    #[test]
    fn test_to_nwc_client_info_from_db() {
        let mut rng = SysRng::new();
        let master_key = test_master_key();
        let label = "Test".to_string();

        let client = NwcClient::new(label.clone());
        let req = client.to_req(&mut rng, &master_key);

        let nwc_client = DbNwcClient {
            client_nostr_pk: req.client_nostr_pk,
            wallet_nostr_pk: req.wallet_nostr_pk,
            ciphertext: req.ciphertext,
            created_at: TimestampMs::now(),
            updated_at: TimestampMs::now(),
        };

        let restored = NwcClient::from_db(&master_key, nwc_client)
            .expect("Should restore");
        let info = restored.to_nwc_client_info();

        assert_eq!(info.label, label);
        assert_eq!(info.wallet_nostr_pk, restored.wallet_nostr_pk);
        assert_eq!(info.client_nostr_pk, restored.client_nostr_pk);
    }

    #[test]
    fn test_nip44_request_response_roundtrip() {
        let nwc_wallet = NwcClient::new("Test Wallet".to_string());

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
