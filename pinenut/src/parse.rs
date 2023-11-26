use std::{
    collections::HashMap,
    fs::File,
    io,
    io::{BufReader, BufWriter, Write},
    ops::{Deref, RangeInclusive},
    path::Path,
};

use thiserror::Error;

use crate::{
    chunk,
    codec::Decode,
    common::{BytesBuf, FnSink, LazyFileWriter},
    compress::{Decompressor, ZstdDecompressor},
    encrypt::{
        ecdh::{ecdh_encryption_key, EMPTY_PUBLIC_KEY},
        AesDecryptor, Decryptor,
    },
    DateTime, DecodingError, DecompressionError, DecryptionError, EncryptionError, EncryptionKey,
    PublicKey, Record, SecretKey, BUFFER_LEN, FORMAT_VERSION,
};

/// Errors that can be occurred during the log parsing process ([`parse`]).
#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("the log file is invalid")]
    FileInvalid,
    #[error("the log file is incomplete")]
    FileIncomplete,

    // Chunk errors:
    #[error("decrypt error: {0}, in {1:?}")]
    Decrypt(DecryptionError, RangeInclusive<DateTime>),
    #[error("decompress error: {0}, in {1:?}")]
    Decompress(DecompressionError, RangeInclusive<DateTime>),
    #[error("decode error: {0}, in {1:?}")]
    Decode(DecodingError, RangeInclusive<DateTime>),

    // The collection of chunk errors.
    #[error("chunk errors: {:#?}", .0.iter().map(|e|e.to_string()).collect::<Vec<_>>())]
    Chunks(Vec<Error>),
}

/// Parses the compressed and encrypted binary log file into multiple log records and
/// calls them back one by one.
pub fn parse(
    path: impl AsRef<Path>,
    secret_key: Option<SecretKey>,
    callback: impl FnMut(&Record) -> Result<(), io::Error>,
) -> Result<(), Error> {
    let reader = BufReader::new(File::open(path.as_ref())?);
    let mut reader = chunk::Reader::new(reader);

    let parser = RecordParser::new(callback);
    let mut processor = Processor::new(secret_key, parser);

    let mut chunk_errors = Vec::new();

    while let Some(header) = reader.read_header_or_reach_to_end()? {
        // Version is not supported, just skips this chunk.
        if header.version() != FORMAT_VERSION {
            continue;
        }

        let payload_len = header.payload_len();
        let time_range = header.time_range().start()..=header.time_range().end();
        let mut sink =
            processor.chunk_sink(payload_len, header.pub_key(), time_range, header.writeback());

        if let Err(err) = reader.read_payload(payload_len, &mut sink) {
            if err.can_continue_to_read_chunk() {
                chunk_errors.push(err);
            } else {
                return Err(err);
            }
        }
    }

    if chunk_errors.is_empty() {
        Ok(())
    } else {
        Err(Error::Chunks(chunk_errors))
    }
}

/// Represents a formatter that formats log records into readable text.
pub trait Format {
    /// Formats the log record then passes the result to the writer.
    fn format(&mut self, record: &Record, writer: &mut impl Write) -> io::Result<()>;
}

/// Parses the compressed and encrypted binary log file into readable text file.
///
/// Errors may be occurred during log writing, and the destination file may have been
/// created by then. The caller is responsible for managing the destination file
/// (e.g., deleting it) afterwards.
#[inline]
pub fn parse_to_file(
    path: impl AsRef<Path>,
    dest_path: impl AsRef<Path>,
    secret_key: Option<SecretKey>,
    mut formatter: impl Format,
) -> Result<(), Error> {
    let dest_path = dest_path.as_ref();
    let mut writer = BufWriter::new(LazyFileWriter::new(dest_path));
    parse(path, secret_key, |record| formatter.format(record, &mut writer))
}

/// The default formatter provides simple log formatting.
pub struct DefaultFormatter;

impl Format for DefaultFormatter {
    #[inline]
    fn format(&mut self, record: &Record, writer: &mut impl Write) -> io::Result<()> {
        const LEVELS: [&str; 5] = ["E", "W", "I", "D", "V"];
        let (meta, content) = (record.meta(), record.content());
        let datetime: chrono::DateTime<chrono::Local> = meta.datetime().into();

        writeln!(
            writer,
            "[{}] {}|{}|{}:{}|{}|{}",
            LEVELS[meta.level() as usize - 1],
            datetime.format("%F %T%.3f"),
            meta.thread_id().unwrap_or(0),
            meta.location().file().unwrap_or(""),
            meta.location().line().unwrap_or(0),
            meta.tag().unwrap_or(""),
            content
        )
    }
}

// ============ Internal ============

