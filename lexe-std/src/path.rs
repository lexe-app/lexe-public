use std::path::{Path, PathBuf};

pub trait PathExt {
    /// Return a new `PathBuf` with all existing extensions removed and
    /// replaced with `new_ext`.
    ///
    /// ex: "foo.tar.gz" with `new_ext="zip"` becomes "foo.zip"
    fn with_all_extensions(&self, new_ext: &str) -> PathBuf;
}

impl PathExt for Path {
    fn with_all_extensions(&self, new_ext: &str) -> PathBuf {
        let mut path = self.to_path_buf();
        while path.extension().is_some() {
            path.set_extension("");
        }
        path.set_extension(new_ext);
        path
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
            ("archive.backup.tar.gz", "7z", "archive.7z"),
        ];

        for (input, new_ext, expected) in cases {
            let input_path = Path::new(input);
            let result = input_path.with_all_extensions(new_ext);
            assert_eq!(result, Path::new(expected));
        }
    }
}
