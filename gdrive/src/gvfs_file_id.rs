use std::{fmt, fmt::Display, str::FromStr};

use anyhow::{Context, ensure};
use lexe_api_core::vfs::VfsFileId;

/// Uniquely identifies a file stored in a GVFS.
/// - Used as the [`GFile::name`] for files in Google Drive.
/// - Basically a [`VfsFileId`] flattened into the form `<dirname>/<filename>`;
///   VFS:[`VfsFileId`]::GVFS:[`GvfsFileId`].
///
/// Enforces the following invariants:
/// - Contains exactly one '/', i.e. the dirname and filename don't contain '/'.
/// - The dirname and filename are NOT just the empty string "".
///
/// On the choice of '/' as the delimiter:
/// - AFAICT, all characters are allowed in Google Drive file names, even '/'.
/// - Lexe users that use the Google Drive desktop client to sync their files to
///   their computer will have the '/'s replaced with ' 's, so no worries there.
///
/// [`GFile::name`]: crate::models::GFile::name
#[derive(Clone)]
pub struct GvfsFileId(String);

impl GvfsFileId {
    /// Destructure into the contained [`String`].
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Get a reference to the contained `dirname`.
    pub fn dirname(&self) -> &str {
        self.as_parts().0
    }

    /// Get a reference to the contained `filename`.
    pub fn filename(&self) -> &str {
        self.as_parts().1
    }

    /// Get a tuple of references to the contained `dirname` and `filename`.
    pub fn as_parts(&self) -> (&str, &str) {
        self.0.split_once('/').expect("Invariant violated")
    }

    /// Constructs a [`VfsFileId`] from the contained information.
    pub fn to_vfile_id(&self) -> VfsFileId {
        VfsFileId::new(self.dirname().to_owned(), self.filename().to_owned())
    }

    /// Validates whether the given [`&str`] is a valid [`GvfsFileId`] without
    /// allocating anything. Centralizes validation logic.
    pub fn validate(s: &str) -> anyhow::Result<()> {
        let mut parts = s.split('/');
        let dirname = parts.next().expect("First item is always Some");
        let filename = parts
            .next()
            .with_context(|| format!("'{s}' did not have a '/'"))?;
        ensure!(parts.next().is_none(), "'{s}' had more than one '/'");

        ensure!(!dirname.is_empty(), "dirname part cannot be empty: '{s}'");
        ensure!(!filename.is_empty(), "filename part cannot be empty: '{s}'");

        Ok(())
    }
}

impl Display for GvfsFileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        String::fmt(&self.0, f)
    }
}

impl TryFrom<String> for GvfsFileId {
    type Error = anyhow::Error;
    fn try_from(s: String) -> anyhow::Result<Self> {
        Self::validate(&s)?;
        Ok(Self(s))
    }
}

impl FromStr for GvfsFileId {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        Self::validate(s)?;
        Ok(Self(s.to_owned()))
    }
}

impl TryFrom<&VfsFileId> for GvfsFileId {
    type Error = anyhow::Error;
    fn try_from(vfile_id: &VfsFileId) -> anyhow::Result<Self> {
        let dirname = &vfile_id.dir.dirname;
        let filename = &vfile_id.filename;
        let inner = format!("{dirname}/{filename}");
        Self::validate(&inner)?;
        Ok(Self(inner))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_validate() {
        let all_bad = [
            "",
            "/",
            "dirname/",
            "/filename",
            "dirname/filename/",
            "/dirname/filename",
            "/dirname/filename/",
        ];
        let all_good = ["./filename", "dirname/filename"];

        for bad in all_bad {
            println!("Trying '{bad}'");
            assert!(GvfsFileId::validate(bad).is_err());
        }
        for good in all_good {
            println!("Trying '{good}'");
            assert!(GvfsFileId::validate(good).is_ok());
        }
    }
}
