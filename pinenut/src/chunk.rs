//! The `Chunk` structure.
//!
//! `Chunk` is a storage unit in Pinenut. In double-buffering system, each side
//! of the buffer maps to an input or output chunk, and the Pinenut log file consists
//! of consecutive chunks. Different compression and encryption algorithms can be
//! selected for log processing between different chunks.
//!
//! # The underlying structure
//!
//! ```plain
//!     ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─   n   ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┐              
//!    ├──── 60 ────┬───────────── n - 60 ──────────────┐              
//!    ▼────────────▼───────────────────────────────────▼              
//! ┌──│   Header   │              Payload              │              
//! │  └────────────┴───────────────────────────────────┘              
//! │  ┌─────────┬───────────┬──────────┬─────────────┬──────────────┬──────────────┐
//! └─▶│  Magic  │  Version  │  Length  │  Writeback  │  Time Range  │  Public Key  │
//!    ▲─────────▲───────────▲──────────▲─────────────▲───┬──────────▲──────────────▲
//!    └─── 4 ───┴──── 2 ────┴─── 4 ────┴───── 1 ─────┴───┼─ 16 ─────┴───── 33 ─────┘
//!                                                       │     ┌─────────┬─────────┐
//!                                                       └────▶│  Start  │   End   │
//!                                                             ▲─────────▲─────────▲
//!                                                             └─── 8 ───┴─── 8 ───┘
//! ```

use std::{
    fmt::{Display, Formatter},
    mem,
    ops::{Deref, DerefMut},
};

use thiserror::Error;

use crate::{encrypt::ecdh::PublicKey, DateTime, Magic, FORMAT_VERSION};

/// Errors that can be occurred during chunk write operations.
#[derive(Error, Clone, Debug)]
pub enum Error {
    /// The chunk has overflowed, the input bytes are too large.
    #[error("chunk overflow")]
    Overflow,
}

/// Represents the `Chunk` structure.
pub(crate) struct Chunk<T>(T);

/// Represents the inclusive time range spanned by a chunk (`start..=end`).
#[repr(C)]
#[derive(Clone, Debug)]
pub(crate) struct TimeRange {
    start: [u8; 8],
    end: [u8; 8],
}

impl TimeRange {
    /// The start datetime of the chunk.
    #[inline]
    pub(crate) fn start(&self) -> DateTime {
        let timestamp = i64::from_le_bytes(self.start);
        // For chunk, time accuracy does not have to be down to nanoseconds.
        DateTime::from_timestamp(timestamp, 0).unwrap_or_default()
    }

    /// The end datetime of the chunk.
    #[inline]
    pub(crate) fn end(&self) -> DateTime {
        let timestamp = i64::from_le_bytes(self.end);
        // For chunk, time accuracy does not have to be down to nanoseconds.
        DateTime::from_timestamp(timestamp, 0).unwrap_or_default()
    }
}

impl Display for TimeRange {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} - {}", self.start(), self.end())
    }
}

/// Represents the header of the chunk.
#[repr(C)]
#[derive(Clone, Debug)]
pub(crate) struct Header {
    magic: [u8; 4],
    version: [u8; 2],
    length: [u8; 4],
    writeback: bool,
    time_range: TimeRange,
    pub_key: PublicKey,
}

impl Header {
    /// Length of a header in bytes. (8 bytes)
    pub(crate) const LEN: usize = mem::size_of::<Self>();

    /// It means: `Feed Cat Chunk`.
    const MAGIC: Magic = Magic::new(0xFEEDCA7C);

    /// Checks the correctness of the chunk.
    #[inline]
    pub(crate) fn validate(&self) -> bool {
        Self::MAGIC == self.magic.into()
    }

    /// The format version of the chunk.
    #[inline]
    pub(crate) fn version(&self) -> u16 {
        u16::from_le_bytes(self.version)
    }

    /// The length of the chunk payload.
    #[inline]
    pub(crate) fn payload_len(&self) -> usize {
        u32::from_le_bytes(self.length) as usize
    }

    /// Represents a chunk to be written back.
    #[inline]
    pub(crate) fn writeback(&self) -> bool {
        self.writeback
    }

