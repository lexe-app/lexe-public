use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, ensure};
use lexe_api::models::nwc::{
    DbNwcClient, DbNwcClientFields, NostrPk, NostrSk, NwcClientInfo,
    nip47::NwcMethod,
};
use lexe_byte_array::ByteArray;
use lexe_common::{
    constants, env::DeployEnv, secp256k1_ctx::SECP256K1, time::TimestampMs,
};
use lexe_crypto::{aes::AesMasterKey, rng::Crng};
use lexe_hex::hex;
use nostr::nips::nip44;
use serde::{Deserialize, Serialize};
use verify::VerifiedNwcRequestEvent;

const NWC_AAD_PREFIX: &[u8] = b"NWC";

/// The NWC methods this wallet supports, advertised in the `13194` info event
/// and in `get_info` responses.
pub(crate) const SUPPORTED_METHODS: &[NwcMethod] =
    &[NwcMethod::GetInfo, NwcMethod::MakeInvoice];

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

    pub fn to_nostr_pk(pk: &NostrPk) -> nostr::PublicKey {
        nostr::PublicKey::from_byte_array(pk.0)
    }

    pub fn to_nostr_sk(sk: &NostrSk) -> nostr::SecretKey {
        // Our `NostrSk`s all come from `nostr::Keys`, so they're valid.
        nostr::SecretKey::from_slice(sk.as_ref())
            .expect("NostrSk should be a valid secret key")
    }

    #[cfg(test)]
    pub fn to_nostr_ts(ts: TimestampMs) -> nostr::Timestamp {
        nostr::Timestamp::from_secs(ts.to_secs())
    }

    pub fn try_from_nostr_ts(
        nostr_ts: nostr::Timestamp,
    ) -> Result<TimestampMs, lexe_common::time::Error> {
        TimestampMs::from_secs(nostr_ts.as_secs())
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
    /// The wallet-signed NIP-47 `13194` info event stored alongside this
    /// client. See [`Self::refresh_info_event`].
    info_event: Vec<u8>,
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
        client_nostr_pk: &NostrPk,
        wallet_nostr_pk: &NostrPk,
    ) -> Vec<u8> {
        let aad = &Self::aad(client_nostr_pk, wallet_nostr_pk);
        vfs_master_key.encrypt(rng, aad, None, &|out: &mut Vec<u8>| {
            serde_json::to_writer(out, self)
                .expect("JSON serialization was not implemented correctly");
        })
    }

    /// VFS-decrypts this data.
    fn decrypt(
        vfs_master_key: &AesMasterKey,
        client_nostr_pk: &NostrPk,
        wallet_nostr_pk: &NostrPk,
        data: Vec<u8>,
    ) -> anyhow::Result<Self> {
        let aad = &Self::aad(client_nostr_pk, wallet_nostr_pk);
        let value = vfs_master_key
            .decrypt(aad, data)
            .context("Failed to decrypt NWC client data")
            .with_context(|| wallet_nostr_pk.to_string())?;
        serde_json::from_slice(&value)
            .context("Failed to deserialize NWC client data")
            .with_context(|| wallet_nostr_pk.to_string())
    }

    /// Binds the ciphertext to both pubkeys, so whoever stores it (the
    /// backend) can't attach it to another client's key, which would let
    /// that client use this connection.
    fn aad<'a>(
        client_nostr_pk: &'a NostrPk,
        wallet_nostr_pk: &'a NostrPk,
    ) -> [&'a [u8]; 3] {
        [
            NWC_AAD_PREFIX,
            client_nostr_pk.as_slice(),
            wallet_nostr_pk.as_slice(),
        ]
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
        now: TimestampMs,
    ) -> (Self, String) {
        let wallet_nostr_keys = nostr::Keys::generate_with_rng(rng);
        let client_nostr_keys = nostr::Keys::generate_with_rng(rng);

        let wallet_nostr_sk =
            NostrSk::from_array(wallet_nostr_keys.secret_key().secret_bytes());
        let wallet_nostr_pk =
            NostrPk::from_array(wallet_nostr_keys.public_key().to_bytes());
        let client_nostr_pk =
            NostrPk::from_array(client_nostr_keys.public_key().to_bytes());

        let connection_string = Self::build_connection_string(
            deploy_env,
            &wallet_nostr_pk,
            client_nostr_keys.secret_key(),
        );

        let info_event =
            Self::build_info_event_json(rng, &wallet_nostr_keys, now);

        let client = Self {
            client_nostr_pk,
            wallet_nostr_pk,
            data: NwcClientData {
                wallet_nostr_sk,
                label,
            },
            info_event,
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
            &nwc_client.fields.client_nostr_pk,
            &nwc_client.fields.wallet_nostr_pk,
            nwc_client.fields.ciphertext,
        )?;
        Ok(Self {
            client_nostr_pk: nwc_client.fields.client_nostr_pk,
            wallet_nostr_pk: nwc_client.fields.wallet_nostr_pk,
            data,
            info_event: nwc_client.fields.info_event,
            created_at: nwc_client.created_at,
            updated_at: nwc_client.updated_at,
        })
    }

    fn build_connection_string(
        deploy_env: DeployEnv,
        wallet_nostr_pk: &NostrPk,
        client_nostr_sk: &nostr::SecretKey,
    ) -> String {
        let relay = relay_url(deploy_env);
        let wallet_nostr_pk = hex::display(wallet_nostr_pk.as_array());
        let client_nostr_sk = client_nostr_sk.to_secret_hex();

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

    /// Encrypt the client data for storage in the DB.
    pub(crate) fn encrypt(
        &self,
        rng: &mut impl Crng,
        vfs_master_key: &AesMasterKey,
    ) -> DbNwcClientFields {
        let ciphertext = self.data.encrypt(
            rng,
            vfs_master_key,
            &self.client_nostr_pk,
            &self.wallet_nostr_pk,
        );
        DbNwcClientFields {
            client_nostr_pk: self.client_nostr_pk,
            wallet_nostr_pk: self.wallet_nostr_pk,
            ciphertext,
            info_event: self.info_event.clone(),
        }
    }

    /// Re-sign the `13194` info event if it no longer matches what we would
    /// sign today, e.g. because [`SUPPORTED_METHODS`] changed. Returns whether
    /// it changed, so callers can skip a needless persist.
    pub(crate) fn refresh_info_event(
        &mut self,
        rng: &mut impl Crng,
        now: TimestampMs,
    ) -> anyhow::Result<bool> {
        if !self.info_event_is_stale() {
            return Ok(false);
        }

        let keys =
            nostr::Keys::new(convert::to_nostr_sk(&self.data.wallet_nostr_sk));
        self.info_event = Self::build_info_event_json(rng, &keys, now);

        Ok(true)
    }

    /// Whether the stored info event is undecodable, isn't validly signed by
    /// this client's wallet key, or differs in content or tags from what
    /// [`Self::info_event_builder`] produces now.
    fn info_event_is_stale(&self) -> bool {
        let Ok(stored) =
            serde_json::from_slice::<nostr::Event>(&self.info_event)
        else {
            return true;
        };
        let wallet_nostr_pk = convert::to_nostr_pk(&self.wallet_nostr_pk);
        let expected = Self::info_event_builder().build(wallet_nostr_pk);
        stored.pubkey != expected.pubkey
            || stored.content != expected.content
            || stored.tags != expected.tags
            || stored.verify_with_ctx(&SECP256K1).is_err()
    }

    /// Build and sign the `13194` NWC info event.
    fn build_info_event_json(
        rng: &mut impl Crng,
        wallet_nostr_keypair: &nostr::Keys,
        now: TimestampMs,
    ) -> Vec<u8> {
        let now = FixedTimestampMsSupplier(now);
        let event = Self::info_event_builder()
            .sign_with_ctx(&SECP256K1, rng, &now, wallet_nostr_keypair)
            .expect("Signing an info event should never fail");
        serde_json::to_vec(&event).expect("Failed to serialize info event")
    }

    /// The unsigned `13194` info event. Its content lists the supported
    /// methods, and its `encryption` tag advertises NIP-44; clients assume
    /// NIP-04 if that tag is absent.
    fn info_event_builder() -> nostr::EventBuilder {
        let content = SUPPORTED_METHODS
            .iter()
            .map(NwcMethod::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        let encryption = nostr::Tag::custom(
            nostr::TagKind::custom("encryption"),
            ["nip44_v2"],
        );
        nostr::EventBuilder::new(nostr::Kind::WalletConnectInfo, content)
            .tag(encryption)
    }

    pub(crate) fn update_label(&mut self, label: Option<String>) {
        if let Some(label) = label {
            self.data.label = label;
        }
    }

    /// Verify a signed NWC request addressed to this connection.
    pub(crate) fn verify_nwc_request_event(
        &self,
        verified_event: VerifiedNwcRequestEvent<'_>,
    ) -> anyhow::Result<()> {
        // Check client pk
        ensure!(
            &self.client_nostr_pk == verified_event.client_nostr_pk(),
            "Client nostr pk mismatch"
        );

        // Check wallet pk
        ensure!(
            &self.wallet_nostr_pk == verified_event.wallet_nostr_pk(),
            "Wallet nostr pk mismatch"
        );

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
        encrypted_payload: &str,
    ) -> anyhow::Result<String> {
        let wallet_nostr_sk = convert::to_nostr_sk(&self.data.wallet_nostr_sk);
        let client_nostr_pk = convert::to_nostr_pk(&self.client_nostr_pk);

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
        let wallet_nostr_sk = convert::to_nostr_sk(&self.data.wallet_nostr_sk);
        let client_nostr_pk = convert::to_nostr_pk(&self.client_nostr_pk);

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
        event_id: nostr::EventId,
        content: String,
    ) -> anyhow::Result<nostr::Event> {
        let kind = nostr::Kind::WalletConnectResponse;
        let client_nostr_pk = convert::to_nostr_pk(&self.client_nostr_pk);
        let now = Instant::now();
        let keys =
            nostr::Keys::new(convert::to_nostr_sk(&self.data.wallet_nostr_sk));

        let event = nostr::EventBuilder::new(kind, content)
            .tag(nostr::Tag::public_key(client_nostr_pk))
            .tag(nostr::Tag::event(event_id))
            .sign_with_ctx(&SECP256K1, rng, &now, &keys)?;
        Ok(event)
    }
}

pub mod verify {
    use super::*;

    pub struct VerifiedNwcRequestEvent<'a> {
        client_nostr_pk: &'a NostrPk,
        wallet_nostr_pk: &'a NostrPk,
    }

    impl<'a> VerifiedNwcRequestEvent<'a> {
        pub fn client_nostr_pk(&self) -> &'a NostrPk {
            self.client_nostr_pk
        }
        pub fn wallet_nostr_pk(&self) -> &'a NostrPk {
            self.wallet_nostr_pk
        }
    }

    pub fn nwc_request_event(
        event: &nostr::Event,
        now: TimestampMs,
    ) -> anyhow::Result<VerifiedNwcRequestEvent<'_>> {
        // Only accept NWC requests
        ensure!(
            event.kind == nostr::Kind::WalletConnectRequest,
            "Expected NWC request (kind 23194)"
        );

        // Prevent replays of old requests
        let created_at = convert::try_from_nostr_ts(event.created_at)?;
        ensure!(
            now.has_within(created_at, constants::NWC_REQUEST_MAX_CLOCK_SKEW),
            "NWC request is too old or too far in the future",
        );

        // Reject expired requests
        if let Some(expiration) = event.tags.expiration() {
            let expiration = convert::try_from_nostr_ts(*expiration)?;
            ensure!(expiration >= now, "NWC request has expired");
        }

        // NWC requests must have a target wallet pk
        let wallet_nostr_pk = event
            .tags
            .public_keys()
            .next()
            .context("Missing wallet pk")?;

        // Verify event integrity (id + client signature)
        event
            .verify_with_ctx(&SECP256K1)
            .context("Invalid NWC event id or signature")?;

        let client_nostr_pk = NostrPk::from_ref(event.pubkey.as_bytes());
        let wallet_nostr_pk = NostrPk::from_ref(wallet_nostr_pk.as_bytes());

        Ok(VerifiedNwcRequestEvent {
            client_nostr_pk,
            wallet_nostr_pk,
        })
    }
}

