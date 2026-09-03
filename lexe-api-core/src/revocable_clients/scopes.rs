//! # Authorization model for revocable clients connecting over mTLS.
//!
//! ## Model
//!
//! Four types:
//!
//! - [`Permission`]: An atom; gives access to a specific API endpoint.
//! - [`PermissionSet`]: A set of permissions. This is what [`Scope`]s and
//!   [`ClientPermissions`] expand to, and what an endpoint check runs against.
//!   This is computed per request and never persisted.
//! - [`Scope`]: a user-facing alias for a bundle of permissions, e.g. `read` /
//!   `receive` / `spend` / `full`. Expands into a [`PermissionSet`]. The
//!   meaning of each scope is intended to evolve over time; for example, if a
//!   new spend-only endpoint is added, all existing credentials with the
//!   `spend` scope automatically get access to it.
//! - [`ClientPermissions`]: Defines the permissions granted to a client;
//!   includes a set of (possibly-overlapping) [`Scope`]s as well as any number
//!   of explicit [`Permission`]s. At request time, this is resolved to a
//!   [`PermissionSet`] and checked against the endpoint's atom.
//!
//! ## "Fail-closed" design
//!
//! - Adding a *permission* is fail-closed in that a new [`Permission`] joins no
//!   scope until we deliberately add it to one. The `full_covers_all` test
//!   forces us to decide which scope a permission should fall under.
//!
//! - Adding an *endpoint* attempts to be fail-closed through the use of a
//!   structural idiom that requires every endpoint to be explicitly marked as
//!   `scoped::` or `unscoped::`, where `scoped::` endpoints must name the
//!   [`Permission`] that they require.
//!
//! - When a node deserializes a persisted [`RevocableClient`], any [`Scope`]s
//!   or [`Permission`]s that the node doesn't recognize are dropped. This
//!   allows us to remove scopes and permissions over time. Likewise, if a
//!   client passes unknown variants inside a [`CreateRevocableClientRequest`],
//!   the node will reject the request. This ensures that an old client relying
//!   on a scope that no longer exists is notified early, instead of failing
//!   later when they try to use it.
//!
//! [`Permission`]: scopes::Permission
//! [`PermissionSet`]: scopes::PermissionSet
//! [`Scope`]: scopes::Scope
//! [`ClientPermissions`]: scopes::ClientPermissions
//! [`RevocableClient`]: RevocableClient
//! [`CreateRevocableClientRequest`]: models::CreateRevocableClientRequest

use std::{collections::BTreeSet, fmt, str::FromStr};

#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use serde_with::DeserializeFromStr;
use strum::VariantArray;
use tracing::warn;

/// The authorization a credential holds: any number of [`Scope`] aliases plus
/// any number of explicit [`Permission`]s. Persisted in a `RevocableClient`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct ClientPermissions {
    pub scopes: BTreeSet<Scope>,
    pub permissions: BTreeSet<Permission>,
}

impl ClientPermissions {
    /// Construct a [`ClientPermissions`] that holds only a single scope alias.
    pub fn from_single_scope(scope: Scope) -> Self {
        Self {
            scopes: BTreeSet::from([scope]),
            permissions: BTreeSet::new(),
        }
    }

