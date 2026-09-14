//! Opinionated Versioned Storage Service (VSS) client.
//!
//! Used by usernodes to backup their channel state to third-party VSS storage
//! providers.

use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};

use vss_client_ng::{
    client::VssClient as UpstreamClient,
    headers::{VssHeaderProvider, sigs_auth::SigsAuthProvider},
    util::retry::{
        ExponentialBackoffRetryPolicy, FilteredRetryPolicy,
        JitteredRetryPolicy, MaxAttemptsRetryPolicy, RetryPolicy,
    },
};
// Reexport vss-client-ng request/response types
pub use vss_client_ng::{
    error::VssError,
    prost,
    types::{
        DeleteObjectRequest, DeleteObjectResponse, GetObjectRequest,
        GetObjectResponse, KeyValue, ListKeyVersionsRequest,
        ListKeyVersionsResponse, PutObjectRequest, PutObjectResponse,
    },
};

/// The version used when creating a store or key.
pub const INITIAL_VERSION: i64 = 0;

/// A key version which disables conditional-write or -delete checks.
pub const NO_VERSION_CHECK: i64 = -1;

/// Default timeout for a single VSS HTTP request.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// A signature-authenticated VSS client with bounded retries.
#[derive(Clone)]
pub struct VssClient {
    inner: Arc<UpstreamClient<Retry>>,
}

impl VssClient {
    /// Constructs an HTTPS client with the given TLS config.
    pub fn new(
        user_agent: &'static str,
        base_url: &str,
        secret_key: secp256k1::SecretKey,
        tls_config: rustls::ClientConfig,
    ) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .https_only(true)
            .timeout(DEFAULT_REQUEST_TIMEOUT)
            .use_preconfigured_tls(tls_config)
            .build()
            .expect("Failed to build VSS reqwest client");
        Self::from_client(base_url, client, secret_key)
    }

    /// Constructs a VSS client using the caller's HTTP and TLS settings.
    pub fn from_client(
        base_url: &str,
        client: reqwest::Client,
        secret_key: secp256k1::SecretKey,
    ) -> Self {
        let base_url = base_url.trim_end_matches('/').to_owned();
        let headers: Arc<dyn VssHeaderProvider> =
            Arc::new(SigsAuthProvider::new(secret_key, HashMap::new()));
        let inner = UpstreamClient::from_client_and_headers(
            base_url,
            client,
            helpers::retry_policy(),
            headers,
        );
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Fetches the object identified by `req`.
    pub fn get_object(
        &self,
        req: &GetObjectRequest,
    ) -> impl Future<Output = Result<GetObjectResponse, VssError>> {
        self.inner.get_object(req)
    }

    /// Applies all puts and deletes in `req` atomically.
    pub fn put_object(
        &self,
        req: &PutObjectRequest,
    ) -> impl Future<Output = Result<PutObjectResponse, VssError>> {
        self.inner.put_object(req)
    }

    /// Deletes the object identified by `req`.
    pub fn delete_object(
        &self,
        req: &DeleteObjectRequest,
    ) -> impl Future<Output = Result<DeleteObjectResponse, VssError>> {
        self.inner.delete_object(req)
    }

    /// Lists one page of key versions matching `req`.
    pub fn list_key_versions(
        &self,
        req: &ListKeyVersionsRequest,
    ) -> impl Future<Output = Result<ListKeyVersionsResponse, VssError>> {
        self.inner.list_key_versions(req)
    }
}

type Retry = FilteredRetryPolicy<
    JitteredRetryPolicy<
        MaxAttemptsRetryPolicy<ExponentialBackoffRetryPolicy<VssError>>,
    >,
    fn(&VssError) -> bool,
>;

mod helpers {
    use super::*;

    const RETRY_BASE_DELAY: Duration = Duration::from_millis(250);
    const RETRY_MAX_ATTEMPTS: u32 = 5;
    const RETRY_MAX_JITTER: Duration = Duration::from_millis(100);

    pub(crate) fn retry_policy() -> Retry {
        ExponentialBackoffRetryPolicy::new(RETRY_BASE_DELAY)
            .with_max_attempts(RETRY_MAX_ATTEMPTS)
            .with_max_jitter(RETRY_MAX_JITTER)
            .skip_retry_on_error(is_terminal_error as fn(&VssError) -> bool)
    }

    pub(crate) fn is_terminal_error(error: &VssError) -> bool {
        match error {
            VssError::AuthError(_)
            | VssError::ConflictError(_)
            | VssError::InvalidRequestError(_)
            | VssError::NoSuchKeyError(_)
            | VssError::VSSVersionMismatchError { .. } => true,
            VssError::InternalError(_) | VssError::InternalServerError(_) =>
                false,
        }
    }
}