#[derive(Copy, Clone)]
struct FixedTimestampMsSupplier(pub TimestampMs);

impl nostr::types::time::TimeSupplier for FixedTimestampMsSupplier {
    type Now = Self;
    type StartingPoint = SystemTime;

    fn now(&self) -> Self::StartingPoint {
        self.0.to_system_time()
    }

    fn instant_now(&self) -> Self::Now {
        *self
    }

    fn starting_point(&self) -> Self::StartingPoint {
        SystemTime::UNIX_EPOCH
    }

    fn duration_since_starting_point(
        &self,
        now: Self::StartingPoint,
    ) -> Duration {
        now.duration_since(self.starting_point())
            .unwrap_or_default()
    }

    fn elapsed_instant_since(
        &self,
        now: Self::Now,
        since: Self::Now,
    ) -> Duration {
        now.0.saturating_duration_since(since.0)
    }

    fn elapsed_since(
        &self,
        now: Self::StartingPoint,
        since: Self::StartingPoint,
    ) -> Duration {
        now.duration_since(since).unwrap_or_default()
    }
}

impl std::ops::Sub for FixedTimestampMsSupplier {
    type Output = Duration;

    fn sub(self, rhs: Self) -> Self::Output {
        self.0.saturating_duration_since(rhs.0)
    }
}

