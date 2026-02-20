//! Flat file system abstraction.

use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

use common::rng::{RngExt, ThreadFastRng};

/// Abstraction over a flat file system (no subdirs), suitable for mocking.
///
/// **Invariant**: The `Ffs` must always be ready for `read` / `write` /
/// `delete` calls, including after `delete_all`.
pub trait Ffs {
    /// Reads the entire contents of `filename`.
    ///
    /// NOTE: Use [`io::ErrorKind::NotFound`] to detect if a file is missing.
    fn read(&self, filename: &str) -> io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        self.read_into(filename, &mut buf)?;
        Ok(buf)
    }

    /// Reads the contents of `filename` into `buf`.
    fn read_into(&self, filename: &str, buf: &mut Vec<u8>) -> io::Result<()>;

    /// Reads all filenames in the `Ffs`.
    fn read_dir(&self) -> io::Result<Vec<String>> {
        let mut filenames = Vec::new();
        self.read_dir_visitor(|filename| {
            filenames.push(filename.to_owned());
            Ok(())
        })?;
        Ok(filenames)
    }

    /// Visit all filenames in the `Ffs`.
    fn read_dir_visitor(
        &self,
        dir_visitor: impl FnMut(&str) -> io::Result<()>,
    ) -> io::Result<()>;

    /// Write `data` to `filename`, overwriting any existing file.
    fn write(&self, filename: &str, data: &[u8]) -> io::Result<()>;

    /// Delete all files and directories in the `Ffs`.
    fn delete_all(&self) -> io::Result<()>;

    /// Delete file.
    fn delete(&self, filename: &str) -> io::Result<()>;
}

/// File system impl for [`Ffs`] that does real IO.
#[derive(Clone)]
pub struct DiskFs {
    /// Files are stored flat (i.e., no subdirectories) in this directory.
    base_dir: PathBuf,

    /// `{base_dir}/.write`
    ///
    /// Used to support atomic writes. We fully write files to this subdir
    /// before moving them to their final destination in `base_dir`.
    ///
    /// We store these pending writes in a subdirectory of `base_dir` to avoid
    /// crossing a filesystem boundary when moving them (which would make the
    /// move definitely not atomic).
    write_dir: PathBuf,
}

impl DiskFs {
    /// Create a new [`DiskFs`] ready for use.
    ///
    /// Normally, it's expected that this directory already exists. In case that
    /// directory doesn't exist, this fn will create `base_dir` and any parent
    /// directories.
    pub fn create_dir_all(base_dir: PathBuf) -> anyhow::Result<Self> {
        // Ensure the base_dir exists
        fs::create_dir_all(base_dir.as_path())?;

        // Clean up any write_dir from before. This could contain partially
        // complete writes from just before a crash.
        let write_dir = Self::write_dir_path(&base_dir);
        fsext::remove_dir_all_idempotent(&write_dir)?;
        fs::create_dir(write_dir.as_path())?;

        Ok(Self {
            base_dir,
            write_dir,
        })
    }

    /// Create a new [`DiskFs`] at `base_dir`, but clean any existing files
    /// first.
    pub fn create_clean_dir_all(base_dir: PathBuf) -> anyhow::Result<Self> {
        // Clean up any existing directory, if it exists.
        fsext::remove_dir_all_idempotent(&base_dir)?;
        fs::create_dir_all(base_dir.as_path())?;

        let write_dir = Self::write_dir_path(&base_dir);
        fs::create_dir(write_dir.as_path())?;

        Ok(Self {
            base_dir,
            write_dir,
        })
    }

    fn write_dir_path(base_dir: &Path) -> PathBuf {
        base_dir.join(".write")
    }
}

impl Ffs for DiskFs {
    fn read_into(&self, filename: &str, buf: &mut Vec<u8>) -> io::Result<()> {
        let mut file = fs::File::open(self.base_dir.join(filename).as_path())?;
        file.read_to_end(buf)?;
        Ok(())
    }

    fn read_dir_visitor(
        &self,
        mut dir_visitor: impl FnMut(&str) -> io::Result<()>,
    ) -> io::Result<()> {
        for maybe_file_entry in self.base_dir.read_dir()? {
            let file_entry = maybe_file_entry?;

            // Only visit files.
            if file_entry.file_type()?.is_file() {
                // Just skip non-UTF-8 filenames.
                if let Some(filename) = file_entry.file_name().to_str() {
                    dir_visitor(filename)?;
                }
            }
        }
        Ok(())
    }

