//! Dart interface for app settings.

use std::{str::FromStr, sync::Arc};

use anyhow::Context;
use common::api::fiat_rates::IsoCurrencyCode;
use flutter_rust_bridge::RustOpaqueNom;

use crate::{
    db::WritebackDb as WritebackDbRs,
    settings::{
        OnboardingStatus as OnboardingStatusRs, SettingsRs,
        WalletFundingState as WalletFundingStateRs,
    },
};

pub struct SettingsDb {
    pub inner: RustOpaqueNom<WritebackDbRs<SettingsRs>>,
}

/// Wallet funding state machine.
///
/// Tracks whether the user has funded their wallet and how.
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum WalletFundingState {
    /// Initial state. User has no funds and no channel.
    NonFunded,
    /// User has received on-chain funds but has no Lightning channel yet.
    OnChainDeposited,
    /// User has a pending channel open. Waiting for confirmation.
    ChannelOpening,
    /// User has a usable Lightning channel but channel reserve is not met.
    /// Can receive but can't send.
    ChannelReserveNotMet,
    /// User has a usable Lightning channel with outbound capacity.
    Funded,
}

pub struct OnboardingStatus {
    pub has_backed_up_seed_phrase: Option<bool>,
    pub has_connected_gdrive: Option<bool>,
    pub has_seen_receive_hint: Option<bool>,
    /// The current wallet funding state. Defaults to `NonFunded` if not set.
    pub wallet_funding_state: Option<WalletFundingState>,
}

pub struct Settings {
    pub locale: Option<String>,
    pub fiat_currency: Option<String>,
    pub show_split_balances: Option<bool>,
    pub onboarding_status: Option<OnboardingStatus>,
}

// --- impl SettingsDb --- //

impl SettingsDb {
    pub(crate) fn new(db: Arc<WritebackDbRs<SettingsRs>>) -> Self {
        Self {
            inner: RustOpaqueNom::from(db),
        }
    }

    /// Read all settings.
    ///
    /// flutter_rust_bridge:sync
    pub fn read(&self) -> Settings {
        Settings::from(self.inner.read())
    }

    /// Reset all settings to their defaults.
    ///
    /// flutter_rust_bridge:sync
    pub fn reset(&self) {
        self.inner.reset();
    }

    /// Update the in-memory settings by merging in any non-null fields in
    /// `update`. The settings will be persisted asynchronously, outside of this
    /// call.
    ///
    /// flutter_rust_bridge:sync
    pub fn update(&self, update: Settings) -> anyhow::Result<()> {
        let update_rs = SettingsRs::try_from(update)
            .context("Dart settings update is invalid")?;
        self.inner
            .update(update_rs)
            .context("Failed to apply settings update")?;
        Ok(())
    }
}

// --- impl Settings --- //

impl From<SettingsRs> for Settings {
    fn from(s: SettingsRs) -> Self {
        Self {
            locale: s.locale,
            fiat_currency: s.fiat_currency.map(|x| x.as_str().to_owned()),
            show_split_balances: s.show_split_balances,
            onboarding_status: s.onboarding_status.map(OnboardingStatus::from),
        }
    }
}

impl From<WalletFundingStateRs> for WalletFundingState {
    fn from(s: WalletFundingStateRs) -> Self {
        match s {
            WalletFundingStateRs::NonFunded => Self::NonFunded,
            WalletFundingStateRs::OnChainDeposited => Self::OnChainDeposited,
            WalletFundingStateRs::ChannelOpening => Self::ChannelOpening,
            WalletFundingStateRs::ChannelReserveNotMet =>
                Self::ChannelReserveNotMet,
            WalletFundingStateRs::Funded => Self::Funded,
        }
    }
}

impl From<WalletFundingState> for WalletFundingStateRs {
    fn from(s: WalletFundingState) -> Self {
        match s {
            WalletFundingState::NonFunded => Self::NonFunded,
            WalletFundingState::OnChainDeposited => Self::OnChainDeposited,
            WalletFundingState::ChannelOpening => Self::ChannelOpening,
            WalletFundingState::ChannelReserveNotMet =>
                Self::ChannelReserveNotMet,
            WalletFundingState::Funded => Self::Funded,
        }
    }
}