#[cfg(test)]
mod test {
    use lexe_common::time::TimestampMs;
    use lexe_crypto::{aes::AesMasterKey, rng::FastRng};

    use super::*;

    fn test_master_key() -> AesMasterKey {
        let key_bytes = [42u8; 32];
        AesMasterKey::new(&key_bytes)
    }

    /// Parse the client keys from a connection string.
    fn parse_client_keys(connection_string: &str) -> nostr::Keys {
        let secret_hex = connection_string
            .split("secret=")
            .nth(1)
            .expect("connection string should contain secret=");
        nostr::Keys::parse(secret_hex).expect("secret should be a secret key")
    }

    #[test]
    fn test_verify_nwc_request_event() {
        let mut rng = FastRng::from_u64(2026100801);
        let now = TimestampMs::from_secs_u32(2026100801);
        let supplier = FixedTimestampMsSupplier(now);
        let (client, connection_string) =
            NwcClient::new(&mut rng, DeployEnv::Dev, "test".to_owned(), now);

        let keys = parse_client_keys(&connection_string);
        let stranger = nostr::Keys::generate_with_rng(&mut rng);
        let wallet_pk = convert::to_nostr_pk(&client.wallet_nostr_pk);
        let now = TimestampMs::from_secs_u32(1_000_000);
        let now_nostr = convert::to_nostr_ts(now);

        let builder = nostr::EventBuilder::new(
            nostr::Kind::WalletConnectRequest,
            "ciphertext",
        )
        .custom_created_at(now_nostr);
        let mut sign = |builder: nostr::EventBuilder| {
            builder
                .sign_with_ctx(&SECP256K1, &mut rng, &supplier, &keys)
                .unwrap()
        };
        let request =
            sign(builder.clone().tag(nostr::Tag::public_key(wallet_pk)));

        let do_verify = |event, now| {
            let verified = verify::nwc_request_event(event, now)?;
            client.verify_nwc_request_event(verified)
        };

        let verified_event = verify::nwc_request_event(&request, now).unwrap();
        client.verify_nwc_request_event(verified_event).unwrap();

        // Each field is bound to the signature, including the timestamp.
        let mut tampered = request.clone();
        tampered.content.push('x');
        do_verify(&tampered, now).unwrap_err();

        let mut tampered = request.clone();
        tampered.created_at = nostr::Timestamp::from_secs(1_000_001);
        do_verify(&tampered, now).unwrap_err();

        let mut tampered = request.clone();
        tampered.id = nostr::EventId::all_zeros();
        do_verify(&tampered, now).unwrap_err();

        let mut tampered = request.clone();
        tampered.sig = sign(builder.clone()).sig;
        do_verify(&tampered, now).unwrap_err();

        let wrong_kind = sign(
            nostr::EventBuilder::text_note("ciphertext")
                .custom_created_at(now_nostr)
                .tag(nostr::Tag::public_key(wallet_pk)),
        );
        do_verify(&wrong_kind, now).unwrap_err();

        let missing_wallet = sign(builder.clone());
        do_verify(&missing_wallet, now).unwrap_err();

        let wrong_wallet = sign(
            builder
                .clone()
                .tag(nostr::Tag::public_key(stranger.public_key())),
        );
        do_verify(&wrong_wallet, now).unwrap_err();

        let expired =
            sign(builder.clone().tag(nostr::Tag::public_key(wallet_pk)).tag(
                nostr::Tag::expiration(nostr::Timestamp::from_secs(999_999)),
            ));
        do_verify(&expired, now).unwrap_err();

        let unexpired =
            sign(builder.clone().tag(nostr::Tag::public_key(wallet_pk)).tag(
                nostr::Tag::expiration(nostr::Timestamp::from_secs(1_000_001)),
            ));
        do_verify(&unexpired, now).unwrap();

        let wrong_client = builder
            .tag(nostr::Tag::public_key(wallet_pk))
            .sign_with_ctx(&SECP256K1, &mut rng, &supplier, &stranger)
            .unwrap();
        do_verify(&wrong_client, now).unwrap_err();
    }

