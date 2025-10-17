use std::ops::DerefMut;

use anyhow::{Context, ensure};
use bytes::Bytes;
use reqwest::{IntoUrl, Method};
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::watch;

use crate::{
    Error,
    models::{Empty, GFile, GFileCow, GFileId, ListFiles, ListFilesResponse},
    oauth2::{self, GDriveCredentials, ReqwestClient},
};

const BASE_URL: &str = "https://www.googleapis.com/drive/v3";
const BASE_UPLOAD_URL: &str = "https://www.googleapis.com/upload/drive/v3";
pub(crate) const FOLDER_MIME_TYPE: &str = "application/vnd.google-apps.folder";
pub(crate) const BINARY_MIME_TYPE: &str = "application/octet-stream";

/// A crate-private Google Drive API client which:
///
/// - Exposes methods to manipulate GDrive files and folders.
/// - Handles API/REST semantics and (de)serialization.
/// - Manages shared access to the underlying API credentials, including
///   refreshing access tokens when needed.
/// - Includes access tokens in requests.
pub(crate) struct GDriveClient {
    client: ReqwestClient,
    credentials: tokio::sync::Mutex<GDriveCredentials>,
    credentials_tx: watch::Sender<GDriveCredentials>,
}

impl GDriveClient {
    pub fn new(
        client: ReqwestClient,
        credentials: GDriveCredentials,
    ) -> (Self, watch::Receiver<GDriveCredentials>) {
        let (credentials_tx, mut credentials_rx) =
            watch::channel(credentials.clone());
        // Mark the current value as seen so that the first call to changed()
        // does not return immediately, to prevent redundant persists.
        credentials_rx.borrow_and_update();
        assert!(!credentials_rx.has_changed().expect("already closed??"));

        let myself = Self {
            client,
            credentials: tokio::sync::Mutex::new(credentials),
            credentials_tx,
        };

        (myself, credentials_rx)
    }

    // --- Helpers --- //
    // These higher-level methods build on the raw API bindings to provide some
    // useful helpers, and return anyhow::Error to make debugging easier

    /// Given the [`GFileId`] of a directory, searches its direct children for a
    /// file (or directory) that is named `child_name` and returns its metadata.
    pub async fn search_direct_children(
        &self,
        parent_id: &GFileId,
        child_name: &str,
    ) -> anyhow::Result<Option<GFile>> {
        // As of 2023-09-20, "parents" only includes direct parents, thank god.
        let q = format!(
            "name = '{child_name}' and '{parent_id}' in parents \
            and trashed = false"
        );
        let data = ListFiles {
            q: q.into(),
            ..Default::default()
        };

        let mut files =
            self.list_files(&data).await.context("list_files")?.files;

        ensure!(
            files.len() <= 1,
            "{parent_id} had multiple of '{child_name}'",
        );

        // Return the single result, if it exists.
        Ok(files.pop())
    }

    /// Given the [`GFileId`] of a directory, returns a list of metadatas for
    /// all of its direct children.
    pub async fn list_direct_children(
        &self,
        parent_id: &GFileId,
    ) -> anyhow::Result<Vec<GFile>> {
        let q = format!("'{parent_id}' in parents and trashed = false");
        let mut data = ListFiles {
            q: q.into(),
            order_by: Some("name".into()),
            ..Default::default()
        };

        let mut all_gfiles = Vec::with_capacity(2);
        let mut resp =
            self.list_files(&data).await.context("first list_files")?;
        all_gfiles.append(&mut resp.files);

        // Paginate until there are no more pages left
        while resp.next_page_token.is_some() {
            data.page_token = resp.next_page_token;
            resp = self.list_files(&data).await.context("paged list_files")?;
            all_gfiles.append(&mut resp.files);
        }

        Ok(all_gfiles)
    }