    /// Whether this grant is empty: no scopes and no explicit permissions.
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty() && self.permissions.is_empty()
    }

    /// Resolve to the full set of granted permissions.
    pub fn resolve(&self) -> PermissionSet {
        let mut set = PermissionSet::EMPTY;
        for scope in &self.scopes {
            set = set.union(scope.permissions());
        }
        for &permission in &self.permissions {
            set = set.with(permission);
        }
        set
    }

    /// Whether this grant contains `permission`.
    pub fn contains(&self, permission: Permission) -> bool {
        self.resolve().contains(permission)
    }

    /// Whether this grant covers everything `other` does.
    ///
    /// Used for scope attenuation: `parent.covers(&child)` must be true.
    pub fn covers(&self, other: &Self) -> bool {
        self.resolve().covers(other.resolve())
    }

    /// An alternative [`Deserialize`] impl which drops unknown scopes and
    /// permissions (with a `warn!` log) instead of erroring.
    ///
    /// Used by [`RevocableClient`] to deserialize persisted grants, which may
    /// contain variants which are no longer valid.
    ///
    /// Deserializers that wish to reject unknown variants (e.g. when creating a
    /// revocable client) should continue to use the derived [`Deserialize`]
    /// impl to ensure unknown variants are rejected early.
    ///
    /// [`RevocableClient`]: super::RevocableClient
    pub fn deserialize_drop_unknown<'de, D>(
        deserializer: D,
    ) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        /// The raw persisted form: lists of string ids.
        #[derive(Deserialize)]
        struct RawClientPermissions {
            scopes: Vec<Box<str>>,
            permissions: Vec<Box<str>>,
        }

        /// Parse the known ids, dropping (and warning on) unknown ones.
        fn parse_known<T: FromStr, C: FromIterator<T>>(
            ids: &[Box<str>],
            kind: &str,
        ) -> C {
            ids.iter()
                .filter_map(|id| match id.parse::<T>() {
                    Ok(known) => Some(known),
                    Err(_) => {
                        warn!("Dropping unknown {kind} id: {id}");
                        None
                    }
                })
                .collect()
        }

        let raw = RawClientPermissions::deserialize(deserializer)?;
        Ok(Self {
            scopes: parse_known(&raw.scopes, "scope"),
            permissions: parse_known(&raw.permissions, "permission"),
        })
    }
}

/// A user-facing alias that expands to a [`PermissionSet`].
///
/// Each scope is persisted by name ([`Scope::as_str`]) rather than as the
/// permissions it expands to, so the meaning each scope can evolve to cover new
/// endpoints.
///
/// A [`ClientPermissions`] may hold multiple, often overlapping, scopes.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(DeserializeFromStr, VariantArray)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum Scope {
    /// Read basic info: user identity, node version, balance, and channels.
    ReadInfo,
    /// Read all payments.
    ReadPayments,
    /// Read everything: `read_info` + `read_payments`, plus on-chain
    /// descriptors, Lexe SDK clients, and other miscellaneous data.
    /// Cannot read any secrets that would allow spending funds.
    //
    // LSP: Also read peers, network graph, scorer, utxos.
    //
    // TODO(max): Also add "NWC clients" when NWC is supported; propagate
    // updated doc to SDKs and app.
    Read,
    /// Create invoices, offers, and addresses to receive to, resync the node,
    /// and cancel payments. Cannot determine if invoices or offers were
    /// actually paid.
    Receive,
    /// Open and close channels.
    //
    // LSP: Also connect and disconnect peers.
    ManageChannels,
    /// Pay invoices, offers, and on-chain addresses; update payment notes.
    Spend,
    /// (LSP only) LSP operations: `read` + `manage_channels`;
    /// resync and update channel configs.
    LspOps,
    /// Full admin access: every permission granted by other scopes, plus
    /// signing with the identity pubkey, managing and revoking SDK clients,
    /// reading encrypted files, and updating the user's HBA.
    //
    // TODO(max): Change to "managing and revoking SDK and NWC clients" when
    // NWC is supported; propagate updated doc to SDKs and app.
    Full,
}

