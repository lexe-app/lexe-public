use std::{future::Future, pin::Pin, sync::Arc};

use anyhow::Context;
use lexe_api::def::NodeBackendApi;
use lexe_common::api::user::UserPk;

use crate::client::NodeBackendClient;

type QuickCache<K, V> = quick_cache::sync::Cache<K, V>;

const USER_EXISTS_CAPACITY: usize = 1000;

/// Async closure that checks whether a user with the given [`UserPk`] exists.
pub(crate) type UserExistsFn = Arc<
    dyn Fn(UserPk) -> Pin<Box<dyn Future<Output = anyhow::Result<bool>> + Send>>
        + Send
        + Sync,
>;

/// Caches whether users exist.
pub(crate) struct UserCache {
    backend_api: Arc<NodeBackendClient>,

    /// Caches [`UserPk`] -> `bool` indicating whether the user exists.
    ///
    /// NOTE: This cache is not strongly consistent because it's always
    /// possible to sign up a user after [`false`] has been cached. Only use
    /// this for use cases where eventual consistency is acceptable.
    user_exists: QuickCache<UserPk, bool>,
}

impl UserCache {
    pub(crate) fn new(backend_api: Arc<NodeBackendClient>) -> Self {
        Self {
            backend_api,
            user_exists: QuickCache::new(USER_EXISTS_CAPACITY),
        }
    }

    /// Returns whether a user with the given [`UserPk`] exists.
    async fn user_exists(&self, user_pk: UserPk) -> anyhow::Result<bool> {
        self.user_exists
            .get_or_insert_async(&user_pk, async {
                let resp = self
                    .backend_api
                    .get_user(user_pk)
                    .await
                    .context("Failed to fetch user from backend")?;
                Ok(resp.maybe_user.is_some())
            })
            .await
    }

    /// Returns a [`UserExistsFn`] async closure for checking if a user exists.
    pub(crate) fn user_exists_fn(self: &Arc<Self>) -> UserExistsFn {
        let this = self.clone();
        Arc::new(move |user_pk| {
            let this = this.clone();
            Box::pin(async move { this.user_exists(user_pk).await })
        })
    }
}