    /// The time range spanned by the chunk.
    #[inline]
    pub(crate) fn time_range(&self) -> &TimeRange {
        &self.time_range
    }

    /// The ECDH public key associated with the chunk.
    #[inline]
    pub(crate) fn pub_key(&self) -> PublicKey {
        self.pub_key
    }

    /// Converts header to bytes representation.
    #[inline]
    pub(crate) fn bytes(self) -> [u8; Self::LEN] {
        // SAFETY: Here the length is guaranteed to be correct.
        unsafe { mem::transmute(self) }
    }
}

impl<T> Chunk<T>
where
    T: Deref<Target = [u8]>,
{
    #[inline]
    pub(crate) fn bind(inner: T) -> Self {
        // Check length and alignment.
        // The alignment of Header is `1`, so memory always conforms to this.
        debug_assert!(inner.len() >= Header::LEN, "the storage is too small");
        Self(inner)
    }

    /// Checks the correctness of the chunk.
    #[inline]
    pub(crate) fn validate(&self) -> bool {
        self.header().validate() && self.header().payload_len() <= self.capacity()
    }

    /// The start datetime of the chunk.
    #[inline]
    pub(crate) fn start_datetime(&self) -> DateTime {
        self.header().time_range().start()
    }

    /// The length of the chunk payload.
    #[inline]
    pub(crate) fn payload_len(&self) -> usize {
        self.header().payload_len()
    }

    /// Checks whether the chunk is almost full.
    pub(crate) fn is_almost_full(&self) -> bool {
        const RATIO: f64 = 0.8;
        self.payload_len() as f64 >= RATIO * self.capacity() as f64
    }

    /// The capacity of the chunk payload.
    #[inline]
    fn capacity(&self) -> usize {
        self.0.len() - Header::LEN
    }

    #[inline]
    fn header(&self) -> &Header {
        // SAFETY: The pointer to the inner is properly aligned for a `Header`. Also, it has
        // been verified at construction to ensure that there are no pointer out-of-bounds
        // issues here.
        unsafe {
            let ptr = self.0.as_ptr() as *const Header;
            &*ptr
        }
    }
}

impl<T> Chunk<T>
where
    T: DerefMut<Target = [u8]>,
{
    /// Initialize the chunk.
    #[inline]
    pub(crate) fn initialize(&mut self, datetime: DateTime, pub_key: PublicKey) {
        let header = self.header_mut();
        header.magic = Header::MAGIC.into();
        header.version = FORMAT_VERSION.to_le_bytes();
        header.length = 0u32.to_le_bytes();
        header.writeback = false;
        header.pub_key = pub_key;

        let datetime = datetime.timestamp().to_le_bytes();
        header.time_range = TimeRange { start: datetime, end: datetime };
    }

    /// Writes bytes to the payload of the chunk.
    #[inline]
    pub(crate) fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let old_len = self.payload_len();
        let new_len = old_len + bytes.len();

        // Checking for overflow
        if new_len > self.capacity() {
            return Err(Error::Overflow);
        }

        let payload = self.payload_mut();
        payload[old_len..new_len].copy_from_slice(bytes);
        self.set_payload_len(new_len);
        Ok(())
    }

    /// Sets the current chunk to be written back.
    #[inline]
    pub(crate) fn set_writeback(&mut self) {
        self.header_mut().writeback = true;
    }

    /// Sets the end datetime of the chunk.
    #[inline]
    pub(crate) fn set_end_datetime(&mut self, datetime: DateTime) {
        self.header_mut().time_range.end = datetime.timestamp().to_le_bytes();
    }

    /// Clears the payload of the chunk.
    #[inline]
    pub(crate) fn clear(&mut self) {
        self.set_payload_len(0);
    }

    #[inline]
    fn set_payload_len(&mut self, len: usize) {
        let len: u32 = len.try_into().expect("len is too large");
        self.header_mut().length = len.to_le_bytes();
    }

    #[inline]
    fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.0[Header::LEN..]
    }

    #[inline]
    fn header_mut(&mut self) -> &mut Header {
        // SAFETY: The pointer to the inner is properly aligned for a `Header`. Also, it has
        // been verified at construction to ensure that there are no pointer out-of-bounds
        // issues here.
        unsafe {
            let ptr = self.0.as_mut_ptr() as *mut Header;
            &mut *ptr
        }
    }
}