impl Scope {
    /// The stable string identifier used when persisted.
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::ReadInfo => "read_info",
            Scope::ReadPayments => "read_payments",
            Scope::Read => "read",
            Scope::Receive => "receive",
            Scope::ManageChannels => "manage_channels",
            Scope::Spend => "spend",
            Scope::LspOps => "lsp_ops",
            Scope::Full => "full",
        }
    }

    /// The set of permissions this alias currently grants.
    pub fn permissions(self) -> PermissionSet {
        use Permission::*;
        match self {
            // Excludes `DebugInfo`, whose on-chain descriptors reveal the
            // full on-chain payment history.
            Scope::ReadInfo => PermissionSet::from_slice(&[
                NodeInfo,
                ListChannels,
                GetHumanBitcoinAddress,
            ]),
            Scope::ReadPayments => PermissionSet::from_slice(&[
                GetPaymentsByIndexes,
                GetNewPayments,
                GetUpdatedPayments,
                GetPaymentById,
                ListBroadcastedTxs,
            ]),
            Scope::Read => Scope::ReadInfo
                .permissions()
                .union(Scope::ReadPayments.permissions())
                .union(PermissionSet::from_slice(&[
                    BackupInfo,
                    DebugInfo,
                    ListRevocableClients,
                    ListNwcClients,
                    ListPeers,
                    GetNetworkGraph,
                    GetProbScorer,
                    GetUtxos,
                ])),
            Scope::Receive => PermissionSet::from_slice(&[
                GetNextUnusedAddress,
                CreateInvoice,
                CreateOffer,
                Resync,
                CancelPayment,
            ]),
            Scope::ManageChannels => PermissionSet::from_slice(&[
                OpenChannel,
                OpenChannelPreflight,
                CloseChannel,
                CloseChannelPreflight,
                ConnectPeer,
                DisconnectPeer,
            ]),
            Scope::Spend => PermissionSet::from_slice(&[
                PayInvoice,
                PayInvoicePreflight,
                PayOffer,
                PayOfferPreflight,
                PayOnchain,
                PayOnchainPreflight,
                CreatePayerProof,
                UpdatePersonalNote,
            ]),
            Scope::LspOps => Scope::Read
                .permissions()
                .union(Scope::ManageChannels.permissions())
                .union(PermissionSet::from_slice(&[
                    Resync,
                    UpdateChannelConfig,
                ])),
            Scope::Full => Scope::Read
                .permissions()
                .union(Scope::Receive.permissions())
                .union(Scope::ManageChannels.permissions())
                .union(Scope::Spend.permissions())
                // NOTE: We intentionally do NOT use `Scope::VARIANTS` here, and
                // instead define "full" as the union of other scopes with
                // full-specific permissions, so that we are forced to make a
                // reasoned decision about where to put a permission when the
                // `full_covers_all` test detects an uncovered permission.
                .union(PermissionSet::from_slice(&[
                    UpdateChannelConfig,
                    SignMessage,
                    VerifyMessage,
                    CreateRevocableClient,
                    UpdateRevocableClient,
                    CreateNwcClient,
                    UpdateNwcClient,
                    DeleteNwcClient,
                    SetupGdrive,
                    UpdateHumanBitcoinAddress,
                    GetFile,
                ])),
        }
    }

    /// The other scopes whose permissions this scope fully covers, i.e. this
    /// scope's "children".
    ///
    /// UIs use this to lock a child's checkbox while its parent is selected.
    /// Persisting a child alongside its parent is redundant but harmless.
    pub fn children(self) -> &'static [Scope] {
        match self {
            Scope::Read => &[Scope::ReadInfo, Scope::ReadPayments],
            Scope::LspOps => &[
                Scope::ReadInfo,
                Scope::ReadPayments,
                Scope::Read,
                Scope::ManageChannels,
            ],
            Scope::Full => &[
                Scope::ReadInfo,
                Scope::ReadPayments,
                Scope::Read,
                Scope::Receive,
                Scope::ManageChannels,
                Scope::Spend,
                Scope::LspOps,
            ],
            Scope::ReadInfo
            | Scope::ReadPayments
            | Scope::Receive
            | Scope::ManageChannels
            | Scope::Spend => &[],
        }
    }

    /// Scopes recommended to be granted alongside this one.
    ///
    /// For example, if someone can `Receive`, it's best if they can also
    /// `ReadPayments` as well, to see whether their invoices were paid.
    ///
    /// UIs use this to select companion scopes for the "Read" / "Receive" /
    /// "Spend" / "Admin" presets, but users are free to opt-out.
    pub fn recommended(self) -> &'static [Scope] {
        match self {
            Scope::Receive => &[Scope::ReadInfo, Scope::ReadPayments],
            Scope::Spend =>
                &[Scope::ReadInfo, Scope::ReadPayments, Scope::ManageChannels],
            Scope::ReadInfo
            | Scope::ReadPayments
            | Scope::Read
            | Scope::ManageChannels
            | Scope::LspOps
            | Scope::Full => &[],
        }
    }
}

impl Serialize for Scope {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.as_str().serialize(serializer)
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Scope {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|scope| scope.as_str() == s)
            .ok_or_else(|| format!("Unknown scope: {s}"))
    }
}

/// A resolved set of [`Permission`]s, as a bitset.
///
/// `Copy`, cheap to check, and purely in memory.
//
// Since this is purely in-memory, we can always change the memory layout or
// expand to u128 / bitvec etc if needed.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PermissionSet(u64);

