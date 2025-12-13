use std::time::Instant;

use anyhow::Context;
use bitcoin::base64::{self, Engine};
use byte_array::ByteArray;
use common::{aes::AesMasterKey, env::DeployEnv, rng::Crng, time::TimestampMs};
use lexe_api::models::nwc::{
    DbNwcClient, DbNwcClientFields, NostrEventId, NostrPk, NostrSignedEvent,
    NostrSk, NwcClientInfo,
};
use nostr::nips::nip44;
use serde::{Deserialize, Serialize};

const NWC_AAD_PREFIX: &[u8] = b"NWC";

/// Relay URLs, percent-encoded for use in NWC connection strings.
const DEV_RELAY: &str = "wss%3A%2F%2Frelay.dev.lexe.app";
const STAGING_RELAY: &str = "wss%3A%2F%2Frelay.staging.lexe.app";
const PROD_RELAY: &str = "wss%3A%2F%2Frelay.lexe.app";

/// Returns the percent-encoded relay URL for the given deploy environment.
pub fn relay_url(deploy_env: DeployEnv) -> &'static str {
    match deploy_env {
        DeployEnv::Dev => DEV_RELAY,
        DeployEnv::Staging => STAGING_RELAY,
        DeployEnv::Prod => PROD_RELAY,
    }
}

/// Helpers to convert between our Nostr types and the `nostr` crate's types.
/// Lives here because `lexe-api-core` doesn't depend on the `nostr` crate.
pub(crate) mod convert {
    use super::*;

    // Both `NostrPk` and `NostrSk` are 32-byte arrays with no additional
    // validation. The `nostr` crate's `from_slice` methods only check the
    // length, so these conversions are infallible. Proptests ensure we're
    // notified if upstream ever adds validation.

    pub fn to_nostr_pk(pk: &NostrPk) -> nostr::PublicKey {
        nostr::PublicKey::from_slice(pk.as_ref())
            .expect("NostrPk is 32 bytes, conversion is infallible")
    }

    #[cfg(test)]
    pub fn from_nostr_pk(pk: nostr::PublicKey) -> NostrPk {
        NostrPk::from_array(pk.to_bytes())
    }

    pub fn to_nostr_sk(sk: &NostrSk) -> nostr::SecretKey {
        nostr::SecretKey::from_slice(sk.as_ref())
            .expect("NostrSk is 32 bytes, conversion is infallible")
    }

    #[cfg(test)]
    pub fn from_nostr_sk(sk: nostr::SecretKey) -> NostrSk {
        NostrSk::from_array(sk.secret_bytes())
    }
}

/// An NWC (Nostr Wallet Connect) client connection.
///
/// Represents a connection between an external NWC client app and this wallet.
/// The wallet acts as the "wallet service" in NIP-47 terminology.
pub(crate) struct NwcClient {
    /// The NWC client app's Nostr public key (the client key).
    client_nostr_pk: NostrPk,
    /// The wallet service's Nostr public key (the server key).
    wallet_nostr_pk: NostrPk,
    /// Sensitive data that gets encrypted when stored in the DB.
    data: NwcClientData,
    /// The time the connection was created.
    created_at: TimestampMs,
    /// The time the connection was last updated.
    updated_at: TimestampMs,
}

/// Sensitive NWC client data that gets encrypted before storage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NwcClientData {
    /// The wallet service's Nostr secret key (randomly generated).
    /// Used with NIP-44 to decrypt requests from and encrypt responses to
    /// the NWC client app.
    pub wallet_nostr_sk: NostrSk,
    /// Human-readable label for this connection, set by the user.
    pub label: String,
}

impl NwcClientData {
    /// VFS-encrypts this data.
    fn encrypt(
        &self,
        rng: &mut impl Crng,
        vfs_master_key: &AesMasterKey,
        wallet_nostr_pk: &NostrPk,
    ) -> Vec<u8> {
        let aad = &[NWC_AAD_PREFIX, wallet_nostr_pk.as_slice()];
        vfs_master_key.encrypt(rng, aad, None, &|out: &mut Vec<u8>| {
            serde_json::to_writer(out, self)
                .expect("JSON serialization was not implemented correctly");
        })
    }

    /// VFS-decrypts this data.
    fn decrypt(
        vfs_master_key: &AesMasterKey,
        wallet_nostr_pk: &NostrPk,
        data: Vec<u8>,
    ) -> anyhow::Result<Self> {
        let aad = &[NWC_AAD_PREFIX, wallet_nostr_pk.as_slice()];
        let value = vfs_master_key
            .decrypt(aad, data)
            .context("Failed to decrypt NWC client data")
            .with_context(|| wallet_nostr_pk.to_string())?;
        serde_json::from_slice(&value)
            .context("Failed to deserialize NWC client data")
            .with_context(|| wallet_nostr_pk.to_string())
    }
}