    #[test]
    fn test_verify_request_timestamp() {
        let mut rng = FastRng::from_u64(2026100802);
        let now = TimestampMs::from_secs_u32(2026100802);
        let (client, connection_string) =
            NwcClient::new(&mut rng, DeployEnv::Dev, "test".to_owned(), now);
        let keys = parse_client_keys(&connection_string);
        let wallet_pk = convert::to_nostr_pk(&client.wallet_nostr_pk);
        let now = 1_000_000;
        let skew = constants::NWC_REQUEST_MAX_CLOCK_SKEW.as_secs();

        let do_verify = |event: &nostr::Event, now| {
            let verified = verify::nwc_request_event(event, now)?;
            client.verify_nwc_request_event(verified)
        };

        for (created_at, accepted) in [
            (0, false),
            (now - skew - 1, false),
            (now - skew, true),
            (now, true),
            (now + skew, true),
            (now + skew + 1, false),
            (u64::MAX, false),
        ] {
            let now = TimestampMs::from_secs(now).unwrap();
            let created_at = nostr::Timestamp::from_secs(created_at);
            let expiration = nostr::Timestamp::from_secs(now.to_u64() + 3600);

            // An explicit future expiration must not extend the clock window.
            let event = nostr::EventBuilder::new(
                nostr::Kind::WalletConnectRequest,
                "ciphertext",
            )
            .custom_created_at(created_at)
            .tag(nostr::Tag::public_key(wallet_pk))
            .tag(nostr::Tag::expiration(expiration))
            .sign_with_ctx(
                &SECP256K1,
                &mut rng,
                &Instant::now(), /* unused b/c we pass in custom_created_at */
                &keys,
            )
            .unwrap();

            let result = do_verify(&event, now).is_ok();
            assert_eq!(result, accepted);
        }
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let mut rng = FastRng::from_u64(2025121301);
        let master_key = test_master_key();

        let data = NwcClientData {
            wallet_nostr_sk: NostrSk::from_array([1u8; 32]),
            label: "Test Connection".to_string(),
        };
        let client_nostr_pk = NostrPk::from_array([2u8; 32]);
        let wallet_nostr_pk = NostrPk::from_array([3u8; 32]);

        let encrypted = data.encrypt(
            &mut rng,
            &master_key,
            &client_nostr_pk,
            &wallet_nostr_pk,
        );
        let decrypted = NwcClientData::decrypt(
            &master_key,
            &client_nostr_pk,
            &wallet_nostr_pk,
            encrypted,
        )
        .expect("Decryption should succeed");

        assert_eq!(data, decrypted);
    }

