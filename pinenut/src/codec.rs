//! Encoding & Decoding.

use std::{any::type_name, slice, str};

use thiserror::Error;

use crate::{
    common::{BytesBuf, FnSink},
    DateTime, Level,
};

/// Errors that can be occurred by encoding a type.
#[non_exhaustive]
#[derive(Error, Clone, Debug)]
pub enum EncodingError {
    /// No errors yet.
    #[allow(dead_code)]
    #[error("unreachable")]
    None,
}

/// Represents a target for encoded data.
pub(crate) trait Sink = crate::Sink<EncodingError>;

/// Any data type that can be encoded.
///
/// `Pinenut` encodes the data into a stream of compact binary bytes and outputs to
/// the `Sink`.
///
/// This trait will be automatically implemented if you add `#[derive(Encode)]` to a
/// struct.
pub(crate) trait Encode {
    /// Encode the data and write encoded bytes to `Sink`.
    fn encode<S>(&self, sink: &mut S) -> Result<(), S::Error>
    where
        S: Sink;
}

/// Errors that can be occurred by decoding a type.
#[derive(Error, Clone, Debug)]
#[non_exhaustive]
pub enum DecodingError {
    /// The source reached its end but more bytes were expected.
    #[error("the source reached its end, but more bytes ({extra_len}) were expected")]
    UnexpectedEnd {
        /// How many extra bytes are needed.
        extra_len: usize,
    },
    /// Invalid variant was found. This error is generally for enums.
    #[error("invalid variant ({found_byte}) was found on type `{type_name}`")]
    UnexpectedVariant {
        /// The type name that was being decoded.
        type_name: &'static str,
        /// The byte that has been read.
        found_byte: u8,
    },
    /// Which can be occurred when attempting to decode bytes as a `str`, it is
    /// essentially an UTF-8 error.
    #[error(transparent)]
    Str(#[from] str::Utf8Error),
    /// The encoded varint is outside of the range of the target integral type.
    ///
    /// This may happen if an usize was encoded on 64-bit architecture and then
    /// decoded on 32-bit architecture (from large type to small type).
    #[error("the encoded varint is outside of the range of the target integral type")]
    IntegerOverflow,
    /// Which can be occurred on out-of-range number of seconds and/or invalid
    /// nanosecond.
    #[error("failed to decode date & time")]
    DateTime,
}

/// Represents a provider for encoded data.
pub(crate) trait Source<'de> {
    type Error: From<DecodingError>;

    /// Take a length and attempt to read that many bytes.
    fn read_bytes(&mut self, len: usize) -> Result<&'de [u8], Self::Error>;
}

/// Any data type that can be decoded.
///
/// `Pinenut` decodes the data by continuously reading a stream of compact binary
/// bytes from the `Source`.
///
/// The `'de` lifetime is what enables `Pinenut` to safely perform efficient
/// zero-copy decoding across a variety of data formats.
///
/// This trait will be automatically implemented if you add `#[derive(Decode)]` to a
/// struct.
pub(crate) trait Decode<'de>: Sized {
    /// Decode the data from `Source`.
    fn decode<S>(source: &mut S) -> Result<Self, S::Error>
    where
        S: Source<'de>;
}

/// Used to accumulate the data generated during encoding and reduce the callback
/// frequency.
///
/// In order to reduce the frequency of calling the Sink, a buffer is used to
/// temporarily store data. When the buffer is full, it will be flushed to the Sink,
/// otherwise it will continue to wait for the buffer to be filled.
pub(crate) struct AccumulationEncoder {
    buffer: BytesBuf,
}

impl AccumulationEncoder {
    /// Constructs a new `AccumulationEncoder`.
    #[inline]
    pub(crate) fn new(buffer_len: usize) -> Self {
        Self { buffer: BytesBuf::with_capacity(buffer_len) }
    }

    /// Encode the data and write encoded bytes to `Sink`.
    pub(crate) fn encode<T, S>(&mut self, value: &T, sink: &mut S) -> Result<(), S::Error>
    where
        T: Encode,
        S: Sink,
    {
        value.encode(&mut FnSink::new(|mut bytes: &[u8]| {
            loop {
                let buffered = self.buffer.buffer(bytes);
                bytes = &bytes[buffered..];

                // The buffer is not full.
                if bytes.is_empty() {
                    break Ok(());
                }

                // The buffer is full, flushes it into Sink.
                let result = sink.sink(&self.buffer);
                if result.is_err() {
                    break result;
                }

                // Keeps waiting for the data to fill in.
                self.buffer.clear();
            }
        }))?;

        // Flushes the buffer into the Sink.
        sink.sink(&self.buffer)?;
        self.buffer.clear();

        Ok(())
    }
}

