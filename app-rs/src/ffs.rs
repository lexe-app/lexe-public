//! Flat file system abstraction

use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

use common::rng::{RngExt, ThreadWeakRng};

/// Abstraction over a flat file system (no subdirs), suitable for mocking.
pub trait Ffs {
    /// NOTE: Use [`io::ErrorKind::NotFound`] to detect if a file is missing.
    fn read(&self, filename: &str) -> io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        self.read_into(filename, &mut buf)?;
        Ok(buf)
    }
    fn read_into(&self, filename: &str, buf: &mut Vec<u8>) -> io::Result<()>;

    fn read_dir(&self) -> io::Result<Vec<String>> {
        let mut filenames = Vec::new();
        self.read_dir_visitor(|filename| {
            filenames.push(filename.to_owned());
            Ok(())
        })?;
        Ok(filenames)
    }
    fn read_dir_visitor(
        &self,
        dir_visitor: impl FnMut(&str) -> io::Result<()>,
    ) -> io::Result<()>;

    fn write(&self, filename: &str, data: &[u8]) -> io::Result<()>;

    /// Delete all files and directories in the `Ffs`.
    fn delete_all(&self) -> io::Result<()>;

    /// Delete file.
    fn delete(&self, filename: &str) -> io::Result<()>;
}

/// File system impl for [`Ffs`] that does real IO.
pub struct FlatFileFs {
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

impl FlatFileFs {
    /// Create a new [`FlatFileFs`] ready for use.
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

    /// Create a new [`FlatFileFs`] at `base_dir`, but clean any existing files
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

impl Ffs for FlatFileFs {
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
            let name: [u8; 16] = ThreadWeakRng::new().gen_alphanum_bytes();
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
        Ok(())
    }

    fn delete(&self, filename: &str) -> io::Result<()> {
        fs::remove_file(self.base_dir.join(filename).as_path())?;
        Ok(())
    }
}

mod fsext {
    use std::{fs, io, path::Path};

    /// [`std::fs::remove_dir_all`] but does not error on file not found.
    pub(crate) fn remove_dir_all_idempotent(dir: &Path) -> io::Result<()> {
        match fs::remove_dir_all(dir) {
            Ok(()) => Ok(()),
            Err(ref err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }
}

#[cfg(test)]
pub(crate) mod test {
    use std::{cell::RefCell, collections::BTreeMap};

    use common::rng::{shuffle, WeakRng};

    use super::*;

    fn io_err_not_found(filename: &str) -> io::Error {
        io::Error::new(io::ErrorKind::NotFound, filename)
    }

    /// An in-memory mock [`Ffs`] implementation.
    #[derive(Debug)]
    pub(crate) struct MockFfs {
        inner: RefCell<MockFfsInner>,
    }

    #[derive(Debug)]
    struct MockFfsInner {
        rng: WeakRng,
        files: BTreeMap<String, Vec<u8>>,
    }

    impl MockFfs {
        pub(crate) fn new() -> Self {
            Self {
                inner: RefCell::new(MockFfsInner {
                    rng: WeakRng::new(),
                    files: BTreeMap::new(),
                }),
            }
        }

        pub(crate) fn from_rng(rng: WeakRng) -> Self {
            Self {
                inner: RefCell::new(MockFfsInner {
                    rng,
                    files: BTreeMap::new(),
                }),
            }
        }
    }

    impl Ffs for MockFfs {
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
            // shuffle the file order to ensure we don't rely on it.
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