impl From<OnboardingStatusRs> for OnboardingStatus {
    fn from(s: OnboardingStatusRs) -> Self {
        Self {
            has_backed_up_seed_phrase: s.has_backed_up_seed_phrase,
            has_connected_gdrive: s.has_connected_gdrive,
            has_seen_receive_hint: s.has_seen_receive_hint,
            wallet_funding_state: Some(
                s.wallet_funding_state
                    .map(WalletFundingState::from)
                    .unwrap_or(WalletFundingState::NonFunded),
            ),
        }
    }
}

impl From<OnboardingStatus> for OnboardingStatusRs {
    fn from(s: OnboardingStatus) -> Self {
        // NOTE: wallet_funding_state is read-only from Dart. We always set it
        // to None here so updates from Dart are ignored.
        let _ = s.wallet_funding_state;
        Self {
            has_backed_up_seed_phrase: s.has_backed_up_seed_phrase,
            has_connected_gdrive: s.has_connected_gdrive,
            has_seen_receive_hint: s.has_seen_receive_hint,
            wallet_funding_state: None,
        }
    }
}

impl TryFrom<Settings> for SettingsRs {
    type Error = anyhow::Error;
    fn try_from(s: Settings) -> Result<Self, Self::Error> {
        Ok(Self {
            schema: SettingsRs::CURRENT_SCHEMA,
            locale: s.locale,
            fiat_currency: s
                .fiat_currency
                .as_deref()
                .map(IsoCurrencyCode::from_str)
                .transpose()?,
            show_split_balances: s.show_split_balances,
            onboarding_status: s
                .onboarding_status
                .map(OnboardingStatusRs::from),
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn onboarding_status_converts_wallet_funding_state() {
        // When Rust has Some(state), FFI should have that state.
        let rs = OnboardingStatusRs {
            has_backed_up_seed_phrase: None,
            has_connected_gdrive: Some(true),
            has_seen_receive_hint: Some(false),
            wallet_funding_state: Some(WalletFundingStateRs::ChannelOpening),
        };
        let ffi = OnboardingStatus::from(rs);

        assert_eq!(ffi.has_connected_gdrive, Some(true));
        assert_eq!(ffi.has_backed_up_seed_phrase, None);
        assert_eq!(ffi.has_seen_receive_hint, Some(false));
        assert_eq!(
            ffi.wallet_funding_state,
            Some(WalletFundingState::ChannelOpening)
        );
    }

    #[test]
    fn onboarding_status_defaults_to_non_funded() {
        // When Rust has None, FFI should default to NonFunded.
        let rs = OnboardingStatusRs {
            has_backed_up_seed_phrase: Some(true),
            has_connected_gdrive: None,
            has_seen_receive_hint: None,
            wallet_funding_state: None,
        };
        let ffi = OnboardingStatus::from(rs);

        assert_eq!(
            ffi.wallet_funding_state,
            Some(WalletFundingState::NonFunded)
        );
    }

    #[test]
    fn onboarding_status_wallet_funding_state_is_read_only() {
        // FFI -> Rust conversion should always set wallet_funding_state to
        // None, making it read-only from Dart.
        let ffi = OnboardingStatus {
            has_backed_up_seed_phrase: Some(false),
            has_connected_gdrive: Some(true),
            has_seen_receive_hint: Some(false),
            wallet_funding_state: Some(WalletFundingState::Funded),
        };
        let rs = OnboardingStatusRs::from(ffi);

        assert_eq!(rs.has_connected_gdrive, Some(true));
        assert_eq!(rs.has_backed_up_seed_phrase, Some(false));
        assert_eq!(rs.has_seen_receive_hint, Some(false));
        // wallet_funding_state is always None when coming from Dart.
        assert_eq!(rs.wallet_funding_state, None);
    }
}
