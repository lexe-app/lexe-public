use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, anyhow, ensure};
use bitcoin::secp256k1;
use lexe_api::models::nwc::{
    DbNwcClient, DbNwcClientFields, NostrPk, NwcClientInfo, nip47::NwcMethod,
};
use lexe_byte_array::ByteArray;
use lexe_common::{
    constants, env::DeployEnv, root_seed::RootSeed, secp256k1_ctx::SECP256K1,
    time::TimestampMs,
};
use lexe_crypto::{aes::AesMasterKey, hkdf, rng::Crng};
use lexe_hex::hex;
use lexe_std::array;
use nostr::nips::{nip04, nip44};
use secrecy::{ExposeSecret, Zeroize};
use serde::{Deserialize, Serialize};

const NWC_AAD_PREFIX: &[u8] = b"NWC client";

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

/// How an NWC request's content, and so our response's, is encrypted.
///
/// We only support NIP-44. NIP-04 is deprecated and insecure, but some clients
/// (e.g. Damus, `nostr-tools`) still send it, and NIP-47 says to reject it with
/// `UNSUPPORTED_ENCRYPTION`. That error is only useful if the client can read
/// it, and the client must find the request's method in its `result_type`, so
/// we decrypt the request and encrypt the error with NIP-04. Rejecting NIP-04
/// requests is the ONLY use of NIP-04, and the only reason we enable `nostr`'s
/// `nip04` feature.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum NwcEncryption {
    /// Only used to reject the request; see above.
    Nip04,
    Nip44,
}

impl NwcEncryption {
    /// The scheme a request declares in its `encryption` tag. Per NIP-47, a
    /// request without one uses NIP-04.
    pub(crate) fn of_request(event: &nostr::Event) -> anyhow::Result<Self> {
        let encryption = event
            .tags
            .find(nostr::TagKind::custom("encryption"))
            .and_then(nostr::Tag::content);
        match encryption {
            None | Some("nip04") => Ok(Self::Nip04),
            Some("nip44_v2") => Ok(Self::Nip44),
            Some(other) => Err(anyhow!("Unknown NWC encryption: {other}")),
        }
    }
}

/// Derives each NWC client's wallet service keys from the client's pk, so we
/// never generate or store wallet service secrets.
///
/// `RootSeed` -- derive("nwc master key") --> `NwcMasterKey`
///            -- expand("wallet key", client_nostr_pk) --> wallet service key
// Like `AesMasterKey`, we store the extracted PRK.
pub(crate) struct NwcMasterKey(hkdf::Prk);

impl NwcMasterKey {
    const HKDF_SALT: [u8; 32] = array::pad(*b"LEXE-REALM::NwcMasterKey");

    pub(crate) fn from_root_seed(root_seed: &RootSeed) -> Self {
        let secret = root_seed.derive_nwc_master_secret();
        Self(hkdf::Prk::extract(&Self::HKDF_SALT, secret.expose_secret()))
    }

    /// Derive the wallet service keys for the NWC client with this pk.
    pub(crate) fn derive_keys(&self, client_nostr_pk: NostrPk) -> NwcKeys {
        let mut secret = [0u8; 32];
        let info = [b"wallet key".as_slice(), client_nostr_pk.as_slice()];
        self.0.expand(&info, &mut secret);
        let wallet_sk = secp256k1::SecretKey::from_slice(&secret)
            .expect("HKDF output should be a valid secp256k1 secret key");
        secret.zeroize();

        let wallet_keys =
            nostr::Keys::new_with_ctx(&SECP256K1, wallet_sk.into());
        NwcKeys {
            client_nostr_pk,
            wallet_keys,
        }
    }
}

/// NWC client's pubkey and the node's wallet keypair derived from it.
pub(crate) struct NwcKeys {
    /// The NWC client app's Nostr pubkey.
    client_nostr_pk: NostrPk,
    /// The node's NWC wallet keypair for this client.
    wallet_keys: nostr::Keys,
}

impl NwcKeys {
    pub(crate) fn client_nostr_pk(&self) -> &NostrPk {
        &self.client_nostr_pk
    }

    pub(crate) fn wallet_nostr_pk(&self) -> &NostrPk {
        NostrPk::from_ref(self.wallet_keys.public_key.as_bytes())
    }
}

