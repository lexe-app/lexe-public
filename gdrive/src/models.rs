use std::{
    borrow::Cow,
    fmt::{self, Display},
};

use serde::{Deserialize, Serialize};

/// The metadata associated with a Google Drive "File".
/// NOTE: GDrive "files" include folders as well.
///
/// This struct is used only for deserialization.
/// See [`GFileCow`] for a write-optimized version suitable for sending.
///
/// Details: <https://developers.google.com/drive/api/reference/rest/v3/files>
// Technically the two structs could be merged, but adding a lifetime parameter
// to [`GFile`] adds useless lifetime generics to everything.
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GFile {
    pub id: GFileId,
    pub name: String,
    pub mime_type: String,
    // kind: String, // Always "drive#file"
}

/// A version of [`GFile`] optimized for serialization.
///
/// `id` is also an optional field (relevant when creating files)
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GFileCow<'a> {
    pub id: Option<GFileId>,
    pub name: Option<Cow<'a, str>>,
    pub parents: Option<Vec<GFileId>>,
    pub mime_type: Option<Cow<'a, str>>,
    pub folder_color_rgb: Option<Cow<'a, str>>,
}

/// A newtype for the `fileId` associated with every file or folder in Google
/// Drive, to ensure that this isn't confused for `VfsFileId`.
#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GFileId(pub String);

/// A struct denoting an empty API request or response.
// We don't use `lexe_api_core::Empty` in case Lexe's API diverges from Google's
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct Empty {}

/// GET /files
///
/// <https://developers.google.com/drive/api/reference/rest/v3/files/list>
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListFiles<'a> {
    /// The search query to use.
    /// More info: <https://developers.google.com/drive/api/guides/search-files>
    /// NOTE: Don't forget to include "trashed = false"!!
    pub q: Cow<'a, str>,
    /// A comma-separated list of sort keys.
    ///
    /// Valid keys are 'createdTime', 'folder', 'modifiedByMeTime',
    /// 'modifiedTime', 'name', 'name_natural', 'quotaBytesUsed', 'recency',
    /// 'sharedWithMeTime', 'starred', and 'viewedByMeTime'.
    ///
    /// Each key sorts ascending by default, but can be reversed with the
    /// 'desc' modifier.
    ///
    /// Example: "folder,modifiedTime desc,name".
    pub order_by: Option<Cow<'a, str>>,
    /// "The token for continuing a previous list request on the next page.
    /// This should be set to the value of 'nextPageToken' from the
    /// previous response." Is [`None`] if there are no more results.
    pub page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListFilesResponse {
    pub files: Vec<GFile>,
    pub next_page_token: Option<String>,
    // This is always `false`. It can only be true if we're searching multiple
    // drives, which we aren't.
    // incomplete_search: bool,
    // This is always "drive#fileList"
    // kind: String,
}

impl Display for GFileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
