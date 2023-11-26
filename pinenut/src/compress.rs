//! Compression & Decompression.

use thiserror::Error;

use crate::Sealed;

/// Errors that can be occurred during compression or decompression.
#[derive(Error, Clone, Debug)]
#[error("{message}")]
pub struct Error {
    /// Represents an error code from the underlying compression library (zstd, zlib,
    /// etc).
    code: usize,
    /// Represents an error descriptive message.
    message: String,
}

/// Errors that can be occurred during compression.
pub type CompressionError = Error;

/// Errors that can be occurred during decompression.
pub type DecompressionError = Error;

/// Operation of compression. Different values are used according to different flush
/// dimensions.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CompressOp<'a> {
    Input(&'a [u8]),
    Flush,
    End,
}

/// Represents a target for compressed data or decompressed data.
pub(crate) trait Sink = crate::Sink<Error>;

/// Represents a compressor that compresses data to its target (`Sink`).
pub(crate) trait Compressor: Sealed {
    fn compress<S>(&mut self, operation: CompressOp, sink: &mut S) -> Result<(), S::Error>
    where
        S: Sink;
}

/// Represents a decompressor that decompresses data to its target (`Sink`).
pub(crate) trait Decompressor: Sealed {
    fn decompress<S>(&mut self, input: &[u8], sink: &mut S) -> Result<(), S::Error>
    where
        S: Sink;
}

pub(crate) use zstd::{Compressor as ZstdCompressor, Decompressor as ZstdDecompressor};

/// `Comporessor` and `Decompressor` for the `Zstandard` compression algorithm.
pub(crate) mod zstd {
    use zstd_safe::{
        get_error_name, max_c_level, min_c_level, zstd_sys::ZSTD_EndDirective, CCtx, CParameter,
        DCtx, ErrorCode, InBuffer, OutBuffer,
    };

    use crate::{
        compress::{
            CompressOp, Compressor as CompressorTrait, Decompressor as DecompressorTrait, Error,
            Sink,
        },
        Sealed,
    };

    impl From<ErrorCode> for Error {
        #[inline]
        fn from(code: ErrorCode) -> Self {
            let message = get_error_name(code).to_string();
            Self { code, message }
        }
    }

    impl From<CompressOp<'_>> for ZSTD_EndDirective {
        #[inline]
        fn from(value: CompressOp) -> Self {
            match value {
                CompressOp::Input(_) => Self::ZSTD_e_continue,
                CompressOp::Flush => Self::ZSTD_e_flush,
                CompressOp::End => Self::ZSTD_e_end,
            }
        }
    }

    /// The `Zstandard` compressor.
    pub(crate) struct Compressor {
        context: CCtx<'static>,
        output_buffer: Vec<u8>,
    }

    impl Compressor {
        /// The default compression level for `Pinenut`.
        pub(crate) const DEFAULT_LEVEL: i32 = 10;

        /// Length of `output buffer`.
        ///
        /// An output buffer of 256 bytes should be sufficient for compression of a
        /// log.
        const BUFFER_LEN: usize = 256;

        /// Constructs a new `Compressor` with compression level.
        ///
        /// `zstd` supports compression levels from 1 up to 22, it also offers
        /// negative compression levels, which extend the range of speed vs.
        /// ratio preferences.
        ///
        /// As the `std`'s documentation says: The lower the level, the faster the
        /// speed (at the cost of compression).
        #[allow(clippy::uninit_vec)]
        pub(crate) fn new(level: i32) -> Result<Self, Error> {
            let mut context = CCtx::create();
            let level = level.min(max_c_level()).max(min_c_level());
            context.set_parameter(CParameter::CompressionLevel(level))?;

            let mut output_buffer = Vec::with_capacity(Self::BUFFER_LEN);
            // SAFETY: Here the length is guaranteed to be correct.
            unsafe {
                output_buffer.set_len(output_buffer.capacity());
            }

            Ok(Self { context, output_buffer })
        }
    }

    impl CompressorTrait for Compressor {
        fn compress<S>(&mut self, operation: CompressOp, sink: &mut S) -> Result<(), S::Error>
        where
            S: Sink,
        {
            let (bytes, is_input_oper) = match operation {
                CompressOp::Input(bytes) => (bytes, true),
                _ => (&[] as &[u8], false),
            };

            let mut input = InBuffer::around(bytes);
            loop {
                let mut output = OutBuffer::around(self.output_buffer.as_mut_slice());
                // Compress into the output buffer and write all of the output to the `Sink` so we
                // can reuse the buffer next iteration.
                let remaining = self
                    .context
                    .compress_stream2(&mut output, &mut input, operation.into())
                    .map_err(Error::from)?;
                if output.pos() > 0 {
                    sink.sink(output.as_slice())?;
                }

                // If we use `Input` we're finished when we've consumed all the input.
                // Otherwise (`Flush` or `End`), we're finished when zstd returns 0, which means its
                // consumed all the input and finished the frame.
                let finished =
                    if is_input_oper { input.pos == input.src.len() } else { remaining == 0 };
                if finished {
                    break Ok(());
                }
            }
        }
    }

    impl Sealed for Compressor {}

    /// The `Zstandard` compressor.
    pub(crate) struct Decompressor {
        context: DCtx<'static>,
        output_buffer: Vec<u8>,
    }

    impl Decompressor {
        /// Length of `output buffer`.
        ///
        /// Uses 1KB as the output buffer length for decompression.
        const BUFFER_LEN: usize = 1024;

        /// Constructs a new `Decompressor`.
        #[inline]
        #[allow(clippy::uninit_vec)]
        pub(crate) fn new() -> Decompressor {
            let mut output_buffer = Vec::with_capacity(Self::BUFFER_LEN);
            // SAFETY: Here the length is guaranteed to be correct.
            unsafe {
                output_buffer.set_len(output_buffer.capacity());
            }

            Self { context: DCtx::create(), output_buffer }
        }
    }

    impl DecompressorTrait for Decompressor {
        fn decompress<S>(&mut self, input: &[u8], sink: &mut S) -> Result<(), S::Error>
        where
            S: Sink,
        {
            let mut input = InBuffer::around(input);
            // Given a valid frame, `zstd` won't consume the last byte of the frame until it has
            // flushed all of the decompressed data of the frame. Therefore, we can just check if
            // input.pos < input.size.
            while input.pos < input.src.len() {
                let mut output = OutBuffer::around(self.output_buffer.as_mut_slice());
                self.context.decompress_stream(&mut output, &mut input).map_err(Error::from)?;
                if output.pos() > 0 {
                    sink.sink(output.as_slice())?;
                }
            }
            Ok(())
        }
    }

    impl Default for Decompressor {
        #[inline]
        fn default() -> Self {
            Self::new()
        }
    }

    impl Sealed for Decompressor {}
}