    /// Given the [`GFileId`] of a directory, creates a directory that is a
    /// direct child of the given dir. Returns the [`GFileId`] of the new child.
    pub async fn create_child_dir(
        &self,
        parent_id: GFileId,
        child_name: &str,
    ) -> anyhow::Result<GFileId> {
        let data = GFileCow {
            id: None,
            name: Some(child_name.into()),
            parents: Some(vec![parent_id]),
            mime_type: Some(FOLDER_MIME_TYPE.into()),
            ..Default::default()
        };
        let file = self
            .create_empty_file(&data)
            .await
            .context("create_empty_file")?;
        Ok(file.id)
    }

    // --- API bindings --- //
    // These slightly lower-level methods provide direct access to Drive APIs,
    // and return the matchable Error enum directly.

    /// "files.list": GET /files
    ///
    /// Performs a file search across My Drive.
    ///
    /// This is a lower-level helper; you might be looking for
    /// [`search_direct_children`] or [`list_direct_children`] instead.
    ///
    /// <https://developers.google.com/drive/api/reference/rest/v3/files/list>
    ///
    /// [`search_direct_children`]: Self::search_direct_children
    /// [`list_direct_children`]: Self::list_direct_children
    pub async fn list_files(
        &self,
        data: &ListFiles<'_>,
    ) -> Result<ListFilesResponse, Error> {
        let url = format!("{BASE_URL}/files");
        let req = self.get(&url, data);
        self.send_and_deserialize(req).await
    }

    /// "files.create": POST /files
    ///
    /// Creates a file with no content.
    /// Mostly useful for creating folders.
    pub async fn create_empty_file(
        &self,
        data: &GFileCow<'_>,
    ) -> Result<GFile, Error> {
        let req = self.post(format!("{BASE_URL}/files"), &data);
        self.send_and_deserialize(req).await
    }

    /// "files.create": POST {BASE_UPLOAD_URL}/files?uploadType=multipart
    ///
    /// Uploads a new binary blob file inside the given parent directory using
    /// the "multipart" API. [`reqwest::multipart`] is used to achieve this.
    ///
    /// We use the "multipart" API (as opposed to "simple" or "resumable")
    /// because (1) it allows file metadata and contents to be sent in a single
    /// request, (2) doesn't require two round trips for every new file
    /// uploaded. If files become so large that network reliability becomes a
    /// problem, we can consider switching to the "resumable" API.
    ///
    /// <https://developers.google.com/drive/api/guides/manage-uploads#multipart>
    pub async fn create_blob_file(
        &self,
        parent_id: GFileId,
        name: String,
        data: Vec<u8>,
    ) -> Result<GFile, Error> {
        use reqwest::multipart::{Form, Part};

        let method = Method::POST;
        let url = format!("{BASE_UPLOAD_URL}/files");
        let query = [("uploadType", "multipart")];

        let metadata = GFileCow {
            id: None,
            name: Some(name.clone().into()),
            parents: Some(vec![parent_id]),
            mime_type: Some(BINARY_MIME_TYPE.into()),
            ..Default::default()
        };

        let metadata_json = serde_json::to_string(&metadata)?;
        let metadata_part = Part::text(metadata_json)
            .mime_str("application/json; charset=UTF-8")?;

        let data_part = Part::bytes(data)
            .mime_str(BINARY_MIME_TYPE)?
            .file_name(name);

        // Metadata part needs to go first
        let multipart = Form::new()
            .part("metadata", metadata_part)
            .part("file", data_part);

        let req = self
            .client
            .request(method, url)
            .query(&query)
            // This method adds the "Content-Type" and "Content-Length" headers
            .multipart(multipart);

        self.send_and_deserialize(req).await
    }

    /// "files.update":
    /// PATCH {BASE_UPLOAD_URL}/files/{fileId}?uploadType=media
    ///
    /// Given the [`GFileId`] of a binary blob file, updates its contents to
    /// the given `data`. The metadata of the file is not changed.
    ///
    /// Uses the "simple" upload API since we don't want to change metadata.
    /// <https://developers.google.com/drive/api/guides/manage-uploads#simple>
    pub async fn update_blob_file(
        &self,
        id: GFileId,
        data: Bytes,
    ) -> Result<GFile, Error> {
        let method = Method::PATCH;
        let url = format!("{BASE_UPLOAD_URL}/files/{id}");

        let req = self
            .client
            .request(method, url)
            .query(&[("uploadType", "media")])
            .header("Content-Type", BINARY_MIME_TYPE)
            .header("Content-Length", data.len())
            .body(data);

        self.send_and_deserialize(req).await
    }