impl NwcClient {
    /// Generate a new NWC client and its connection string.
    ///
    /// Generates Nostr keypairs for the wallet service and client for use with
    /// NIP-44 encryption. Returns the client and the connection string (which
    /// contains the client secret key needed by the NWC app).
    pub(crate) fn new(
        rng: &mut impl Crng,
        deploy_env: DeployEnv,
        label: String,
    ) -> (Self, String) {
        let wallet_nostr_keys = nostr::Keys::generate_with_rng(rng);
        let client_nostr_keys = nostr::Keys::generate_with_rng(rng);

        let wallet_nostr_sk =
            NostrSk::from_array(wallet_nostr_keys.secret_key().secret_bytes());
        let wallet_nostr_pk =
            NostrPk::from_array(wallet_nostr_keys.public_key().to_bytes());
        let client_nostr_sk =
            NostrSk::from_array(client_nostr_keys.secret_key().secret_bytes());
        let client_nostr_pk =
            NostrPk::from_array(client_nostr_keys.public_key().to_bytes());
        let now = TimestampMs::now();

        let connection_string = Self::build_connection_string(
            deploy_env,
            &wallet_nostr_pk,
            &client_nostr_sk,
        );

        let client = Self {
            client_nostr_pk,
            wallet_nostr_pk,
            data: NwcClientData {
                wallet_nostr_sk,
                label,
            },
            created_at: now,
            updated_at: now,
        };

        (client, connection_string)
    }

    /// Decrypt an [`NwcClient`] from an encrypted [`DbNwcClient`] record.
    pub(crate) fn decrypt(
        vfs_master_key: &AesMasterKey,
        nwc_client: DbNwcClient,
    ) -> anyhow::Result<Self> {
        let data = NwcClientData::decrypt(
            vfs_master_key,
            &nwc_client.fields.wallet_nostr_pk,
            nwc_client.fields.ciphertext,
        )?;
        Ok(Self {
            client_nostr_pk: nwc_client.fields.client_nostr_pk,
            wallet_nostr_pk: nwc_client.fields.wallet_nostr_pk,
            data,
            created_at: nwc_client.created_at,
            updated_at: nwc_client.updated_at,
        })
    }

    fn build_connection_string(
        deploy_env: DeployEnv,
        wallet_nostr_pk: &NostrPk,
        client_nostr_sk: &NostrSk,
    ) -> String {
        let relay = relay_url(deploy_env);
        let wallet_nostr_pk = hex::display(wallet_nostr_pk.as_array());
        let client_nostr_sk = hex::display(client_nostr_sk.as_array());

        format!(
            "nostr+walletconnect://{wallet_nostr_pk}\
             ?relay={relay}&secret={client_nostr_sk}"
        )
    }

