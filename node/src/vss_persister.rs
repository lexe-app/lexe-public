use anyhow::Context;
use lexe_api::rest::RestClient;
use lexe_common::{constants::timeout, env::DeployEnv, root_seed::RootSeed};
use lexe_crypto::rng::Crng;
use lexe_tls_attest_server::{self as tls_attest, NodeMode};
use lexe_vss_client::{
    KeyValue, NO_VERSION_CHECK, PutObjectRequest, VssClient,
};
use tokio::time;

use crate::backup_persister::BackupBatch;

#[cfg(test)]
mod tests;

pub(crate) struct VssPersister {
    client: VssClient,
}

impl VssPersister {
    const STORE_ID: &str = "lexe";

    /// Create a new `VssPersister` that uses a Lexe VSS server.
    pub(crate) fn new_lexe(
        rng: &mut impl Crng,
        root_seed: &RootSeed,
        backend_url: &str,
        deploy_env: DeployEnv,
    ) -> anyhow::Result<Self> {
        let tls_config =
            tls_attest::node_lexe_client_config(rng, deploy_env, NodeMode::Run)
                .context("Failed to build VSS client TLS config")?;

        let http_client = RestClient::client_builder("node")
            .use_preconfigured_tls(tls_config)
            .build()
            .context("Failed to build VSS HTTP client")?;

        let backend_url = backend_url.trim_end_matches('/');
        let base_url = format!("{backend_url}/vss");
        let client = VssClient::from_client(
            &base_url,
            http_client,
            root_seed.derive_vss_auth_key(),
        );

        Ok(Self { client })
    }

    /// Persist a [`BackupBatch`] to the remote VSS server (writes+deletes).
    pub(crate) async fn persist(
        &self,
        files: &mut BackupBatch,
    ) -> anyhow::Result<()> {
        let mut request = PutObjectRequest {
            store_id: Self::STORE_ID.to_owned(),
            global_version: None,
            transaction_items: Vec::new(),
            delete_items: Vec::new(),
        };
        for (id, data) in files.drain() {
            let (value, items) = match data {
                Some(value) => (value, &mut request.transaction_items),
                // Note small VSS API quirk where deletes still take a
                // non-optional `value: Vec<u8>`
                None => (Vec::new(), &mut request.delete_items),
            };
            items.push(KeyValue {
                key: id.to_string(),
                version: NO_VERSION_CHECK,
                value,
            });
        }
        time::timeout(
            timeout::TRANSIENT_ERROR_TOLERANCE,
            self.client.put_object(&request),
        )
        .await
        .context("VSS backup timed out")?
        .context("VSS backup failed")?;
        Ok(())
    }
}
