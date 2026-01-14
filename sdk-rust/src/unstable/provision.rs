//! Provision-related helpers and utilities.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

use anyhow::Context;
use common::{
    ExposeSecret, Secret,
    api::{provision::NodeProvisionRequest, version::NodeEnclave},
    constants, enclave,
    releases::Release,
    root_seed::RootSeed,
};
use lexe_api::def::AppNodeProvisionApi;
use lexe_tokio::task::LxTask;
use node_client::client::NodeClient;
use serde::Deserialize;
use tracing::{info, info_span, warn};

use crate::config::WalletEnv;

/// The contents of `public/releases.json`.
pub static RELEASES_JSON: &str = include_str!("../../../releases.json");

/// The measurements of the three latest trusted node releases.
/// This is the set of measurements that we want to provision.
/// There is no need to provision anything older than this.
pub static LATEST_TRUSTED_MEASUREMENTS: LazyLock<
    BTreeSet<enclave::Measurement>,
> = LazyLock::new(|| {
    trusted_node_releases()
        .values()
        .rev()
        .take(constants::RELEASE_WINDOW_SIZE)
        .map(|release| release.measurement)
        .collect()
});

/// Models the structure of a releases.json file.
#[derive(Deserialize)]
pub struct ReleasesJson(
    pub BTreeMap<String, BTreeMap<semver::Version, Release>>,
);

/// The set of trusted node releases (populated from releases.json).
///
/// The user trusts these releases simply by installing the open-source app or
/// SDK library which has these values hard-coded. This prevents Lexe from
/// pushing out unilateral node updates without the user's consent.
pub fn trusted_node_releases() -> BTreeMap<semver::Version, Release> {
    releases_json().0.remove("node").unwrap_or_default()
}

/// Parses [`RELEASES_JSON`] into a [`ReleasesJson`].
pub fn releases_json() -> ReleasesJson {
    serde_json::from_str(RELEASES_JSON).expect("Invalid releases.json")
}

// TODO(max): Questionable whether it's actually OK to spawn tokio tasks here.
// Does it complicate app FFI for downstream devs? Python SDK?
// Maybe we should just provision everything inline, especially once we
// implement server-side calculation of enclaves_to_provision, as
// `LexeWallet`s without persistence won't need to always try to provision
// everything returned by `current_enclaves()`.

/// Helper to provision to the given enclaves.
///
/// - `allow_gvfs_access`: See [`NodeProvisionRequest::allow_gvfs_access`].
/// - `google_auth_code`: See [`NodeProvisionRequest::google_auth_code`].
/// - `maybe_encrypted_seed`: See [`NodeProvisionRequest::encrypted_seed`].
pub(crate) async fn provision_all(
    node_client: NodeClient,
    mut enclaves_to_provision: BTreeSet<NodeEnclave>,
    root_seed: RootSeed,
    wallet_env: WalletEnv,
    google_auth_code: Option<String>,
    allow_gvfs_access: bool,
    encrypted_seed: Option<Vec<u8>>,
) -> anyhow::Result<()> {
    info!("Starting provisioning: {enclaves_to_provision:?}");

    // Make sure the latest trusted version is provisioned before we return,
    // so that when we request a node run, Lexe runs the latest version.
    let latest = match enclaves_to_provision.pop_last() {
        Some(enclave) => enclave,
        None => {
            info!("No enclaves to provision");
            return Ok(());
        }
    };

    // Provision the latest trusted enclave inline
    let root_seed_clone = clone_root_seed(&root_seed);
    provision_one(
        &node_client,
        latest,
        root_seed_clone,
        wallet_env,
        google_auth_code.clone(),
        allow_gvfs_access,
        encrypted_seed.clone(),
    )
    .await?;

    // Early return if no work left to do
    if enclaves_to_provision.is_empty() {
        return Ok(());
    }

    // Provision remaining versions asynchronously so that we don't block
    // app startup.

    // TODO(max): In the future we may want to drive the secondary
    // provisioning in function calls instead of background tasks. Some sage
    // advice from wizard Philip:
    //
    // """
    // I've found that structuring everything as function calls driven by
    // the flutter frontend to the app-rs library ends up being the
    // best approach in the end.
    //
    // - The flutter frontend owns the page and app lifecycle, best understands
    //   what calls and services are relevant, and trying to keep that in sync
    //   with Rust is cumbersome.
    // - It's much easier to mock out RPC-style fn calls for design work.
    // - Reporting errors to the user is also easy, since the error gets bubbled
    //   up to the frontend to display.
    // - If a background task has an error, there's no clear way to report to
    //   the user, so you just log and things are silently broken.
    // """
    const SPAN_NAME: &str = "(secondary-provision)";
    let task =
        LxTask::spawn_with_span(SPAN_NAME, info_span!(SPAN_NAME), async move {
            // NOTE: We provision enclaves serially because each provision
            // updates the approved versions list, and we don't currently
            // have a locking mechanism.
            for node_enclave in enclaves_to_provision {
                let root_seed_clone = clone_root_seed(&root_seed);
                let provision_result = provision_one(
                    &node_client,
                    node_enclave.clone(),
                    root_seed_clone,
                    wallet_env,
                    google_auth_code.clone(),
                    allow_gvfs_access,
                    encrypted_seed.clone(),
                )
                .await;

                if let Err(e) = provision_result {
                    warn!(
                        version = %node_enclave.version,
                        measurement = %node_enclave.measurement,
                        machine_id = %node_enclave.machine_id,
                        "Secondary provision failed: {e:#}"
                    );
                }
            }

            info!("Secondary provisioning complete");
        });

    // TODO(max): Ideally, we could await on this ephemeral task somewhere
    // for structured concurrency. But not sure if it even matters, as the
    // mobile OS will often just kill the app.
    task.detach();

    Ok(())
}

/// Provisions a single enclave.
async fn provision_one(
    node_client: &NodeClient,
    enclave: NodeEnclave,
    root_seed: RootSeed,
    wallet_env: WalletEnv,
    google_auth_code: Option<String>,
    allow_gvfs_access: bool,
    // TODO(max): We could have cheaper cloning by using Bytes here
    encrypted_seed: Option<Vec<u8>>,
) -> anyhow::Result<()> {
    let provision_req = NodeProvisionRequest {
        root_seed,
        deploy_env: wallet_env.deploy_env,
        network: wallet_env.network,
        google_auth_code,
        allow_gvfs_access,
        encrypted_seed,
    };
    node_client
        .provision(enclave.measurement, provision_req)
        .await
        .context("Failed to provision node")?;

    info!(
        version = %enclave.version,
        measurement = %enclave.measurement,
        machine_id = %enclave.machine_id,
        "Provision success:"
    );

    Ok(())
}

/// Clone a RootSeed reference into a new RootSeed instance.
// TODO(phlip9): we should get rid of this helper eventually. We could
// use something like a `Cow<'a, &RootSeed>` in `NodeProvisionRequest`. Ofc
// we still have the seed serialized in a heap-allocated json blob when we
// make the request, which is much harder for us to zeroize...
pub fn clone_root_seed(root_seed_ref: &RootSeed) -> RootSeed {
    RootSeed::new(Secret::new(*root_seed_ref.expose_secret()))
}

#[cfg(test)]
mod test {
    use super::*;

    /// Test that [`LATEST_TRUSTED_MEASUREMENTS`] doesn't panic and contains an
    /// entry. Implicitly tests [`trusted_releases`] and [`releases_json`].
    #[test]
    fn test_trusted_measurements() {
        assert!(!LATEST_TRUSTED_MEASUREMENTS.is_empty());
    }
}