    /// Convert to the public client info struct (for API responses).
    pub(crate) fn to_nwc_client_info(&self) -> NwcClientInfo {
        NwcClientInfo {
            client_nostr_pk: self.client_nostr_pk,
            wallet_nostr_pk: self.wallet_nostr_pk,
            label: self.data.label.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    pub(crate) fn label(&self) -> &str {
        &self.data.label
    }

    /// Encrypt the client data for storage in the DB.
    pub(crate) fn encrypt(
        &self,
        rng: &mut impl Crng,
        vfs_master_key: &AesMasterKey,
    ) -> DbNwcClientFields {
        let ciphertext =
            self.data
                .encrypt(rng, vfs_master_key, &self.wallet_nostr_pk);
        DbNwcClientFields {
            client_nostr_pk: self.client_nostr_pk,
            wallet_nostr_pk: self.wallet_nostr_pk,
            ciphertext,
        }
    }

    pub(crate) fn update_label(&mut self, label: Option<String>) {
        if let Some(label) = label {
            self.data.label = label;
        }
    }

    /// Get the wallet service secret key.
    fn wallet_nostr_sk(&self) -> &NostrSk {
        &self.data.wallet_nostr_sk
    }

    pub(crate) fn client_nostr_pk(&self) -> &NostrPk {
        &self.client_nostr_pk
    }

    pub(crate) fn wallet_nostr_pk(&self) -> &NostrPk {
        &self.wallet_nostr_pk
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
        let wallet_nostr_sk = convert::to_nostr_sk(self.wallet_nostr_sk());
        let client_nostr_pk = convert::to_nostr_pk(self.client_nostr_pk());

        nip44::decrypt(&wallet_nostr_sk, &client_nostr_pk, encrypted_payload)
            .context("Failed to decrypt NIP-44 payload")
    }

    /// Encrypt a NWC response using NIP-44.
    /// See [`decrypt_nip44_request`](Self::decrypt_nip44_request) for details.
    pub(crate) fn encrypt_nip44_response(
        &self,
        rng: &mut impl Crng,
        response_json: &str,
    ) -> anyhow::Result<String> {
        let wallet_nostr_sk = convert::to_nostr_sk(self.wallet_nostr_sk());
        let client_nostr_pk = convert::to_nostr_pk(self.client_nostr_pk());

        let encrypted_string = nip44::encrypt_with_rng(
            rng,
            &wallet_nostr_sk,
            &client_nostr_pk,
            response_json,
            nip44::Version::default(),
        )
        .context("Failed to encrypt NIP-44 payload")?;

        Ok(encrypted_string)
    }

    /// Build a signed Nostr event for an NWC response.
    ///
    /// The response is tagged with the client's public key and the original
    /// request's event ID, then signed with the wallet service's secret key.
    pub(crate) fn build_response(
        &self,
        rng: &mut impl Crng,
        event_id: NostrEventId,
        content: String,
    ) -> anyhow::Result<NostrSignedEvent> {
        let event = {
            let kind = nostr::Kind::WalletConnectResponse;
            let secp = rng.gen_secp256k1_ctx();
            let client_nostr_pk = convert::to_nostr_pk(self.client_nostr_pk());
            let event_id = nostr::event::EventId::from_byte_array(event_id.0);
            let now = Instant::now();
            let keys =
                nostr::Keys::new(convert::to_nostr_sk(self.wallet_nostr_sk()));

            nostr::EventBuilder::new(kind, content)
                .tag(nostr::Tag::public_key(client_nostr_pk))
                .tag(nostr::Tag::event(event_id))
                .sign_with_ctx(&secp, rng, &now, &keys)?
        };

        let event_json = serde_json::to_string(&event)
            .context("Failed to serialize nwc response")?;
        let event_bytes = base64::engine::general_purpose::STANDARD_NO_PAD
            .encode(event_json.as_bytes())
            .into_bytes();

        Ok(NostrSignedEvent { event: event_bytes })
    }
}

#[cfg(test)]
mod test {
    use common::{aes::AesMasterKey, rng::FastRng, time::TimestampMs};

    use super::*;

    fn test_master_key() -> AesMasterKey {
        let key_bytes = [42u8; 32];
        AesMasterKey::new(&key_bytes)
    }

    /// Parse the client secret key from a connection string.
    fn parse_client_secret(connection_string: &str) -> NostrSk {
        let secret_hex = connection_string
            .split("secret=")
            .nth(1)
            .expect("connection string should contain secret=");
        let bytes =
            hex::decode(secret_hex).expect("secret should be valid hex");
        let arr: [u8; 32] =
            bytes.try_into().expect("secret should be 32 bytes");
        NostrSk::from_array(arr)
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let mut rng = FastRng::from_u64(2025121301);
        let master_key = test_master_key();

        let data = NwcClientData {
            wallet_nostr_sk: NostrSk::from_array([1u8; 32]),
            label: "Test Connection".to_string(),
        };
        let wallet_nostr_pk = NostrPk::from_array([3u8; 32]);

        let encrypted = data.encrypt(&mut rng, &master_key, &wallet_nostr_pk);
        let decrypted =
            NwcClientData::decrypt(&master_key, &wallet_nostr_pk, encrypted)
                .expect("Decryption should succeed");

        assert_eq!(data, decrypted);
    }

    #[test]
    fn test_decrypt_with_wrong_key_fails() {
        let mut rng = FastRng::from_u64(2025121302);
        let master_key = test_master_key();
        let wrong_key = AesMasterKey::new(&[99u8; 32]);

        let data = NwcClientData {
            wallet_nostr_sk: NostrSk::from_array([1u8; 32]),
            label: "Test".to_string(),
        };
        let wallet_nostr_pk = NostrPk::from_array([3u8; 32]);

        let encrypted = data.encrypt(&mut rng, &master_key, &wallet_nostr_pk);
        let result =
            NwcClientData::decrypt(&wrong_key, &wallet_nostr_pk, encrypted);

        assert!(result.is_err(), "Decryption with wrong key should fail");
    }

    #[test]
    fn test_nwc_connection_new() {
        let mut rng = FastRng::from_u64(2025121303);
        let label = "My NWC Connection".to_string();

        let (client, conn_str) =
            NwcClient::new(&mut rng, DeployEnv::Prod, label.clone());

        assert_eq!(client.data.label, label);
        assert!(conn_str.starts_with("nostr+walletconnect://"));
        // Relay URL should be percent-encoded
        assert!(conn_str.contains("relay=wss%3A%2F%2Frelay.lexe.app"));
        assert!(conn_str.contains("secret="));
    }

    #[test]
    fn test_connection_string_format() {
        let mut rng = FastRng::from_u64(2025121304);
        let (_client, conn_str) =
            NwcClient::new(&mut rng, DeployEnv::Prod, "test".to_string());

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
    fn test_db_client_roundtrip() {
        let mut rng = FastRng::from_u64(2025121305);
        let master_key = test_master_key();
        let label = "Test Label".to_string();

        let (client, _conn_str) =
            NwcClient::new(&mut rng, DeployEnv::Prod, label.clone());

        let nwc_wallet = DbNwcClient {
            fields: client.encrypt(&mut rng, &master_key),
            created_at: client.created_at,
            updated_at: client.updated_at,
        };

        let restored = NwcClient::decrypt(&master_key, nwc_wallet)
            .expect("Should restore connection");

        assert_eq!(client.data.wallet_nostr_sk, restored.data.wallet_nostr_sk);
        assert_eq!(client.client_nostr_pk, restored.client_nostr_pk);
        assert_eq!(client.data.label, restored.data.label);
        assert_eq!(client.wallet_nostr_pk, restored.wallet_nostr_pk);
    }

    #[test]
    fn test_update_label() {
        let mut rng = FastRng::from_u64(2025121306);
        let (mut connection, _) = NwcClient::new(
            &mut rng,
            DeployEnv::Prod,
            "Original Label".to_string(),
        );

        assert_eq!(connection.data.label, "Original Label");

        connection.update_label(Some("Updated Label".to_string()));
        assert_eq!(connection.data.label, "Updated Label");
    }

    #[test]
    fn test_to_nwc_client_info() {
        let mut rng = FastRng::from_u64(2025121307);
        let label = "Test Connection".to_string();
        let (client, _) =
            NwcClient::new(&mut rng, DeployEnv::Prod, label.clone());

        let info = client.to_nwc_client_info();

        assert_eq!(info.label, label);
        assert_eq!(info.wallet_nostr_pk, client.wallet_nostr_pk);
        assert_eq!(info.created_at, client.created_at);
        assert_eq!(info.updated_at, client.updated_at);
    }

    #[test]
    fn test_to_nwc_client_info_decrypt() {
        let mut rng = FastRng::from_u64(2025121308);
        let master_key = test_master_key();
        let label = "Test".to_string();

        let (client, _) =
            NwcClient::new(&mut rng, DeployEnv::Prod, label.clone());

        let db_client = DbNwcClient {
            fields: client.encrypt(&mut rng, &master_key),
            created_at: TimestampMs::now(),
            updated_at: TimestampMs::now(),
        };

        let restored =
            NwcClient::decrypt(&master_key, db_client).expect("Should restore");
        let info = restored.to_nwc_client_info();

        assert_eq!(info.label, label);
        assert_eq!(info.wallet_nostr_pk, restored.wallet_nostr_pk);
        assert_eq!(info.client_nostr_pk, restored.client_nostr_pk);
    }

    #[test]
    fn test_nip44_request_response_roundtrip() {
        let mut rng = FastRng::from_u64(2025121309);
        let (nwc_wallet, connection_string) = NwcClient::new(
            &mut rng,
            DeployEnv::Prod,
            "Test Wallet".to_string(),
        );

        // Parse client secret from connection string
        let client_nostr_sk = parse_client_secret(&connection_string);
        let client_keys =
            nostr::Keys::new(convert::to_nostr_sk(&client_nostr_sk));

        // Get wallet service public key
        let wallet_service_pk =
            convert::to_nostr_pk(&nwc_wallet.wallet_nostr_pk);

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
            .encrypt_nip44_response(&mut rng, response_payload)
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

    /// Ensure NostrPk <-> nostr::PublicKey roundtrips, so we're notified if the
    /// nostr crate ever adds validation beyond the length check.
    #[test]
    fn nostr_pk_roundtrip() {
        use proptest::prelude::*;
        proptest!(|(pk1: NostrPk)| {
            let nostr_pk = convert::to_nostr_pk(&pk1);
            let pk2 = convert::from_nostr_pk(nostr_pk);
            prop_assert_eq!(pk1, pk2);
        });
    }

    /// Ensure NostrSk <-> nostr::SecretKey roundtrips, so we're notified if the
    /// nostr crate ever adds validation beyond the length check.
    #[test]
    fn nostr_sk_roundtrip() {
        use proptest::prelude::*;
        proptest!(|(sk1: NostrSk)| {
            let nostr_sk = convert::to_nostr_sk(&sk1);
            let sk2 = convert::from_nostr_sk(nostr_sk);
            prop_assert_eq!(sk1, sk2);
        });
    }
}
