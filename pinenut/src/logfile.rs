//! The `Logfile` implementation.

use std::{
    fs,
    fs::{DirEntry, File},
    io::{Error, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::Arc,
};

use crate::{DateTime, Domain, FILE_EXTENSION};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Read,
    Write,
}

pub(crate) struct Logfile {
    domain: Arc<Domain>,
    datetime: DateTime,
    mode: Mode,
    lazy_file: Option<File>,
}

impl Logfile {
    const NAME_SEPARATOR: &'static str = "-";

    #[inline]
    pub(crate) fn new(domain: Arc<Domain>, datetime: DateTime, mode: Mode) -> Self {
        Self { domain, datetime, mode, lazy_file: None }
    }

    #[inline]
    pub(crate) fn datetime(&self) -> DateTime {
        self.datetime
    }

    #[inline]
    pub(crate) fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let file = self.open()?;
        file.seek(SeekFrom::End(0))?;
        file.write_all(bytes)
    }

    #[inline]
    pub(crate) fn flush(&mut self) -> Result<(), Error> {
        let file = self.open()?;
        file.flush()?;
        file.sync_all()
    }

    #[inline]
    pub(crate) fn delete(mut self) -> Result<(), Error> {
        self.lazy_file = None;
        fs::remove_file(self.path())
    }

    pub(crate) fn open(&mut self) -> Result<&mut File, Error> {
        if self.lazy_file.is_none() {
            let path = self.path();
            let to_write = self.mode == Mode::Write;

            // Creates all intermediate directories if they are missing.
            if to_write && let Some(parent_path) = path.parent() {
                fs::create_dir_all(parent_path)?;
            }

            let file = fs::OpenOptions::new()
                .read(!to_write)
                .append(to_write)
                .create(to_write)
                .open(path)?;

            self.lazy_file = Some(file);
        }

        // SAFETY: a `None` variant for `lazy_file` would have been replaced by a `Some`
        // variant in the code above.
        Ok(unsafe { self.lazy_file.as_mut().unwrap_unchecked() })
    }

    #[inline]
    pub(crate) fn path(&self) -> PathBuf {
        self.domain.directory.join(Self::name(&self.domain, self.datetime))
    }

    #[inline]
    fn name(Domain { identifier, .. }: &Domain, datetime: DateTime) -> String {
        format!("{}{}{}.{}", identifier, Self::NAME_SEPARATOR, datetime.timestamp(), FILE_EXTENSION)
    }
}

impl Logfile {
    #[inline]
    pub(crate) fn logfiles(
        domain: &Arc<Domain>,
        mode: Mode,
    ) -> Result<impl Iterator<Item = Self> + '_, Error> {
        Ok(fs::read_dir(&domain.directory)?
            .filter_map(|entry| entry.ok())
            .filter_map(move |entry| Self::from_entry(entry, Arc::clone(domain), mode)))
    }

    fn from_entry(entry: DirEntry, domain: Arc<Domain>, mode: Mode) -> Option<Self> {
        let name = PathBuf::from(entry.file_name());
        if name.extension() != Some(FILE_EXTENSION.as_ref()) {
            return None;
        }

        let (identifier, timestamp) =
            name.file_stem()?.to_str()?.split_once(Self::NAME_SEPARATOR)?;

        if identifier != domain.identifier {
            return None;
        }
        // For chunk, time accuracy does not have to be down to nanoseconds.
        let datetime = DateTime::from_timestamp(timestamp.parse().ok()?, 0)?;

        Some(Self::new(domain, datetime, mode))
    }
}