// Enforce the 64-permission ceiling of `PermissionSet`'s `u64`.
const _: () = assert!(
    Permission::VARIANTS.len() <= 64,
    "Too many permissions for PermissionSet's u64 — widen the representation",
);

impl PermissionSet {
    /// The empty set.
    pub const EMPTY: Self = Self(0);

    /// Every defined permission (the union of all their bits).
    pub const ALL: Self = Self::from_slice(Permission::VARIANTS);

    /// Build a set from a list of permissions.
    pub const fn from_slice(permissions: &[Permission]) -> Self {
        let mut set = Self::EMPTY;
        // Index loop instead of an iterator because this is a `const fn`.
        let mut i = 0;
        while i < permissions.len() {
            let permission = permissions[i];
            set = set.with(permission);
            i += 1;
        }
        set
    }

    /// Whether this set contains `p`.
    #[inline]
    pub const fn contains(self, p: Permission) -> bool {
        self.0 & p.bit() != 0
    }

    /// This set with `p` added.
    #[inline]
    pub const fn with(self, p: Permission) -> Self {
        Self(self.0 | p.bit())
    }

    /// The union of two sets.
    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether this set covers every permission in `other`.
    #[inline]
    pub const fn covers(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// The set of permissions in `self` but not in `other` (`self - other`).
    #[inline]
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Whether this set contains no permissions.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Iterate over the permissions in this set.
    pub fn iter(self) -> impl Iterator<Item = Permission> {
        Permission::VARIANTS
            .iter()
            .copied()
            .filter(move |p| self.contains(*p))
    }
}

impl fmt::Display for PermissionSet {
    /// The set's permissions as their string ids, comma-separated.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for permission in self.iter() {
            if !first {
                f.write_str(", ")?;
            }
            f.write_str(permission.as_str())?;
            first = false;
        }
        Ok(())
    }
}

/// An atom; the most granular capability that gives access to a specific API
/// endpoint. If permissions are requested specifically, they are persisted
/// using [`Permission::as_str`], which must remain stable.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(DeserializeFromStr, VariantArray)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum Permission {
    // --- read_info --- //
    NodeInfo,
    ListChannels,
    GetHumanBitcoinAddress,

    // --- read_payments --- //
    GetPaymentsByIndexes,
    GetNewPayments,
    GetUpdatedPayments,
    GetPaymentById,
    ListBroadcastedTxs,

    // --- read --- //
    BackupInfo,
    DebugInfo,
    ListRevocableClients,
    ListNwcClients,
    ListPeers,
    GetNetworkGraph,
    GetProbScorer,
    GetUtxos,

    // --- receive --- //
    GetNextUnusedAddress,
    CreateInvoice,
    CreateOffer,
    Resync,
    CancelPayment,

    // --- manage_channels --- //
    OpenChannel,
    OpenChannelPreflight,
    CloseChannel,
    CloseChannelPreflight,
    ConnectPeer,
    DisconnectPeer,

    // --- spend --- //
    PayInvoice,
    PayInvoicePreflight,
    PayOffer,
    PayOfferPreflight,
    PayOnchain,
    PayOnchainPreflight,
    CreatePayerProof,
    UpdatePersonalNote,

    // --- lsp_ops --- //
    UpdateChannelConfig,

    // --- full only --- //
    SignMessage,
    VerifyMessage,
    CreateRevocableClient,
    UpdateRevocableClient,
    CreateNwcClient,
    UpdateNwcClient,
    DeleteNwcClient,
    SetupGdrive,
    UpdateHumanBitcoinAddress,
    GetFile,
}

