use std::{
    fs,
    io::{self, Read},
    path::PathBuf,
};

use anyhow::Context;

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
    base_dir: PathBuf,
}

impl FlatFileFs {
    /// Create a new [`FlatFileFs`] without ensuring that the directory exists.
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Create a new [`FlatFileFs`] ready for use.
    ///
    /// Normally, it's expected that this directory already exists. In case that
    /// directory doesn't exist, this fn will create `base_dir` and any parent
    /// directories.
    pub fn create_dir_all(base_dir: PathBuf) -> anyhow::Result<Self> {
        fs::create_dir_all(&base_dir).with_context(|| {
            format!("Failed to create directory ({})", base_dir.display())
        })?;
        Ok(Self::new(base_dir))
    }

    /// Create a new [`FlatFileFs`] at `base_dir`, but clean any existing files
    /// first.
    pub fn create_clean_dir_all(base_dir: PathBuf) -> anyhow::Result<Self> {
        // Clean up any existing directory, if it exists.
        if let Err(err) = fs::remove_dir_all(&base_dir) {
            match err.kind() {
                io::ErrorKind::NotFound => (),
                _ => return Err(anyhow::Error::new(err))
                    .with_context(|| {
                        format!(
                            "Something went wrong while trying to clean the directory ({})",
                            base_dir.display(),
                        )
                    }),
            }
        }

        Self::create_dir_all(base_dir)
    }
}

impl Ffs for FlatFileFs {
    fn read_into(&self, filename: &str, buf: &mut Vec<u8>) -> io::Result<()> {
        let mut file = fs::File::open(self.base_dir.join(filename))?;
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
        // NOTE: could use `atomicwrites` crate to make this a little safer
        // against random crashes. definitely not free though; costs at
        // least 5 ms per write on Linux (while macOS just ignores fsyncs lol).
        fs::write(self.base_dir.join(filename), data)?;
        Ok(())
    }

    fn delete_all(&self) -> io::Result<()> {
        fs::remove_dir_all(&self.base_dir)?;
        fs::create_dir(&self.base_dir)?;
        Ok(())
    }

    fn delete(&self, filename: &str) -> io::Result<()> {
        fs::remove_file(self.base_dir.join(filename))?;
        Ok(())
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