/// NWC (Nostr Wallet Connect) client connection.
///
/// Represents a connection between an external NWC client app and this wallet.
/// The wallet acts as the "wallet service" in NIP-47 terminology.
pub(crate) struct NwcClient {
    keys: NwcKeys,
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
    /// Human-readable label for this connection, set by the user.
    pub label: String,
}

impl NwcClientData {
    /// VFS-encrypts this data.
    fn encrypt(
        &self,
        rng: &mut impl Crng,
        vfs_master_key: &AesMasterKey,
        keys: &NwcKeys,
    ) -> Vec<u8> {
        let aad = &Self::aad(keys);
        vfs_master_key.encrypt(rng, aad, None, &|out: &mut Vec<u8>| {
            serde_json::to_writer(out, self)
                .expect("JSON serialization was not implemented correctly");
        })
    }

    /// VFS-decrypts this data.
    fn decrypt(
        vfs_master_key: &AesMasterKey,
        keys: &NwcKeys,
        data: Vec<u8>,
    ) -> anyhow::Result<Self> {
        let aad = &Self::aad(keys);
        let wallet_nostr_pk = keys.wallet_nostr_pk();
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
    fn aad(keys: &NwcKeys) -> [&[u8]; 3] {
        [
            NWC_AAD_PREFIX,
            keys.client_nostr_pk().as_slice(),
            keys.wallet_nostr_pk().as_slice(),
        ]
    }
}

impl NwcClient {
    /// Generate a new NWC client and its connection string.
    ///
    /// Generates a random client keypair and derives the node's wallet keypair
    /// from it. Returns the client and the connection string, which contains
    /// the client secret key needed by the NWC app.
    pub(crate) fn new(
        rng: &mut impl Crng,
        nwc_master_key: &NwcMasterKey,
        deploy_env: DeployEnv,
        label: String,
        now: TimestampMs,
    ) -> (Self, String) {
        let client_nostr_keys = nostr::Keys::generate_with_rng(rng);
        let client_nostr_pk =
            NostrPk::from_array(client_nostr_keys.public_key().to_bytes());
        let keys = nwc_master_key.derive_keys(client_nostr_pk);

        let connection_string = Self::build_connection_string(
            deploy_env,
            keys.wallet_nostr_pk(),
            client_nostr_keys.secret_key(),
        );

        let info_event =
            Self::build_info_event_json(rng, &keys.wallet_keys, now);

        let client = Self {
            keys,
            data: NwcClientData { label },
            info_event,
            created_at: now,
            updated_at: now,
        };

        (client, connection_string)
    }