// ============ Implementations ============

impl Encode for u8 {
    #[inline]
    fn encode<S>(&self, sink: &mut S) -> Result<(), S::Error>
    where
        S: Sink,
    {
        sink.sink(slice::from_ref(self))
    }
}

impl<'de> Decode<'de> for u8 {
    #[inline]
    fn decode<S>(source: &mut S) -> Result<Self, S::Error>
    where
        S: Source<'de>,
    {
        let bytes = source.read_bytes(1)?;
        // `source` is responsible for errors handling, so use `unwrap` directly here.
        Ok(*bytes.first().unwrap())
    }
}

/// Implements `Encode` and `Decode` traits for specified integral type, using
/// `varint` (variable length integer) encoding.
///
/// Currently, encoding negative integers is not supported. `ZigZag` encoding may be
/// used in the future.
macro_rules! integral_type_codec_impl {
    ($Self:ty) => {
        integral_type_codec_impl!(encode: $Self);
        integral_type_codec_impl!(decode: $Self);
    };

    (encode: $Self:ty) => {
        impl Encode for $Self {
            fn encode<S>(&self, sink: &mut S) -> Result<(), S::Error>
            where
                S: Sink,
            {
                let mut val = *self;
                loop {
                    if val <= 0x7F {
                        (val as u8).encode(sink)?;
                        break Ok(());
                    }
                    ((val & 0x7F) as u8 | 0x80).encode(sink)?;
                    val >>= 7;
                }
            }
        }
    };

    (decode: $Self:ty) => {
        impl<'de> Decode<'de> for $Self {
            fn decode<S>(source: &mut S) -> Result<Self, S::Error>
            where
                S: Source<'de>,
            {
                let (mut val, mut shift) = (0, 0);
                loop {
                    let byte = u8::decode(source)?;
                    let high_bits = byte as $Self & 0x7F;
                    // Check for overflow.
                    if high_bits.leading_zeros() < shift {
                        break Err(DecodingError::IntegerOverflow.into());
                    }
                    val |= high_bits << shift;
                    if byte & 0x80 == 0 {
                        break Ok(val);
                    }
                    shift += 7;
                }
            }
        }
    };
}

integral_type_codec_impl!(u32);
integral_type_codec_impl!(u64);
integral_type_codec_impl!(usize);

impl<const N: usize> Encode for &[u8; N] {
    #[inline]
    fn encode<S>(&self, sink: &mut S) -> Result<(), S::Error>
    where
        S: Sink,
    {
        self.as_slice().encode(sink)
    }
}

impl<'de: 'a, 'a, const N: usize> Decode<'de> for &'a [u8; N] {
    #[inline]
    fn decode<S>(source: &mut S) -> Result<Self, S::Error>
    where
        S: Source<'de>,
    {
        let bytes = source.read_bytes(N)?;
        // `source` is responsible for errors handling, so use `unwrap` directly here.
        Ok(bytes.try_into().unwrap())
    }
}

impl Encode for &[u8] {
    #[inline]
    fn encode<S>(&self, sink: &mut S) -> Result<(), S::Error>
    where
        S: Sink,
    {
        // Encode the length first, then the payload.
        self.len().encode(sink)?;
        sink.sink(self)
    }
}

impl<'de: 'a, 'a> Decode<'de> for &'a [u8] {
    #[inline]
    fn decode<S>(source: &mut S) -> Result<Self, S::Error>
    where
        S: Source<'de>,
    {
        // Decode the length first, then read bytes of length.
        let len = usize::decode(source)?;
        source.read_bytes(len)
    }
}

// `&[u8]` is also a `Source`.
impl<'a> Source<'a> for &'a [u8] {
    type Error = DecodingError;

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], Self::Error> {
        if self.len() >= len {
            let (bytes, remaining) = self.split_at(len);
            *self = remaining;
            Ok(bytes)
        } else {
            Err(DecodingError::UnexpectedEnd { extra_len: len - self.len() })
        }
    }
}

impl Encode for &str {
    #[inline]
    fn encode<S>(&self, sink: &mut S) -> Result<(), S::Error>
    where
        S: Sink,
    {
        self.as_bytes().encode(sink)
    }
}

impl<'de: 'a, 'a> Decode<'de> for &'a str {
    #[inline]
    fn decode<S>(source: &mut S) -> Result<Self, S::Error>
    where
        S: Source<'de>,
    {
        let bytes = Decode::decode(source)?;
        str::from_utf8(bytes).map_err(|e| DecodingError::Str(e).into())
    }
}

const OPTION_NONE_TAG: u8 = 0;
const OPTION_SOME_TAG: u8 = 1;