impl Permission {
    /// The stable string identifier used for persistence.
    pub fn as_str(self) -> &'static str {
        match self {
            Permission::NodeInfo => "node_info",
            Permission::ListChannels => "list_channels",
            Permission::GetHumanBitcoinAddress => "get_human_bitcoin_address",
            Permission::GetPaymentsByIndexes => "get_payments_by_indexes",
            Permission::GetNewPayments => "get_new_payments",
            Permission::GetUpdatedPayments => "get_updated_payments",
            Permission::GetPaymentById => "get_payment_by_id",
            Permission::ListBroadcastedTxs => "list_broadcasted_txs",
            Permission::BackupInfo => "backup_info",
            Permission::DebugInfo => "debug_info",
            Permission::ListRevocableClients => "list_revocable_clients",
            Permission::ListNwcClients => "list_nwc_clients",
            Permission::ListPeers => "list_peers",
            Permission::GetNetworkGraph => "get_network_graph",
            Permission::GetProbScorer => "get_prob_scorer",
            Permission::GetUtxos => "get_utxos",
            Permission::GetNextUnusedAddress => "get_next_unused_address",
            Permission::CreateInvoice => "create_invoice",
            Permission::CreateOffer => "create_offer",
            Permission::Resync => "resync",
            Permission::CancelPayment => "cancel_payment",
            Permission::OpenChannel => "open_channel",
            Permission::OpenChannelPreflight => "open_channel_preflight",
            Permission::CloseChannel => "close_channel",
            Permission::CloseChannelPreflight => "close_channel_preflight",
            Permission::ConnectPeer => "connect_peer",
            Permission::DisconnectPeer => "disconnect_peer",
            Permission::PayInvoice => "pay_invoice",
            Permission::PayInvoicePreflight => "pay_invoice_preflight",
            Permission::PayOffer => "pay_offer",
            Permission::PayOfferPreflight => "pay_offer_preflight",
            Permission::PayOnchain => "pay_onchain",
            Permission::PayOnchainPreflight => "pay_onchain_preflight",
            Permission::CreatePayerProof => "create_payer_proof",
            Permission::UpdatePersonalNote => "update_personal_note",
            Permission::UpdateChannelConfig => "update_channel_config",
            Permission::SignMessage => "sign_message",
            Permission::VerifyMessage => "verify_message",
            Permission::CreateRevocableClient => "create_revocable_client",
            Permission::UpdateRevocableClient => "update_revocable_client",
            Permission::CreateNwcClient => "create_nwc_client",
            Permission::UpdateNwcClient => "update_nwc_client",
            Permission::DeleteNwcClient => "delete_nwc_client",
            Permission::SetupGdrive => "setup_gdrive",
            Permission::UpdateHumanBitcoinAddress =>
                "update_human_bitcoin_address",
            Permission::GetFile => "get_file",
        }
    }

    /// This permission's bit in a [`PermissionSet`].
    #[inline]
    const fn bit(self) -> u64 {
        1u64 << (self as u64)
    }
}

impl Serialize for Permission {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.as_str().serialize(serializer)
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Permission {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|permission| permission.as_str() == s)
            .ok_or_else(|| format!("Unknown permission: {s}"))
    }
}

#[cfg(test)]
mod test {
    use lexe_common::test_utils::roundtrip;

    use super::*;