    /// Decrypt an [`NwcClient`] from an encrypted [`DbNwcClient`] record,
    /// rejecting records whose pks don't match `keys`.
    pub(crate) fn decrypt(
        vfs_master_key: &AesMasterKey,
        keys: NwcKeys,
        nwc_client: DbNwcClient,
    ) -> anyhow::Result<Self> {
        let DbNwcClient {
            fields,
            created_at,
            updated_at,
        } = nwc_client;
        ensure!(
            &fields.client_nostr_pk == keys.client_nostr_pk(),
            "NWC client pk mismatch"
        );
        ensure!(
            &fields.wallet_nostr_pk == keys.wallet_nostr_pk(),
            "NWC wallet pk isn't derived from the client pk"
        );

        let data =
            NwcClientData::decrypt(vfs_master_key, &keys, fields.ciphertext)?;
        Ok(Self {
            keys,
            data,
            info_event: fields.info_event,
            created_at,
            updated_at,
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
            client_nostr_pk: *self.keys.client_nostr_pk(),
            wallet_nostr_pk: *self.keys.wallet_nostr_pk(),
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
        let ciphertext = self.data.encrypt(rng, vfs_master_key, &self.keys);
        DbNwcClientFields {
            client_nostr_pk: *self.keys.client_nostr_pk(),
            wallet_nostr_pk: *self.keys.wallet_nostr_pk(),
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

        self.info_event =
            Self::build_info_event_json(rng, &self.keys.wallet_keys, now);

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
        let expected =
            Self::info_event_builder().build(self.keys.wallet_keys.public_key);
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

    /// Decrypt an NWC request's content.
    ///
    /// [NIP-44] uses ECDH to derive a shared secret from both parties'
    /// keypairs, then authenticated encryption (ChaCha20 + HMAC-SHA256).
    /// The HMAC provides integrity: decryption fails if the payload was
    /// encrypted with different keys, protecting against spoofed client pks.
    ///
    /// [NIP-44]: https://github.com/nostr-protocol/nips/blob/master/44.md
    pub(crate) fn decrypt_request(
        &self,
        encryption: NwcEncryption,
        content: &str,
    ) -> anyhow::Result<String> {
        let wallet_nostr_sk = self.keys.wallet_keys.secret_key();
        let client_nostr_pk = convert::to_nostr_pk(self.keys.client_nostr_pk());

        match encryption {
            NwcEncryption::Nip04 =>
                nip04::decrypt(wallet_nostr_sk, &client_nostr_pk, content)
                    .context("Failed to decrypt NIP-04 payload"),
            NwcEncryption::Nip44 =>
                nip44::decrypt(wallet_nostr_sk, &client_nostr_pk, content)
                    .context("Failed to decrypt NIP-44 payload"),
        }
    }

    /// Encrypt an NWC response's content.
    /// See [`decrypt_request`](Self::decrypt_request) for details.
    pub(crate) fn encrypt_response(
        &self,
        rng: &mut impl Crng,
        encryption: NwcEncryption,
        response_json: &str,
    ) -> anyhow::Result<String> {
        let wallet_nostr_sk = self.keys.wallet_keys.secret_key();
        let client_nostr_pk = convert::to_nostr_pk(self.keys.client_nostr_pk());

        match encryption {
            NwcEncryption::Nip04 => nip04::encrypt_with_rng(
                rng,
                wallet_nostr_sk,
                &client_nostr_pk,
                response_json,
            )
            .context("Failed to encrypt NIP-04 payload"),
            NwcEncryption::Nip44 => nip44::encrypt_with_rng(
                rng,
                wallet_nostr_sk,
                &client_nostr_pk,
                response_json,
                nip44::Version::default(),
            )
            .context("Failed to encrypt NIP-44 payload"),
        }
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
        let client_nostr_pk = convert::to_nostr_pk(self.keys.client_nostr_pk());
        let now = Instant::now();

        let event = nostr::EventBuilder::new(kind, content)
            .tag(nostr::Tag::public_key(client_nostr_pk))
            .tag(nostr::Tag::event(event_id))
            .sign_with_ctx(&SECP256K1, rng, &now, &self.keys.wallet_keys)?;
        Ok(event)
    }
}

/// Verify a signed NWC request and derive its client's keys, rejecting
/// requests not addressed to the client's derived wallet service key.
pub(crate) fn verify_request_event(
    event: &nostr::Event,
    now: TimestampMs,
    nwc_master_key: &NwcMasterKey,
) -> anyhow::Result<NwcKeys> {
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

    // Check the request is addressed to the client's wallet key
    let client_nostr_pk = NostrPk::from_array(event.pubkey.to_bytes());
    let keys = nwc_master_key.derive_keys(client_nostr_pk);
    ensure!(
        &keys.wallet_keys.public_key == wallet_nostr_pk,
        "NWC request isn't addressed to the client's wallet pk"
    );

    Ok(keys)
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

    fn test_vfs_master_key() -> AesMasterKey {
        let key_bytes = [42u8; 32];
        AesMasterKey::new(&key_bytes)
    }

    fn test_nwc_master_key() -> NwcMasterKey {
        NwcMasterKey::from_root_seed(&RootSeed::from_u64(20261014))
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
    fn test_verify_request_event() {
        let nwc_master_key = test_nwc_master_key();
        let mut rng = FastRng::from_u64(2026100801);
        let now = TimestampMs::from_secs_u32(2026100801);
        let supplier = FixedTimestampMsSupplier(now);
        let (client, connection_string) = NwcClient::new(
            &mut rng,
            &nwc_master_key,
            DeployEnv::Dev,
            "test".to_owned(),
            now,
        );

        let keys = parse_client_keys(&connection_string);
        let stranger = nostr::Keys::generate_with_rng(&mut rng);
        let wallet_pk = convert::to_nostr_pk(client.keys.wallet_nostr_pk());
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
            verify_request_event(event, now, &nwc_master_key).map(|_| ())
        };

        let verified_keys =
            verify_request_event(&request, now, &nwc_master_key).unwrap();
        assert_eq!(
            verified_keys.client_nostr_pk(),
            client.keys.client_nostr_pk()
        );
        assert_eq!(
            verified_keys.wallet_nostr_pk(),
            client.keys.wallet_nostr_pk()
        );

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
        let nwc_master_key = test_nwc_master_key();
        let mut rng = FastRng::from_u64(2026100802);
        let now = TimestampMs::from_secs_u32(2026100802);
        let (client, connection_string) = NwcClient::new(
            &mut rng,
            &nwc_master_key,
            DeployEnv::Dev,
            "test".to_owned(),
            now,
        );
        let keys = parse_client_keys(&connection_string);
        let wallet_pk = convert::to_nostr_pk(client.keys.wallet_nostr_pk());
        let now = 1_000_000;
        let skew = constants::NWC_REQUEST_MAX_CLOCK_SKEW.as_secs();

        let do_verify = |event: &nostr::Event, now| {
            verify_request_event(event, now, &nwc_master_key).map(|_| ())
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
        let master_key = test_vfs_master_key();

        let data = NwcClientData {
            label: "Test Connection".to_string(),
        };
        let keys =
            test_nwc_master_key().derive_keys(NostrPk::from_array([2u8; 32]));

        let encrypted = data.encrypt(&mut rng, &master_key, &keys);
        let decrypted = NwcClientData::decrypt(&master_key, &keys, encrypted)
            .expect("Decryption should succeed");

        assert_eq!(data, decrypted);
    }

    #[test]
    fn test_decrypt_with_wrong_key_or_pks_fails() {
        let mut rng = FastRng::from_u64(2025121302);
        let master_key = test_vfs_master_key();
        let wrong_key = AesMasterKey::new(&[99u8; 32]);

        let data = NwcClientData {
            label: "Test".to_string(),
        };
        let nwc_master_key = test_nwc_master_key();
        let client_pk = NostrPk::from_array([2u8; 32]);
        let other_pk = NostrPk::from_array([4u8; 32]);
        let keys = nwc_master_key.derive_keys(client_pk);
        let other = nwc_master_key.derive_keys(other_pk);

        let encrypted = data.encrypt(&mut rng, &master_key, &keys);
        let decrypt =
            |key, keys| NwcClientData::decrypt(key, keys, encrypted.clone());

        decrypt(&master_key, &keys).unwrap();
        decrypt(&wrong_key, &keys).unwrap_err();
        // The ciphertext is bound to its client and wallet pks
        let other_client = NwcKeys {
            client_nostr_pk: other_pk,
            wallet_keys: keys.wallet_keys.clone(),
        };
        let other_wallet = NwcKeys {
            client_nostr_pk: client_pk,
            wallet_keys: other.wallet_keys.clone(),
        };
        decrypt(&master_key, &other_client).unwrap_err();
        decrypt(&master_key, &other_wallet).unwrap_err();
    }

    #[test]
    fn test_nwc_connection_new() {
        let mut rng = FastRng::from_u64(2025121303);
        let now = TimestampMs::from_secs_u32(2025121303);
        let label = "My NWC Connection".to_string();

        let (client, conn_str) = NwcClient::new(
            &mut rng,
            &test_nwc_master_key(),
            DeployEnv::Prod,
            label.clone(),
            now,
        );

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
        let (_client, conn_str) = NwcClient::new(
            &mut rng,
            &test_nwc_master_key(),
            DeployEnv::Prod,
            "test".to_string(),
            now,
        );

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
        let nwc_master_key = test_nwc_master_key();
        let mut rng = FastRng::from_u64(2025121305);
        let now = TimestampMs::from_secs_u32(2025121305);
        let master_key = test_vfs_master_key();
        let label = "Test Label".to_string();

        let (client, _conn_str) = NwcClient::new(
            &mut rng,
            &nwc_master_key,
            DeployEnv::Prod,
            label.clone(),
            now,
        );

        let client_pk = *client.keys.client_nostr_pk();
        let db_client = DbNwcClient {
            fields: client.encrypt(&mut rng, &master_key),
            created_at: client.created_at,
            updated_at: client.updated_at,
        };

        let keys = nwc_master_key.derive_keys(client_pk);
        let restored = NwcClient::decrypt(&master_key, keys, db_client)
            .expect("Should restore connection");

        assert_eq!(
            client.keys.client_nostr_pk(),
            restored.keys.client_nostr_pk()
        );
        assert_eq!(
            client.keys.wallet_nostr_pk(),
            restored.keys.wallet_nostr_pk()
        );
        assert_eq!(client.data.label, restored.data.label);
    }

    /// Wallet service keys must never change, or NWC connections break.
    #[test]
    fn nwc_master_key_snapshot() {
        let seed = RootSeed::from_u64(20240506);
        let nwc_master_key = NwcMasterKey::from_root_seed(&seed);
        let keys = nwc_master_key.derive_keys(NostrPk::from_array([0x42; 32]));
        assert_eq!(
            keys.wallet_keys.secret_key().to_secret_hex(),
            "5e4089593d33c1dbb8ac1627211b24496ba6c85dcfab538ccf2703bc275846f9",
        );
    }

    /// The backend can't attach a stored client's record to another client pk.
    #[test]
    fn test_decrypt_rejects_forged_records() {
        let mut rng = FastRng::from_u64(2026101401);
        let now = TimestampMs::from_secs_u32(2026101401);
        let master_key = test_vfs_master_key();
        let nwc_master_key = test_nwc_master_key();
        let (client, _) = NwcClient::new(
            &mut rng,
            &nwc_master_key,
            DeployEnv::Prod,
            "test".to_owned(),
            now,
        );
        let fields = client.encrypt(&mut rng, &master_key);
        let attacker_pk = NostrPk::from_array([0x69; 32]);
        let attacker_wallet_pk =
            *nwc_master_key.derive_keys(attacker_pk).wallet_nostr_pk();
        let decrypt = |fields| {
            let keys = nwc_master_key.derive_keys(attacker_pk);
            let db_client = DbNwcClient {
                fields,
                created_at: now,
                updated_at: now,
            };
            NwcClient::decrypt(&master_key, keys, db_client).map(|_| ())
        };

        // Keys for a different client than the record's
        decrypt(fields.clone()).unwrap_err();

        // The victim's record under the attacker's client pk, whose derived
        // wallet pk differs
        let mut forged = fields;
        forged.client_nostr_pk = attacker_pk;
        decrypt(forged.clone()).unwrap_err();

        // Even with the attacker's derived wallet pk, the ciphertext is still
        // bound to the victim's pks
        forged.wallet_nostr_pk = attacker_wallet_pk;
        let err = decrypt(forged).unwrap_err();
        assert!(format!("{err:#}").contains("Failed to decrypt"), "{err:#}");
    }

    #[test]
    fn test_update_label() {
        let mut rng = FastRng::from_u64(2025121306);
        let now = TimestampMs::from_secs_u32(2025121306);
        let (mut connection, _) = NwcClient::new(
            &mut rng,
            &test_nwc_master_key(),
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
        let (mut client, _) = NwcClient::new(
            &mut rng,
            &test_nwc_master_key(),
            DeployEnv::Prod,
            "test".to_owned(),
            now,
        );

        // A fresh client has an info event, no refresh required.
        assert!(!client.refresh_info_event(&mut rng, now).unwrap());

        let event: nostr::Event =
            serde_json::from_slice(&client.info_event).unwrap();
        event.verify().unwrap();
        assert_eq!(event.pubkey, client.keys.wallet_keys.public_key);
        assert_eq!(event.content, "get_info make_invoice");
        let encryption = event
            .tags
            .find(nostr::TagKind::custom("encryption"))
            .expect("Missing encryption tag");
        assert_eq!(encryption.content(), Some("nip44_v2"));

        // Refreshing again is a no-op...
        assert!(!client.refresh_info_event(&mut rng, now).unwrap());

        // ...until the stored event advertises something else.
        let keys = client.keys.wallet_keys.clone();
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
        let (client, _) = NwcClient::new(
            &mut rng,
            &test_nwc_master_key(),
            DeployEnv::Prod,
            label.clone(),
            now,
        );

        let info = client.to_nwc_client_info();

        assert_eq!(info.label, label);
        assert_eq!(&info.wallet_nostr_pk, client.keys.wallet_nostr_pk());
        assert_eq!(info.created_at, client.created_at);
        assert_eq!(info.updated_at, client.updated_at);
    }

    #[test]
    fn test_to_nwc_client_info_decrypt() {
        let nwc_master_key = test_nwc_master_key();
        let mut rng = FastRng::from_u64(2025121308);
        let master_key = test_vfs_master_key();
        let label = "Test".to_string();
        let now = TimestampMs::from_secs_u32(2025121308);

        let (client, _) = NwcClient::new(
            &mut rng,
            &nwc_master_key,
            DeployEnv::Prod,
            label.clone(),
            now,
        );

        let keys = nwc_master_key.derive_keys(*client.keys.client_nostr_pk());
        let db_client = DbNwcClient {
            fields: client.encrypt(&mut rng, &master_key),
            created_at: TimestampMs::now(),
            updated_at: TimestampMs::now(),
        };

        let restored = NwcClient::decrypt(&master_key, keys, db_client)
            .expect("Should restore");
        let info = restored.to_nwc_client_info();

        assert_eq!(info.label, label);
        assert_eq!(&info.wallet_nostr_pk, restored.keys.wallet_nostr_pk());
        assert_eq!(&info.client_nostr_pk, restored.keys.client_nostr_pk());
    }

    #[test]
    fn test_request_encryption() {
        let mut rng = FastRng::from_u64(2026101301);
        let keys = nostr::Keys::generate_with_rng(&mut rng);
        let mut request = |encryption: Option<&str>| {
            let tags = encryption.map(|encryption| {
                let kind = nostr::TagKind::custom("encryption");
                nostr::Tag::custom(kind, [encryption])
            });
            let event = nostr::EventBuilder::new(
                nostr::Kind::WalletConnectRequest,
                "ciphertext",
            )
            .tags(tags)
            .sign_with_ctx(&SECP256K1, &mut rng, &Instant::now(), &keys)
            .unwrap();
            NwcEncryption::of_request(&event)
        };

        assert_eq!(request(None).unwrap(), NwcEncryption::Nip04);
        assert_eq!(request(Some("nip04")).unwrap(), NwcEncryption::Nip04);
        assert_eq!(request(Some("nip44_v2")).unwrap(), NwcEncryption::Nip44);
        request(Some("nip44_v3")).unwrap_err();
    }

    #[test]
    fn test_request_response_encryption_roundtrip() {
        let mut rng = FastRng::from_u64(2025121309);
        let now = TimestampMs::from_secs_u32(2025121309);
        let (nwc_wallet, connection_string) = NwcClient::new(
            &mut rng,
            &test_nwc_master_key(),
            DeployEnv::Prod,
            "Test Wallet".to_string(),
            now,
        );

        let client_keys = parse_client_keys(&connection_string);
        let client_sk = client_keys.secret_key();
        let wallet_pk = convert::to_nostr_pk(nwc_wallet.keys.wallet_nostr_pk());

        let request_payload = r#"{"method":"get_info","params":{}}"#;
        let response_payload =
            r#"{"result":{"alias":"test","network":"bitcoin"},"error":null}"#;

        for encryption in [NwcEncryption::Nip04, NwcEncryption::Nip44] {
            // Client encrypts request, wallet service decrypts it
            let encrypted_request = match encryption {
                NwcEncryption::Nip04 =>
                    nip04::encrypt(client_sk, &wallet_pk, request_payload)
                        .unwrap(),
                NwcEncryption::Nip44 => nip44::encrypt(
                    client_sk,
                    &wallet_pk,
                    request_payload,
                    nip44::Version::default(),
                )
                .unwrap(),
            };
            let decrypted_request = nwc_wallet
                .decrypt_request(encryption, &encrypted_request)
                .unwrap();
            assert_eq!(decrypted_request, request_payload);

            // Wallet service encrypts response, client decrypts it
            let encrypted_response = nwc_wallet
                .encrypt_response(&mut rng, encryption, response_payload)
                .unwrap();
            let decrypted_response = match encryption {
                NwcEncryption::Nip04 =>
                    nip04::decrypt(client_sk, &wallet_pk, &encrypted_response)
                        .unwrap(),
                NwcEncryption::Nip44 =>
                    nip44::decrypt(client_sk, &wallet_pk, &encrypted_response)
                        .unwrap(),
            };
            assert_eq!(decrypted_response, response_payload);
        }
    }
}