impl<T> Encode for Option<T>
where
    T: Encode,
{
    #[inline]
    fn encode<S>(&self, sink: &mut S) -> Result<(), S::Error>
    where
        S: Sink,
    {
        // Encode the tag first, then the payload if there is one.
        match self {
            None => OPTION_NONE_TAG.encode(sink),
            Some(inner) => {
                OPTION_SOME_TAG.encode(sink)?;
                inner.encode(sink)
            }
        }
    }
}

impl<'de, T> Decode<'de> for Option<T>
where
    T: Decode<'de>,
{
    fn decode<S>(source: &mut S) -> Result<Self, S::Error>
    where
        S: Source<'de>,
    {
        // Decode the tag first, then the payload if the tag is `Some`.
        let tag = Decode::decode(source)?;
        match tag {
            OPTION_NONE_TAG => Ok(None),
            OPTION_SOME_TAG => Decode::decode(source).map(Some),
            _ => Err(DecodingError::UnexpectedVariant {
                type_name: type_name::<Self>(),
                found_byte: tag,
            }
            .into()),
        }
    }
}

impl Encode for Level {
    #[inline]
    fn encode<S>(&self, sink: &mut S) -> Result<(), S::Error>
    where
        S: Sink,
    {
        self.primitive().encode(sink)
    }
}

impl<'de> Decode<'de> for Level {
    #[inline]
    fn decode<S>(source: &mut S) -> Result<Self, S::Error>
    where
        S: Source<'de>,
    {
        let primitive = Decode::decode(source)?;
        if let Some(level) = Level::from_primitive(primitive) {
            Ok(level)
        } else {
            Err(DecodingError::UnexpectedVariant {
                type_name: type_name::<Self>(),
                found_byte: primitive,
            }
            .into())
        }
    }
}

impl Encode for DateTime {
    #[inline]
    fn encode<S>(&self, sink: &mut S) -> Result<(), S::Error>
    where
        S: Sink,
    {
        // Encode `secs`. It can't be earlier than the midnight on January 1, 1970.
        self.timestamp().try_into().unwrap_or(0u64).encode(sink)?;
        // Encode `nsecs`.
        self.timestamp_subsec_nanos().encode(sink)
    }
}

impl<'de> Decode<'de> for DateTime {
    #[inline]
    fn decode<S>(source: &mut S) -> Result<Self, S::Error>
    where
        S: Source<'de>,
    {
        // Decode `secs`.
        let secs = u64::decode(source)?.try_into().map_err(|_| DecodingError::IntegerOverflow)?;
        // Decode `nsecs`.
        let nsecs = u32::decode(source)?;
        // Make date & time.
        DateTime::from_timestamp(secs, nsecs).ok_or(DecodingError::DateTime.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        codec::{Decode, DecodingError, Encode},
        DateTime,
    };

    /// Codec testing helper.
    ///
    /// It takes two arguments (type, value) and returns the encoded bytes.
    macro_rules! test_coding {
        ($ty:ty, $val:expr) => {{
            let mut sink = Vec::new();

            let val: $ty = $val;
            val.encode(&mut sink).unwrap();

            let mut source = sink.as_slice();
            assert_eq!(<$ty>::decode(&mut source).unwrap(), $val);
            assert!(source.is_empty());

            sink
        }};
    }

    #[test]
    fn test_integer() {
        assert_eq!(test_coding!(u32, 0x7F), [0x7F]);
        assert_eq!(test_coding!(u64, 0x80), [0x80, 0x01]);
        assert_eq!(test_coding!(u64, 0xC0C0C0C0), [0xC0, 0x81, 0x83, 0x86, 0x0C]);
        // Test for overflow.
        let sink = test_coding!(u64, u32::MAX as u64 + 1);
        assert_eq!(sink, [0x80, 0x80, 0x80, 0x80, 0x10]);
        let mut source = sink.as_slice();
        assert!(matches!(u32::decode(&mut source), Err(DecodingError::IntegerOverflow)));
    }

    #[test]
    fn test_option() {
        assert_eq!(test_coding!(Option<u8>, None), [0x00]);
        assert_eq!(test_coding!(Option<u8>, Some(0xFF)), [0x01, 0xFF]);
    }

    #[test]
    fn test_str() {
        assert_eq!(test_coding!(&str, ""), [0x00]);
        assert_eq!(
            test_coding!(&str, "Hello World"),
            [0x0B, 0x48, 0x65, 0x6C, 0x6C, 0x6F, 0x20, 0x57, 0x6F, 0x72, 0x6C, 0x64]
        );
    }

    #[test]
    fn test_datetime() {
        let datetime = chrono::Utc::now();
        test_coding!(DateTime, datetime);
    }
}