    fn write(&self, filename: &str, data: &[u8]) -> io::Result<()> {
        let final_dest_path = self.base_dir.join(filename);

        // Sample a new random alphanumeric filename to use in the .write subdir
        // ex: "{base_dir}/.write/z2l86yb3zYS6CT7C".
        //
        // This way multiple threads can't partially write to the same file.
        // Only one will win, and the write will be atomic.
        let tmp_write_path = {
            let name: [u8; 16] = ThreadFastRng::new().gen_alphanum_bytes();
            let name_str = std::str::from_utf8(name.as_slice())
                .expect("ASCII is all valid UTF-8");
            self.write_dir.join(name_str)
        };

        // Low effort atomic write (sans fsync's).
        fs::write(tmp_write_path.as_path(), data)?;
        fs::rename(tmp_write_path.as_path(), final_dest_path)?;
        Ok(())
    }

    fn delete_all(&self) -> io::Result<()> {
        fs::remove_dir_all(self.base_dir.as_path())?;
        fs::create_dir(self.base_dir.as_path())?;
        // Recreate the .write dir so subsequent writes still work.
        fs::create_dir(self.write_dir.as_path())?;
        Ok(())
    }

    fn delete(&self, filename: &str) -> io::Result<()> {
        fs::remove_file(self.base_dir.join(filename).as_path())?;
        Ok(())
    }
}

/// [`std::fs`] extensions.
// TODO(max): Maybe move to lexe-std
pub mod fsext {
    use std::{fs, io, path::Path};

    /// [`std::fs::remove_dir_all`] but does not error on file not found.
    /// Returns `true` if the directory existed and was deleted.
    pub fn remove_dir_all_idempotent(dir: &Path) -> io::Result<bool> {
        match fs::remove_dir_all(dir) {
            Ok(()) => Ok(true),
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// [`Ffs`]-related test utilities.
#[cfg(feature = "test-utils")]
pub mod test_utils {
    use std::{cell::RefCell, collections::BTreeMap, io};

    use common::rng::{FastRng, shuffle};

    use super::Ffs;

    fn io_err_not_found(filename: &str) -> io::Error {
        io::Error::new(io::ErrorKind::NotFound, filename)
    }

    /// An in-memory [`Ffs`] implementation, useful for testing.
    #[derive(Debug)]
    pub struct InMemoryFfs {
        inner: RefCell<InMemoryFfsInner>,
    }

    #[derive(Debug)]
    struct InMemoryFfsInner {
        rng: FastRng,
        files: BTreeMap<String, Vec<u8>>,
    }

    impl InMemoryFfs {
        /// Create a new empty [`InMemoryFfs`].
        pub fn new() -> Self {
            Self {
                inner: RefCell::new(InMemoryFfsInner {
                    rng: FastRng::new(),
                    files: BTreeMap::new(),
                }),
            }
        }

        /// Create a new [`InMemoryFfs`] with a seeded RNG.
        pub fn from_rng(rng: FastRng) -> Self {
            Self {
                inner: RefCell::new(InMemoryFfsInner {
                    rng,
                    files: BTreeMap::new(),
                }),
            }
        }
    }

    impl Default for InMemoryFfs {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Ffs for InMemoryFfs {
        fn read_into(
            &self,
            filename: &str,
            buf: &mut Vec<u8>,
        ) -> io::Result<()> {
            match self.inner.borrow().files.get(filename) {
                Some(data) => buf.extend_from_slice(data),
                None => return Err(io_err_not_found(filename)),
            }
            Ok(())
        }

        fn read_dir_visitor(
            &self,
            mut dir_visitor: impl FnMut(&str) -> io::Result<()>,
        ) -> io::Result<()> {
            // Shuffle the file order to ensure we don't rely on it.
            let mut filenames = self
                .inner
                .borrow()
                .files
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            {
                let rng = &mut self.inner.borrow_mut().rng;
                shuffle(rng, &mut filenames);
            }

            for filename in &filenames {
                dir_visitor(filename)?;
            }
            Ok(())
        }

        fn write(&self, filename: &str, data: &[u8]) -> io::Result<()> {
            self.inner
                .borrow_mut()
                .files
                .insert(filename.to_owned(), data.to_owned());
            Ok(())
        }

        fn delete_all(&self) -> io::Result<()> {
            self.inner.borrow_mut().files = BTreeMap::new();
            Ok(())
        }

        fn delete(&self, filename: &str) -> io::Result<()> {
            match self.inner.borrow_mut().files.remove(filename) {
                Some(_) => Ok(()),
                None => Err(io_err_not_found(filename)),
            }
        }
    }
}
