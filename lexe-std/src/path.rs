use std::{
    ffi::OsStr,
    path::{self, Path, PathBuf},
};

pub trait PathExt {
    /// Backport of [`Path::file_prefix`].
    // TODO(phlip9): Remove once MSRV >= 1.91; use `Path::file_prefix`.
    fn file_prefix_stable(&self) -> Option<&OsStr>;

    /// Return a new `PathBuf` with all existing extensions removed and
    /// replaced with `new_ext`.
    ///
    /// Leaves the filename unchanged if removal would produce `.` or `..`.
    ///
    /// ex: "foo.tar.gz" with `new_ext="zip"` becomes "foo.zip"
    fn with_all_extensions(&self, new_ext: &str) -> PathBuf;
}

impl PathExt for Path {
    fn file_prefix_stable(&self) -> Option<&OsStr> {
        let file_name = self.file_name()?;
        let bytes = file_name.as_encoded_bytes();
        let Some(idx) = bytes[1..].iter().position(|b| *b == b'.') else {
            return Some(file_name);
        };
        // SAFETY: The slice ends immediately before an ASCII dot in an
        // existing OsStr, so it ends at a valid encoding boundary.
        Some(unsafe { OsStr::from_encoded_bytes_unchecked(&bytes[..idx + 1]) })
    }

    fn with_all_extensions(&self, new_ext: &str) -> PathBuf {
        assert!(
            !new_ext.contains(path::is_separator),
            "extension cannot contain path separators: {new_ext:?}",
        );
        let Some(prefix) = self.file_prefix_stable() else {
            return self.to_path_buf();
        };

        let mut file_name = prefix.to_os_string();
        if !new_ext.is_empty() {
            file_name.push(".");
            file_name.push(new_ext);
        }
        if file_name == "." || file_name == ".." {
            return self.to_path_buf();
        }
        self.with_file_name(file_name)
    }
}

#[cfg(test)]
mod test {
    use std::path::Path;

    use super::PathExt;

    #[test]
    fn test_with_all_extensions() {
        let cases = [
            ("foo.txt", "md", "foo.md"),
            ("foo.tar.gz", "zip", "foo.zip"),
            ("foo", "txt", "foo.txt"),
            (".hiddenfile", "txt", ".hiddenfile.txt"),
            (".config.toml", "json", ".config.json"),
            ("archive.backup.tar.gz", "7z", "archive.7z"),
            ("client.pem.age", "cert.pem", "client.cert.pem"),
            ("foo.tar.gz", "", "foo"),
            ("foo.", "zip", "foo.zip"),
            ("..upload", "zip", "..zip"),
            ("...upload", "zip", "..zip"),
            ("...", "zip", "..zip"),
            ("....", "zip", "..zip"),
            ("..upload", "", "..upload"),
            ("...upload", "", "...upload"),
            ("...", "", "..."),
            ("....", "", "...."),
        ];

        for parent in [Path::new(""), Path::new("parent.with.dots/child")] {
            for (input, new_ext, expected) in cases {
                let result = parent.join(input).with_all_extensions(new_ext);
                assert_eq!(result, parent.join(expected));
            }
        }

        for path in ["", ".", "..", "/"] {
            assert_eq!(
                Path::new(path).with_all_extensions("zip"),
                Path::new(path)
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_with_all_extensions_non_utf8() {
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

        let path = Path::new(OsStr::from_bytes(b"parent/\xff.tar.\xfe"));
        let expected = Path::new(OsStr::from_bytes(b"parent/\xff.zip"));
        assert_eq!(path.with_all_extensions("zip"), expected);
    }
}