impl<T> Compressor for Option<T>
where
    T: Compressor,
{
    #[inline]
    fn compress<S>(&mut self, operation: CompressOp, sink: &mut S) -> Result<(), S::Error>
    where
        S: Sink,
    {
        match self {
            Some(compressor) => compressor.compress(operation, sink),
            // Just writes its all input to the sink directly.
            None => match operation {
                CompressOp::Input(bytes) => sink.sink(bytes),
                _ => Ok(()),
            },
        }
    }
}

impl<T> Decompressor for Option<T>
where
    T: Decompressor,
{
    #[inline]
    fn decompress<S>(&mut self, input: &[u8], sink: &mut S) -> Result<(), S::Error>
    where
        S: Sink,
    {
        match self {
            Some(decompressor) => decompressor.decompress(input, sink),
            // Just writes its all input to the sink directly.
            None => sink.sink(input),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::slice;

    use crate::compress::{CompressOp, Compressor, Decompressor, ZstdCompressor, ZstdDecompressor};

    fn zstd_compress(input: &[u8]) -> Vec<u8> {
        let mut compressor = ZstdCompressor::new(3).unwrap();
        let mut sink = Vec::new();
        compressor.compress(CompressOp::Input(input), &mut sink).unwrap();
        compressor.compress(CompressOp::End, &mut sink).unwrap();
        sink
    }

    fn zstd_compress_mul(input: &[u8]) -> Vec<u8> {
        let mut compressor = ZstdCompressor::new(3).unwrap();
        let mut sink = Vec::new();
        for byte in input {
            compressor.compress(CompressOp::Input(slice::from_ref(byte)), &mut sink).unwrap();
            compressor.compress(CompressOp::Flush, &mut sink).unwrap();
        }
        compressor.compress(CompressOp::End, &mut sink).unwrap();
        sink
    }

    fn zstd_decompress(input: &[u8]) -> Vec<u8> {
        let mut decompressor = ZstdDecompressor::new();
        let mut sink = Vec::new();
        let mut sink_mul = Vec::new();

        // One time.
        decompressor.decompress(input, &mut sink).unwrap();

        // Multiple times.
        for byte in input {
            decompressor.decompress(slice::from_ref(byte), &mut sink_mul).unwrap();
        }

        assert_eq!(sink, sink_mul);
        sink
    }

    #[test]
    fn test_zstd() {
        let data = b"Hello, I'm Tangent, nice to meet you.";
        assert_eq!(zstd_decompress(&zstd_compress(data)), data);
        assert_eq!(zstd_decompress(&zstd_compress_mul(data)), data);

        // Empty data.
        assert_eq!(zstd_decompress(&zstd_compress(&[])), &[]);
    }
}