    #[test]
    fn test_decrypt_with_wrong_key_or_pks_fails() {
        let mut rng = FastRng::from_u64(2025121302);
        let master_key = test_master_key();
        let wrong_key = AesMasterKey::new(&[99u8; 32]);

        let data = NwcClientData {
            wallet_nostr_sk: NostrSk::from_array([1u8; 32]),
            label: "Test".to_string(),
        };
        let client_pk = NostrPk::from_array([2u8; 32]);
        let wallet_pk = NostrPk::from_array([3u8; 32]);
        let other_pk = NostrPk::from_array([4u8; 32]);

        let encrypted =
            data.encrypt(&mut rng, &master_key, &client_pk, &wallet_pk);
        let decrypt = |key, client_pk, wallet_pk| {
            NwcClientData::decrypt(key, client_pk, wallet_pk, encrypted.clone())
        };

        decrypt(&master_key, &client_pk, &wallet_pk).unwrap();
        decrypt(&wrong_key, &client_pk, &wallet_pk).unwrap_err();
        // The ciphertext is bound to its client and wallet pks
        decrypt(&master_key, &other_pk, &wallet_pk).unwrap_err();
        decrypt(&master_key, &client_pk, &other_pk).unwrap_err();
    }

    #[test]
    fn test_nwc_connection_new() {
        let mut rng = FastRng::from_u64(2025121303);
        let now = TimestampMs::from_secs_u32(2025121303);
        let label = "My NWC Connection".to_string();

        let (client, conn_str) =
            NwcClient::new(&mut rng, DeployEnv::Prod, label.clone(), now);

        assert_eq!(client.data.label, label);
        assert!(conn_str.starts_with("nostr+walletconnect://"));
        // Relay URL should be percent-encoded
        assert!(conn_str.contains("relay=wss%3A%2F%2Frelay.lexe.app"));
        assert!(conn_str.contains("secret="));
    }