impl<T> Deref for Chunk<T>
where
    T: Deref<Target = [u8]>,
{
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        let len = self.payload_len().min(self.capacity()) + Header::LEN;
        &self.0[..len]
    }
}

pub(crate) use reader::{Error as ReadError, Reader};

/// The internal module that implements the chunk reader.
pub(crate) mod reader {
    use std::io;

    use thiserror::Error;

    use crate::{
        chunk::Header,
        common::{BytesBuf, Sink},
        BUFFER_LEN,
    };

    /// Errors that can be occurred during chunk read operations.
    #[derive(Error, Debug)]
    pub(crate) enum Error {
        #[error("invalid chunk")]
        Invalid,
        #[error(transparent)]
        Io(#[from] io::Error),
        #[error("unexpected end of bytes")]
        UnexpectedEnd,
    }

    /// Represents a reader that reads chunks from the underlying [`io::Read`].
    ///
    /// To read chunk, the methods are called in the following order:
    /// * [`Reader::read_header_or_end`]
    /// * [`Reader::read_payload`]
    ///
    /// These methods are called in a loop until either an error occurs or the
    /// underlying reader finishes reading.
    pub(crate) struct Reader<R> {
        inner: R,
        buffer: BytesBuf,
    }

    impl<R> Reader<R>
    where
        R: io::Read + io::Seek,
    {
        /// Construct a new `Reader`.
        #[inline]
        pub(crate) fn new(inner: R) -> Self {
            Self { inner, buffer: BytesBuf::with_capacity(BUFFER_LEN) }
        }

        /// Reads the head of the chunk. If the underlying reader has reached the
        /// end, returns `None`.
        pub(crate) fn read_header_or_reach_to_end(&mut self) -> Result<Option<&Header>, Error> {
            let buffer = self.buffer.as_buffer_mut_slice();
            assert!(buffer.len() >= Header::LEN, "buffer is too small");

            let mut read_len = 0;
            while read_len < Header::LEN {
                match self.inner.read(&mut buffer[read_len..Header::LEN]) {
                    Ok(0) => {
                        return if read_len == 0 { Ok(None) } else { Err(Error::UnexpectedEnd) }
                    }
                    Ok(len) => read_len += len,
                    Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(e.into()),
                }
            }

            // SAFETY: Here the length is guaranteed to be correct. The alignment of Header is
            // `1`, so memory always conforms to this.
            let header: &Header = unsafe {
                let ptr = buffer.as_ptr() as *const Header;
                &*ptr
            };

            if header.validate() {
                Ok(Some(header))
            } else {
                Err(Error::Invalid)
            }
        }

        /// Reads the payload of the chunk with payload length.
        pub(crate) fn read_payload<S>(&mut self, len: usize, sink: &mut S) -> Result<(), S::Error>
        where
            S: Sink<Error>,
        {
            let buffer = self.buffer.as_buffer_mut_slice();
            let mut remaining = len;

            while remaining > 0 {
                let capacity = buffer.len().min(remaining);
                match self.inner.read(&mut buffer[..capacity]) {
                    Ok(0) => return Err(Error::UnexpectedEnd.into()),
                    Ok(len) => {
                        remaining -= len;
                        sink.sink(&buffer[..len]).inspect_err(|_| {
                            // Reader needs to ensure that it has pointed to the next chunk to be
                            // read.
                            _ = self.inner.seek(io::SeekFrom::Current(
                                remaining.try_into().expect("payload is too large"),
                            ));
                        })?;
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(Error::Io(e).into()),
                }
            }

            Ok(())
        }

        /// Skips the current payload with payload length.
        #[inline]
        pub(crate) fn skip(&mut self, len: usize) -> Result<(), Error> {
            let len: i64 = len.try_into().expect("chunk is too large");
            self.inner.seek(io::SeekFrom::Current(len))?;
            Ok(())
        }
    }
}
