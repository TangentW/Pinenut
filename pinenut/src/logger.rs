//! The `Logger` implementation.

use std::{
    io,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex},
};

use thiserror::Error;

use crate::{
    buffer::{self, Buffer, EitherMemory, Memory},
    chunk::Chunk,
    codec::{AccumulationEncoder, EncodingError},
    common,
    compress::{CompressOp, CompressionError, Compressor, ZstdCompressor},
    encrypt::{
        ecdh::{self, PublicKey, EMPTY_PUBLIC_KEY},
        AesEncryptor, EncryptOp, EncryptionError, Encryptor,
    },
    logfile::{self, Logfile},
    mmap::Mmap,
    runloop::{self, Handle as RunloopHandle, Runloop},
    ChunkError, Config, Domain, Record, RunloopError, TimeDimension, Tracker,
    MMAP_BUFFER_EXTENSION,
};

/// The error type for [`Logger`].
///
/// Errors occurred in the logger can be track by the specified [`Tracker`].
#[derive(Error, Debug)]
pub enum Error {
    #[error("encoding: {0}")]
    Encode(#[from] EncodingError),
    #[error("compression: {0}")]
    Compress(#[from] CompressionError),
    #[error("encryption: {0}")]
    Encrypt(#[from] EncryptionError),
    #[error("chunk: {0}")]
    Chunk(#[from] ChunkError),
    #[error("IO runloop: {0}")]
    IoRunloop(#[from] RunloopError),
    #[error("IO: {0}")]
    Io(#[from] io::Error),
}

/// The `Pinenut` logger.
pub struct Logger {
    inner: Mutex<LoggerInner>,
}

impl Logger {
    /// Constructs a new `Logger`.
    #[inline]
    pub fn new(domain: Domain, config: Config) -> Self {
        Self { inner: Mutex::new(LoggerInner::new_inner(domain, config)) }
    }

    /// Logs the record.
    ///
    /// The low-level IO operations are performed asynchronously.
    #[inline]
    pub fn log(&self, record: &Record) {
        self.inner.lock().unwrap().on(Operation::Input(record));
    }

    /// Flushes any buffered records asynchronously.
    ///
    /// The low-level IO operations are performed asynchronously.
    #[inline]
    pub fn flush(&self) {
        self.inner.lock().unwrap().on(Operation::Rotate);
    }

    /// Deletes the expired log files with lifetime (seconds).
    ///
    /// The low-level IO operations are performed asynchronously.
    #[inline]
    pub fn trim(&self, lifetime: u64) {
        self.inner.lock().unwrap().trim(lifetime);
    }

    /// Flushes then Shuts down the logger.
    ///
    /// All asynchronous IO operations will be waiting to complete.
    #[inline]
    pub fn shutdown(self) {
        let mut inner = self.inner.into_inner().unwrap();
        inner.on(Operation::Rotate);
        inner.shutdown();
    }
}

// ============ Internal ============

/// Represents the logger context.
struct Context {
    domain: Arc<Domain>,
    pub_key: PublicKey,
    rotation: TimeDimension,
    tracker: Option<Tracker>,
}

impl Context {
    #[inline]
    fn new(
        domain: Domain,
        pub_key: Option<PublicKey>,
        rotation: TimeDimension,
        tracker: Option<Tracker>,
    ) -> Self {
        Self {
            domain: Arc::new(domain),
            pub_key: pub_key.unwrap_or(EMPTY_PUBLIC_KEY),
            rotation,
            tracker,
        }
    }

    /// Determines whether the chunk needs to be rotated.
    #[inline]
    pub(crate) fn rotate_chunk<B>(&self, chunk: &Chunk<B>, new_record: &Record) -> bool
    where
        B: Deref<Target = [u8]>,
    {
        !self.chunk_dimension().check_match(chunk.start_datetime(), new_record.meta().datetime())
    }

    /// Determines whether the log file needs to be rotated.
    #[inline]
    pub(crate) fn rotate_file<B>(&self, logfile: &Logfile, new_chunk: &Chunk<B>) -> bool
    where
        B: Deref<Target = [u8]>,
    {
        !self.file_dimension().check_match(new_chunk.start_datetime(), logfile.datetime())
    }

    /// Time dimension for chunk rotation.
    #[inline]
    fn chunk_dimension(&self) -> TimeDimension {
        self.rotation
    }

    /// Time dimension for log file rotation.
    #[inline]
    fn file_dimension(&self) -> TimeDimension {
        match self.chunk_dimension() {
            TimeDimension::Minute => TimeDimension::Hour,
            TimeDimension::Hour => TimeDimension::Day,
            TimeDimension::Day => TimeDimension::Day,
        }
    }
}

/// Returns a closure that reports the error to tracker.
macro_rules! track {
    ($tracker:expr) => {{
        |err| {
            if let Some(ref tracker) = $tracker {
                tracker.track(err.into(), file!(), line!());
            }
        }
    }};
}

/// The `Core Logger` associated with the specified `Compressor`, `Encryptor` and
/// `Memory`.
///
/// The current version of `Pinenut` will use the `zstd` compression algorithm and
/// `AES` encryption algorithm to process the logs.
type LoggerInner = Core<Option<ZstdCompressor>, Option<AesEncryptor>, EitherMemory>;

impl LoggerInner {
    #[inline]
    pub fn new_inner(domain: Domain, config: Config) -> Self {
        let memory = Self::initialize_memory(&domain, &config);

        let keys =
            config.key.and_then(|k| ecdh::Keys::new(&k).map_err(track!(config.tracker)).ok());
        let encryptor = keys.as_ref().map(|k| AesEncryptor::new(&k.encryption_key));

        let compressor =
            ZstdCompressor::new(config.compression_level).map_err(track!(config.tracker)).ok();

        let context =
            Context::new(domain, keys.map(|k| k.public_key), config.rotation, config.tracker);

        Self::new(context, compressor, encryptor, memory)
    }

    fn initialize_memory(domain: &Domain, config: &Config) -> EitherMemory {
        config
            .use_mmap
            .then(|| {
                let path =
                    domain.directory.join(&domain.identifier).with_extension(MMAP_BUFFER_EXTENSION);
                Mmap::new(path, config.buffer_len).map(EitherMemory::Mmap)
            })
            .and_then(|mmap| mmap.map_err(track!(config.tracker)).ok())
            .unwrap_or_else(|| {
                let mut vec = Vec::with_capacity(config.buffer_len);
                #[allow(clippy::uninit_vec)]
                unsafe {
                    vec.set_len(config.buffer_len);
                }
                EitherMemory::Vec(vec)
            })
    }
}

/// Operation for `Core` and `Processor`.
#[derive(Clone, Copy)]
enum Operation<'a> {
    Input(&'a Record<'a>),
    Rotate,
    Writeback,
}

/// Represents the `Core Logger`.
struct Core<C, E, M> {
    context: Arc<Context>,
    processor: Processor<C, E>,
    buffer: Buffer<M>,
    io_runloop: Runloop<IoEvent>,
}

impl<C, E, M> Core<C, E, M>
where
    C: Compressor,
    E: Encryptor,
    M: Memory,
{
    fn new(context: Context, compressor: C, encryptor: E, memory: M) -> Self {
        let context = Arc::new(context);
        let processor = Processor::new(compressor, encryptor);

        let (input_buffer, output_buffer) = Self::initialize_buffer(memory, &context);
        let io_runloop = Io::new(Arc::clone(&context), output_buffer).run();

        let mut core = Self { context, processor, buffer: input_buffer, io_runloop };
        // Attempts to write previously unwritten chunk to the logfile.
        core.on(Operation::Writeback);

        core
    }

    fn initialize_buffer(memory: M, context: &Context) -> (Buffer<M>, Buffer<M>) {
        let (mut input, mut output) = buffer::initialize(memory);
        {
            let (mut input_chunk, mut output_chunk) =
                (Chunk::bind(input.handle()), Chunk::bind(output.handle()));

            // If either side of the buffer is invalid, both sides need to be initialized.
            // Due to the internal structure of the double buffer system, when the buffer length
            // configuration is changed, one chunk must be invalid.
            if !input_chunk.validate() || !output_chunk.validate() {
                let now = chrono::Utc::now();
                input_chunk.initialize(now, context.pub_key);
                output_chunk.initialize(now, context.pub_key);
            }
        }
        (input, output)
    }

    fn on(&mut self, operation: Operation) {
        let mut chunk = Chunk::bind(self.buffer.handle());

        let write_operation = match operation {
            Operation::Rotate => Some(operation),
            // Writes back if chunk payload is not empty.
            Operation::Writeback => (chunk.payload_len() > 0).then(|| {
                chunk.set_writeback();
                operation
            }),
            // Checks if rotation is required.
            Operation::Input(record) => (chunk.is_almost_full()
                || self.context.rotate_chunk(&chunk, record))
            .then_some(Operation::Rotate),
        };

        if let Some(write_operation) = write_operation {
            self.processor
                .process(write_operation, &mut chunk)
                .unwrap_or_else(track!(self.context.tracker));

            // If the length of the chunk is greater than 0, it means that there are bytes to be
            // written to the file, we need to switch the buffer and perform IO write operation,
            // otherwise we can reuse the chunk and not perform IO write operation.
            if chunk.payload_len() > 0 {
                // Switches the double buffering system then rebinds the chunk.
                drop(chunk);
                self.buffer.switch();
                chunk = Chunk::bind(self.buffer.handle());

                // Performs asynchronous file write IO operation.
                self.io_runloop
                    .on(IoEvent::WriteChunk)
                    .unwrap_or_else(track!(self.context.tracker));
            }

            // Re-initialize the chunk.
            let datetime = match operation {
                Operation::Input(record) => record.meta().datetime(),
                Operation::Rotate | Operation::Writeback => chrono::Utc::now(),
            };
            chunk.initialize(datetime, self.context.pub_key);
        }

        if let Operation::Input(record) = operation {
            self.processor
                .process(Operation::Input(record), &mut chunk)
                .unwrap_or_else(track!(self.context.tracker));
        }
    }

    #[inline]
    fn trim(&mut self, lifetime: u64) {
        self.io_runloop.on(IoEvent::Trim { lifetime }).unwrap_or_else(track!(self.context.tracker));
    }

    #[inline]
    fn shutdown(self) {
        self.io_runloop.on(IoEvent::Shutdown).unwrap_or_else(track!(self.context.tracker));
        _ = self.io_runloop.join();
    }
}

/// The Log processor. It processes the log record step by step.
///
/// # Workflow
///
/// ```plain
/// ┌──────────┐   ┌──────────┐   ┌───────────┐   ┌───────────────────────────┐
/// │  Encode  │──▶│ Compress │──▶│  Encrypt  │──▶│  Write to Chunk (Buffer)  │
/// └──────────┘   └──────────┘   └───────────┘   └───────────────────────────┘
/// ```
struct Processor<C, E> {
    encoder: AccumulationEncoder,
    compressor: C,
    encryptor: E,
}

impl<C, E> Processor<C, E>
where
    C: Compressor,
    E: Encryptor,
{
    /// Length of `encoder buffer`.
    ///
    /// A buffer of 256 bytes should be sufficient for encoding of a log.
    const ENCODER_BUFFER_LEN: usize = 256;

    #[inline]
    fn new(compressor: C, encryptor: E) -> Self {
        let encoder = AccumulationEncoder::new(Self::ENCODER_BUFFER_LEN);
        Self { encoder, compressor, encryptor }
    }

    fn process<B>(&mut self, operation: Operation, chunk: &mut Chunk<B>) -> Result<(), Error>
    where
        B: DerefMut<Target = [u8]>,
    {
        type FnSink<F> = common::FnSink<F, Error>;

        let mut to_chunk = FnSink::new(|bytes: &[u8]| chunk.write(bytes).map_err(Into::into));

        let mut to_encryptor = FnSink::new(|bytes: &[u8]| {
            self.encryptor.encrypt(EncryptOp::Input(bytes), &mut to_chunk)
        });

        let mut to_compressor = FnSink::new(|bytes: &[u8]| {
            self.compressor.compress(CompressOp::Input(bytes), &mut to_encryptor)
        });

        match operation {
            Operation::Input(record) => {
                self.encoder.encode(record, &mut to_compressor)?;
                self.compressor.compress(CompressOp::Flush, &mut to_encryptor)?;
                chunk.set_end_datetime(record.meta().datetime());
            }

            Operation::Rotate => {
                self.compressor.compress(CompressOp::End, &mut to_encryptor)?;
                self.encryptor.encrypt(EncryptOp::Flush, &mut to_chunk)?;
            }

            Operation::Writeback => { /* Do nothing on writeback. */ }
        }

        Ok(())
    }
}

/// The IO handler. It is responsible for all file IO interactions.
///
/// It implements the [`runloop::Handle`] trait so that it can invoke a runloop to
/// handle IO events asynchronously.
struct Io<M> {
    context: Arc<Context>,
    buffer: Buffer<M>,
    logfile: Option<Logfile>,
}

/// IO events that the [`Io`] handler can receive.
enum IoEvent {
    /// Writes chunk to log file.
    WriteChunk,
    /// Deletes the expired log files.
    Trim { lifetime: u64 },
    /// Shuts down the IO handler.
    Shutdown,
}

impl<M> Io<M>
where
    M: Memory,
{
    #[inline]
    fn new(context: Arc<Context>, buffer: Buffer<M>) -> Self {
        let mut io = Io { context, buffer, logfile: None };
        // Attempts to write previously unwritten chunk to the logfile.
        if Chunk::bind(io.buffer.handle()).payload_len() > 0 {
            io.write_chunk();
        }
        io
    }

    /// Writes chunk to log file.
    fn write_chunk(&mut self) {
        let mut chunk = Chunk::bind(self.buffer.handle());
        // The chunk is empty, there is no need to write to the logfile.
        if chunk.payload_len() == 0 {
            return;
        }

        self.logfile.take_if(|f| self.context.rotate_file(f, &chunk));

        let logfile = if let Some(logfile) = &mut self.logfile {
            logfile
        } else {
            self.logfile = Some(Logfile::new(
                Arc::clone(&self.context.domain),
                chunk.start_datetime(),
                logfile::Mode::Write,
            ));
            // SAFETY: a `None` variant for `logfile` would have been replaced by a `Some`
            // variant in the code above.
            unsafe { self.logfile.as_mut().unwrap_unchecked() }
        };

        logfile.write(&chunk).unwrap_or_else(track!(self.context.tracker));
        logfile.flush().unwrap_or_else(track!(self.context.tracker));

        // Sets the chunk length to 0 to indicate that the chunk has finished writing to the
        // logfile and will not be written again.
        chunk.clear();
    }

    /// Deletes the expired log files.
    #[inline]
    fn trim(&mut self, lifetime: u64) {
        let expires = chrono::Utc::now().timestamp().saturating_sub_unsigned(lifetime);

        if let Ok(logfiles) = Logfile::logfiles(&self.context.domain, logfile::Mode::Read)
            .map_err(track!(self.context.tracker))
        {
            logfiles
                .filter(|f| f.datetime().timestamp() < expires)
                .for_each(|file| file.delete().unwrap_or_else(track!(self.context.tracker)));
        }
    }
}

impl<M> RunloopHandle for Io<M>
where
    M: Memory,
{
    type Event = IoEvent;

    #[inline]
    fn handle(&mut self, event: Self::Event, context: &mut runloop::Context) {
        match event {
            IoEvent::WriteChunk => self.write_chunk(),
            IoEvent::Trim { lifetime } => self.trim(lifetime),
            IoEvent::Shutdown => context.stop(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        chunk, chunk::Chunk, codec::Decode, compress::ZstdCompressor, encrypt::AesEncryptor,
        logger, logger::Operation, Record, RecordBuilder,
    };

    #[test]
    fn test_processor() {
        type Processor = logger::Processor<Option<ZstdCompressor>, Option<AesEncryptor>>;
        let mut processor = Processor::new(None, None);

        let mut memory = Vec::<u8>::with_capacity(256);
        unsafe {
            memory.set_len(256);
        }

        fn test_process<'a>(
            processor: &mut Processor,
            memory: &mut Vec<u8>,
            contents: impl IntoIterator<Item = &'a str>,
        ) {
            let mut chunk = Chunk::bind(memory.as_mut_slice());
            chunk.initialize(chrono::Utc::now(), [0; 33]);

            let records = contents
                .into_iter()
                .map(|c| RecordBuilder::new().content(c).build())
                .collect::<Vec<_>>();

            for record in &records {
                processor.process(Operation::Input(&record), &mut chunk).unwrap();
            }
            processor.process(Operation::Rotate, &mut chunk).unwrap();

            let payload_len = chunk.payload_len();
            let mut payload = &memory[chunk::Header::LEN..chunk::Header::LEN + payload_len];

            for record in records {
                let new_record = Record::decode(&mut payload).unwrap();
                assert_eq!(record, new_record);
            }

            assert_eq!(payload.len(), 0);
        }

        test_process(&mut processor, &mut memory, []);
        test_process(&mut processor, &mut memory, ["Hello", "World"]);
    }
}