    #[test]
    fn test_connection_string_format() {
        let mut rng = FastRng::from_u64(2025121304);
        let now = TimestampMs::from_secs_u32(2025121304);
        let (_client, conn_str) =
            NwcClient::new(&mut rng, DeployEnv::Prod, "test".to_string(), now);

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
        let now = TimestampMs::from_secs_u32(2025121305);
        let master_key = test_master_key();
        let label = "Test Label".to_string();

        let (client, _conn_str) =
            NwcClient::new(&mut rng, DeployEnv::Prod, label.clone(), now);

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
        let now = TimestampMs::from_secs_u32(2025121306);
        let (mut connection, _) = NwcClient::new(
            &mut rng,
            DeployEnv::Prod,
            "Original Label".to_string(),
            now,
        );

        assert_eq!(connection.data.label, "Original Label");

        connection.update_label(Some("Updated Label".to_string()));
        assert_eq!(connection.data.label, "Updated Label");
    }

    #[test]
    fn test_refresh_info_event() {
        let mut rng = FastRng::from_u64(2025121310);
        let now = TimestampMs::from_secs_u32(2025121310);
        let (mut client, _) =
            NwcClient::new(&mut rng, DeployEnv::Prod, "test".to_owned(), now);

        // A fresh client has an info event, no refresh required.
        assert!(!client.refresh_info_event(&mut rng, now).unwrap());

        let event: nostr::Event =
            serde_json::from_slice(&client.info_event).unwrap();
        event.verify().unwrap();
        assert_eq!(event.pubkey, convert::to_nostr_pk(&client.wallet_nostr_pk));
        assert_eq!(event.content, "get_info make_invoice");
        let encryption = event
            .tags
            .find(nostr::TagKind::custom("encryption"))
            .expect("Missing encryption tag");
        assert_eq!(encryption.content(), Some("nip44_v2"));

        // Refreshing again is a no-op...
        assert!(!client.refresh_info_event(&mut rng, now).unwrap());

        // ...until the stored event advertises something else.
        let keys = nostr::Keys::new(convert::to_nostr_sk(
            &client.data.wallet_nostr_sk,
        ));
        let stale = nostr::EventBuilder::new(
            nostr::Kind::WalletConnectInfo,
            "get_info",
        )
        .sign_with_ctx(&SECP256K1, &mut rng, &Instant::now(), &keys)
        .unwrap();
        client.info_event = serde_json::to_vec(&stale).unwrap();
        assert!(client.refresh_info_event(&mut rng, now).unwrap());

        // Undecodable stored events are also stale.
        client.info_event = b"garbage".to_vec();
        assert!(client.refresh_info_event(&mut rng, now).unwrap());

        // As are current events signed by another key or with a bad signature.
        let fresh =
            serde_json::from_slice::<nostr::Event>(&client.info_event).unwrap();
        let stranger = nostr::Keys::generate_with_rng(&mut rng);
        let foreign = NwcClient::info_event_builder()
            .sign_with_ctx(&SECP256K1, &mut rng, &Instant::now(), &stranger)
            .unwrap();
        client.info_event = serde_json::to_vec(&foreign).unwrap();
        assert!(client.refresh_info_event(&mut rng, now).unwrap());

        let mut forged = fresh;
        forged.sig = foreign.sig;
        client.info_event = serde_json::to_vec(&forged).unwrap();
        assert!(client.refresh_info_event(&mut rng, now).unwrap());
    }

    #[test]
    fn test_to_nwc_client_info() {
        let mut rng = FastRng::from_u64(2025121307);
        let label = "Test Connection".to_string();
        let now = TimestampMs::from_secs_u32(2025121307);
        let (client, _) =
            NwcClient::new(&mut rng, DeployEnv::Prod, label.clone(), now);

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
        let now = TimestampMs::from_secs_u32(2025121308);

        let (client, _) =
            NwcClient::new(&mut rng, DeployEnv::Prod, label.clone(), now);

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
        let now = TimestampMs::from_secs_u32(2025121309);
        let (nwc_wallet, connection_string) = NwcClient::new(
            &mut rng,
            DeployEnv::Prod,
            "Test Wallet".to_string(),
            now,
        );

        // Parse client keys from connection string
        let client_keys = parse_client_keys(&connection_string);

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
            .decrypt_nip44_request(&encrypted_request)
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
}