#[derive(Error, Debug)]
enum ChunkError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Decrypt(#[from] DecryptionError),
    #[error(transparent)]
    Decompress(#[from] DecompressionError),
    #[error(transparent)]
    Decode(#[from] DecodingError),
}

/// # Workflow
///
/// ```plain
/// ┌──────────────┐   ┌───────────┐   ┌────────────┐   ┌──────────┐   ┌────────────┐
/// │  Read Chunk  │──▶│  Decrypt  │──▶│ Decompress │──▶│  Decode  │──▶│  Callback  │
/// └──────────────┘   └───────────┘   └────────────┘   └──────────┘   └────────────┘
/// ```
struct Processor<F> {
    decompressor: ZstdDecompressor,
    secret_key: Option<SecretKey>,
    encryption_keys: HashMap<PublicKey, EncryptionKey>,
    parser: RecordParser<F>,
}

impl<F> Processor<F>
where
    F: FnMut(&Record) -> Result<(), io::Error>,
{
    #[inline]
    fn new(secret_key: Option<SecretKey>, parser: RecordParser<F>) -> Self {
        Self {
            decompressor: ZstdDecompressor::new(),
            secret_key,
            encryption_keys: HashMap::new(),
            parser,
        }
    }

    fn obtain_decryptor(
        &mut self,
        pub_key: PublicKey,
    ) -> Result<Option<AesDecryptor>, EncryptionError> {
        if pub_key == EMPTY_PUBLIC_KEY {
            // No encryption.
            Ok(None)
        } else if let Some(key) = self.encryption_keys.get(&pub_key) {
            // Hit cache.
            Ok(Some(AesDecryptor::new(key)))
        } else {
            // Negotiates the key.
            if let Some(secret_key) = self.secret_key.as_ref() {
                let key = ecdh_encryption_key(secret_key, &pub_key)?;
                let key = self.encryption_keys.entry(pub_key).or_insert(key);
                Ok(Some(AesDecryptor::new(key)))
            } else {
                Ok(None)
            }
        }
    }

    fn chunk_sink(
        &mut self,
        payload_len: usize,
        pub_key: PublicKey,
        time_range: RangeInclusive<DateTime>,
        writeback: bool,
    ) -> FnSink<impl FnMut(&[u8]) -> Result<(), Error> + '_, Error> {
        let mut read_len = 0;
        let mut decryptor = self.obtain_decryptor(pub_key);

        FnSink::new(move |bytes: &[u8]| {
            read_len += bytes.len();
            let reached_to_end = read_len == payload_len;

            let decryptor =
                decryptor.as_mut().map_err(|e| Error::Decrypt(e.clone(), time_range.clone()))?;

            let mut to_decompressor = FnSink::new(|bytes: &[u8]| {
                self.decompressor.decompress(
                    bytes,
                    &mut FnSink::new(|bytes: &[u8]| self.parser.parse_all(bytes)),
                )
            });

            // Because the data of the chunk written back is incomplete (the last encrypted block
            // is lost), padding is not required when decrypting.
            decryptor
                .decrypt(bytes, reached_to_end && !writeback, &mut to_decompressor)
                .map_err(|e| Error::from_chunk_error(e, time_range.clone()))?;

            if reached_to_end {
                self.parser.clear_buffer();
            }

            Ok(())
        })
    }
}

struct RecordParser<F> {
    callback: F,
    buffer: BytesBuf,
}

impl<F> RecordParser<F>
where
    F: FnMut(&Record) -> Result<(), io::Error>,
{
    #[inline]
    fn new(callback: F) -> Self {
        Self { callback, buffer: BytesBuf::with_capacity(BUFFER_LEN) }
    }

    #[inline]
    fn parse_all(&mut self, mut bytes: &[u8]) -> Result<(), ChunkError> {
        while !bytes.is_empty() {
            let len = self.parse(bytes)?;
            bytes = &bytes[len..];
        }
        Ok(())
    }

    fn parse(&mut self, bytes: &[u8]) -> Result<usize, ChunkError> {
        let len = self.buffer.buffer(bytes);
        let mut source = self.buffer.deref();
        let mut read_len = 0;

        let res = loop {
            if source.is_empty() {
                break Ok(());
            }
            match Record::decode(&mut source) {
                Ok(record) => {
                    read_len = self.buffer.len() - source.len();
                    if let Err(e) = (self.callback)(&record) {
                        break Err(e.into());
                    }
                }
                // Not necessarily an error, writer needs to continue reading bytes.
                Err(ref e) if matches!(e, DecodingError::UnexpectedEnd { .. }) => break Ok(()),
                Err(e) => break Err(e.into()),
            }
        };

        self.buffer.drain(read_len);
        res.map(|_| len)
    }

    fn clear_buffer(&mut self) {
        self.buffer.clear()
    }
}

impl Error {
    #[inline]
    fn can_continue_to_read_chunk(&self) -> bool {
        matches!(self, Self::Decrypt(..))
            || matches!(self, Self::Decompress(..))
            || matches!(self, Self::Decode(..))
    }

    #[inline]
    fn from_chunk_error(error: ChunkError, time_range: RangeInclusive<DateTime>) -> Self {
        use ChunkError::*;
        match error {
            Io(err) => Self::Io(err),
            Decrypt(err) => Self::Decrypt(err, time_range),
            Decompress(err) => Self::Decompress(err, time_range),
            Decode(err) => Self::Decode(err, time_range),
        }
    }
}

impl From<chunk::ReadError> for Error {
    #[inline]
    fn from(error: chunk::ReadError) -> Self {
        use chunk::ReadError::*;
        match error {
            Io(err) => Self::Io(err),
            Invalid => Self::FileInvalid,
            UnexpectedEnd => Self::FileIncomplete,
        }
    }
}