    #[test]
    fn client_permissions_json_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<ClientPermissions>();
    }

    /// The derived impl rejects unknown scope/permission ids (requests must
    /// be strict).
    #[test]
    fn unknown_ids_rejected() {
        let ok = r#"{"scopes": ["read"], "permissions": ["pay_invoice"]}"#;
        assert!(serde_json::from_str::<ClientPermissions>(ok).is_ok());

        let bad_scope = r#"{"scopes": ["bogus"], "permissions": []}"#;
        assert!(serde_json::from_str::<ClientPermissions>(bad_scope).is_err());

        let bad_perm = r#"{"scopes": [], "permissions": ["bogus"]}"#;
        assert!(serde_json::from_str::<ClientPermissions>(bad_perm).is_err());
    }

    /// [`ClientPermissions::deserialize_drop_unknown`] drops unknown ids
    /// instead of erroring.
    #[test]
    fn unknown_ids_dropped() {
        #[derive(Deserialize)]
        struct Lenient(
            #[serde(
                deserialize_with = "ClientPermissions::deserialize_drop_unknown"
            )]
            ClientPermissions,
        );

        let json = r#"{
            "scopes": ["read", "bogus_scope"],
            "permissions": ["pay_invoice", "bogus_permission"]
        }"#;
        let permissions = serde_json::from_str::<Lenient>(json).unwrap().0;
        assert_eq!(permissions.scopes, BTreeSet::from([Scope::Read]));
        assert_eq!(
            permissions.permissions,
            BTreeSet::from([Permission::PayInvoice])
        );

        // Leniency covers unknown ids only; structural errors still reject.
        let missing_key = r#"{"scopes": ["read"]}"#;
        assert!(serde_json::from_str::<Lenient>(missing_key).is_err());
    }

    /// `full` must cover every permission, including newly-added ones.
    #[test]
    fn full_covers_all() {
        assert_eq!(Scope::Full.permissions(), PermissionSet::ALL);
    }

    /// Exactly these permissions are reachable only via `full`.
    #[test]
    fn full_only_permissions() {
        let non_full = Scope::VARIANTS
            .iter()
            .filter(|scope| !matches!(scope, Scope::Full))
            .fold(PermissionSet::EMPTY, |acc, scope| {
                acc.union(scope.permissions())
            });
        let expected = PermissionSet::from_slice(&[
            Permission::SignMessage,
            Permission::VerifyMessage,
            Permission::CreateRevocableClient,
            Permission::UpdateRevocableClient,
            Permission::CreateNwcClient,
            Permission::UpdateNwcClient,
            Permission::DeleteNwcClient,
            Permission::SetupGdrive,
            Permission::UpdateHumanBitcoinAddress,
            Permission::GetFile,
        ]);
        assert_eq!(PermissionSet::ALL.difference(non_full), expected);
    }

    /// [`Scope::children`] must be exactly the set of other scopes each
    /// scope covers — the scope hierarchy is part of the API contract.
    #[test]
    fn children_exactly_covered() {
        for &scope in Scope::VARIANTS {
            for &other in Scope::VARIANTS {
                if scope == other {
                    continue;
                }
                let covered = scope.permissions().covers(other.permissions());
                let is_child = scope.children().contains(&other);
                assert_eq!(
                    is_child, covered,
                    "{scope}.children() disagrees with covers({other})"
                );
            }
        }
    }

    /// Recommended scopes complement their scope; a scope never recommends
    /// itself or a scope it already covers.
    #[test]
    fn recommended_not_covered() {
        for &scope in Scope::VARIANTS {
            for &rec in scope.recommended() {
                assert!(
                    !scope.permissions().covers(rec.permissions()),
                    "{scope}.recommended() contains covered scope {rec}"
                );
            }
        }
    }

    /// Every permission's string id roundtrips and is unique.
    #[test]
    fn permission_str_roundtrip() {
        let mut seen = BTreeSet::new();
        for &p in Permission::VARIANTS {
            assert_eq!(p.as_str().parse::<Permission>(), Ok(p));
            assert!(seen.insert(p.as_str()), "duplicate id {}", p.as_str());
        }
    }

    /// Snapshot of the persisted `Permission` and `Scope` ids.
    ///
    /// - Removing a variant is allowed, but must be deliberate: persisted
    ///   credentials silently lose the grant on read, and requests naming the
    ///   old id are rejected. Never reuse a removed id for a different
    ///   capability — old credentials may still carry it.
    /// - Renaming a variant must not drop the old id: serialize the new id, but
    ///   keep parsing the old one (an alias arm in `FromStr`, like
    ///   `#[serde(alias)]`) until all persisted grants have migrated, possibly
    ///   forever.
    /// - Reordering variants is safe (only the string ids are persisted) but
    ///   must be mirrored here; appending a new variant just extends the
    ///   snapshot.
    #[test]
    fn json_backwards_compat() {
        let permissions_ser = r#"["node_info","list_channels","get_human_bitcoin_address","get_payments_by_indexes","get_new_payments","get_updated_payments","get_payment_by_id","list_broadcasted_txs","backup_info","debug_info","list_revocable_clients","list_nwc_clients","list_peers","get_network_graph","get_prob_scorer","get_utxos","get_next_unused_address","create_invoice","create_offer","resync","cancel_payment","open_channel","open_channel_preflight","close_channel","close_channel_preflight","connect_peer","disconnect_peer","pay_invoice","pay_invoice_preflight","pay_offer","pay_offer_preflight","pay_onchain","pay_onchain_preflight","create_payer_proof","update_personal_note","update_channel_config","sign_message","verify_message","create_revocable_client","update_revocable_client","create_nwc_client","update_nwc_client","delete_nwc_client","setup_gdrive","update_human_bitcoin_address","get_file"]"#;
        roundtrip::json_unit_enum_backwards_compat::<Permission>(
            permissions_ser,
        );

        let scopes_ser = r#"["read_info","read_payments","read","receive","manage_channels","spend","lsp_ops","full"]"#;
        roundtrip::json_unit_enum_backwards_compat::<Scope>(scopes_ser);
    }
}