    /// "files.get": GET /files/{id}?alt=media
    ///
    /// Downloads a blob file given its ID.
    ///
    /// <https://developers.google.com/drive/api/guides/manage-downloads#download_blob_file_content>
    pub async fn download_blob_file(
        &self,
        gid: &GFileId,
    ) -> Result<Vec<u8>, Error> {
        let url = format!("{BASE_URL}/files/{gid}");
        let req = self.get(url, &Empty {}).query(&[("alt", "media")]);
        let resp = self.send_no_deserialize(req).await?;
        let bytes = resp.bytes().await?;
        let data = <Vec<u8>>::from(bytes);
        Ok(data)
    }

    /// "files.delete": DELETE {BASE_URL}/files/{fileId}
    ///
    /// Permanently deletes a file by its `gid`, skipping the trash.
    /// If the target is a folder, all descendants are also deleted.
    ///
    /// <https://developers.google.com/drive/api/reference/rest/v3/files/delete>
    pub async fn delete_file(&self, gid: &GFileId) -> Result<(), Error> {
        let url = format!("{BASE_URL}/files/{gid}");
        let req = self.client.delete(url);
        self.send_no_deserialize(req).await?;
        Ok(())
    }

    /// Create a GET request and serialize the given data into query params.
    #[inline]
    fn get(
        &self,
        url: impl IntoUrl,
        data: &(impl Serialize + ?Sized),
    ) -> reqwest::RequestBuilder {
        self.client.get(url).query(data)
    }

    /// Create a POST request and serialize the given data into a JSON body.
    #[inline]
    fn post(
        &self,
        url: impl IntoUrl,
        data: &(impl Serialize + ?Sized),
    ) -> reqwest::RequestBuilder {
        self.client.post(url).json(data)
    }

    /// Adds the bearer auth token to the request and sends the request.
    /// Deserializes the response as `T` if we got a success status; returns
    /// [`Error::Api`] otherwise. Use this for JSON endpoints.
    #[inline]
    async fn send_and_deserialize<T: DeserializeOwned>(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<T, Error> {
        self.send_no_deserialize(req)
            .await?
            .json::<T>()
            .await
            .map_err(Error::from)
    }

    /// Like `send_and_deserialize` but skips the JSON deserialization step.
    /// Use this when you need to extract a raw binary response or do anything
    /// else non-standard.
    async fn send_no_deserialize(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, Error> {
        let req = {
            let mut locked_credentials = self.credentials.lock().await;
            let updated = oauth2::refresh_if_necessary(
                &self.client,
                locked_credentials.deref_mut(),
            )
            .await
            .map_err(Box::new)
            .map_err(Error::TokenRefresh)?;

            // If the access token was refreshed, update the credentials in the
            // channel with the new access_token and expires_at timestamp.
            if updated {
                self.credentials_tx.send_modify(|c| {
                    c.access_token.clone_from(&locked_credentials.access_token);
                    c.expires_at = locked_credentials.expires_at;
                });
            }

            req.bearer_auth(&locked_credentials.access_token)
        };

        let req = req.build()?;

        // Log helpful data in tests
        #[cfg(test)]
        {
            use tracing::debug;

            let url = req.url();
            let headers = req.headers();
            let body = req
                .body()
                .and_then(|body| body.as_bytes())
                .map(String::from_utf8_lossy)
                .unwrap_or_else(|| "(No body)".into());
            debug!(%url, ?headers, %body, "Request");
        }

        let resp = self.client.execute(req).await?;

        let code = resp.status();
        if code.is_success() {
            Ok(resp)
        } else {
            let resp_str = match resp.bytes().await {
                Ok(b) => String::from_utf8_lossy(&b).to_string(),
                Err(e) => format!("Failed to get error response text: {e:#}"),
            };
            Err(Error::Api { code, resp_str })
        }
    }
}
