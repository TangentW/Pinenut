use std::{
    io,
    io::{BufReader, BufWriter, Read, Seek, Write},
    ops::RangeInclusive,
    path::{Path, PathBuf},
    sync::Arc,
};

use thiserror::Error;

use crate::{chunk, common, common::LazyFileWriter, logfile, logfile::Logfile, DateTime, Domain};

/// Errors that can be occurred during the log extraction process ([`extract`]).
#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("the log file is invalid: {0}")]
    FileInvalid(PathBuf),
    #[error("the log file is incomplete: {0}")]
    FileIncomplete(PathBuf),
    #[error("logs in the specified time range were not found")]
    NotFound,
}

/// Extracts the logs for the specified time range and writes them to the destination
/// file.
///
/// Errors may be occurred during log writing, and the destination file may have been
/// created by then. The caller is responsible for managing the destination file
/// (e.g., deleting it) afterwards.
pub fn extract(
    domain: Domain,
    time_range: RangeInclusive<DateTime>,
    dest_path: impl AsRef<Path>,
) -> Result<(), Error> {
    let dest_path = dest_path.as_ref();
    let mut writer = BufWriter::new(LazyFileWriter::new(dest_path));

    for mut logfile in logfiles(domain, &time_range)? {
        let mut reader = BufReader::new(logfile.open()?);
        extract_chunks(&mut reader, &mut writer, &time_range)
            .map_err(|err| Error::from_chunk_error(err, logfile.path()))?;
    }

    if writer.into_inner().map_err(|err| err.into_error())?.is_empty() {
        Err(Error::NotFound)
    } else {
        Ok(())
    }
}

// ============ Internal ============

fn extract_chunks<R, W>(
    reader: &mut R,
    writer: &mut W,
    time_range: &RangeInclusive<DateTime>,
) -> Result<(), chunk::ReadError>
where
    R: Read + Seek,
    W: Write,
{
    let mut reader = chunk::Reader::new(reader);
    while let Some(header) = reader.read_header_or_reach_to_end()? {
        if header.time_range().start().gt(time_range.end()) {
            return Ok(());
        }

        let payload_len = header.payload_len();

        if header.time_range().end().lt(time_range.start()) {
            reader.skip(payload_len)?;
            continue;
        }

        // Write header.
        let header_bytes = header.clone().bytes();
        writer.write_all(header_bytes.as_ref())?;

        type FnSink<F> = common::FnSink<F, chunk::ReadError>;

        // Write payload.
        reader.read_payload(
            payload_len,
            &mut FnSink::new(|bytes: &[u8]| writer.write_all(bytes).map_err(Into::into)),
        )?;
    }
    Ok(())
}

fn logfiles(domain: Domain, time_range: &RangeInclusive<DateTime>) -> Result<Vec<Logfile>, Error> {
    let mut original =
        Logfile::logfiles(&Arc::new(domain), logfile::Mode::Read)?.collect::<Vec<_>>();
    original.sort_by_key(|f| f.datetime());

    let mut logfiles = Vec::new();

    for logfile in original {
        if logfile.datetime().ge(time_range.end()) {
            break;
        }
        if logfile.datetime().le(time_range.start()) && !logfiles.is_empty() {
            logfiles.clear();
        }
        logfiles.push(logfile);
    }

    Ok(logfiles)
}

impl Error {
    #[inline]
    fn from_chunk_error(error: chunk::ReadError, path: PathBuf) -> Self {
        use chunk::ReadError::*;
        match error {
            Io(err) => Self::Io(err),
            Invalid => Self::FileInvalid(path),
            UnexpectedEnd => Self::FileIncomplete(path),
        }
    }
}
