//! An extremely high performance logging system for clients (iOS, Android, Desktop),
//! written in Rust.
//!
//! ### Compression
//!
//! Pinenut supports streaming log compression, it uses the `Zstandard (aka zstd)`, a
//! high performance compression algorithm that has a good balance between
//! compression rate and speed.
//!
//! ### Encryption
//!
//! Pinenut uses the `AES 128` algorithm for symmetric encryption during logging. To
//! prevent embedding the symmetric key directly into the code, Pinenut uses `ECDH`
//! for key negotiation (RSA is not used  because its key are too long). When
//! initializing the Logger, there is no need to provide the symmetric encryption
//! key, instead the ECDH public key should be passed.
//!
//! Pinenut uses `secp256r1` elliptic curve for ECDH. You can generate the secret and
//! public keys for encryption yourself, or use Pinenut's built-in command line tool:
//! `pinenut-cli`.
//!
//! ### Buffering
//!
//! In order to minimize IO frequency, Pinenut buffers the log data before writing to
//! the file. Client programs may exit unexpectedly (e.g., crash), Pinenut uses
//! `mmap` as buffer support, so that if the program unexpectedly exits, the OS can
//! still help to persist the buffered data. The next time the Logger is initialized,
//! the buffered data is automatically read and written back to the log file.
//!
//! In addition, Pinenut implements a `double-buffering` system to improve buffer
//! read/write performance and prevent asynchronous IOs from affecting logging of the
//! current thread.
//!
//! ### Extraction
//!
//! With Pinenut, we don't need to retrieve all the log files in the directory to
//! extract logs, it provides convenient extraction capabilities and supports
//! extraction in time ranges with minute granularity.
//!
//! ### Parsing
//!
//! The content of Pinenut log files is a special binary sequence after encoding,
//! compression and encryption, and we can parse the log files using the parsing
//! capabilities provided by Pinenut.
//!
//! ## Usage
//!
//! Pinenut's APIs are generally similar regardless of the language used.
//!
//! ### Logger Initialization
//!
//! Pinenut uses a `Logger` instance for logging. Before we initialize the Logger, we
//! need to pass in the logger identifier and the path to the directory where the log
//! files are stored to construct the `Domain` structure.
//!
//! We can customize the Logger by explicitly specifying `Config`, see the API
//! documentation for details.
//!
//! ```rust,no_run
//! # use pinenut_log::{Domain, Config, Logger};
//! let domain = Domain::new("MyApp".into(), "/path/to/dir".into());
//! let config = Config::new().key_str(Some("Public Key Base64")).compression_level(10);
//! let logger = Logger::new(domain, config);
//! ```
//!
//! ### Logging
//!
//! Just construct the `Record` and call the `log` method.
//!
//! Records can be constructed in `Rust` via the Builder pattern:
//!
//! ```rust,no_run
//! # use pinenut_log::{Meta, Level, Record, Domain};
//! # let logger = Domain::new("".into(), "".into()).logger_with_default_config();
//! // Builds `Meta` & `Record`.
//! let meta = Meta::builder().level(Level::Info).build();
//! let record = Record::builder().meta(meta).content("Hello World").build();
//! logger.log(&record);
//!
//! // Flushes any buffered records asynchronously.
//! logger.flush();
//! ```
//!
//! See the API documentation for details.
//!
//! ### Extraction
//!
//! Just call the `extract` method to extract the logs for the specified time range
//! (with minute granularity) and write them to the destination file.
//!
//! ```rust,no_run
//! # use std::ops::Sub;
//! # use std::time::Duration;
//! # use pinenut_log::Domain;
//! let domain = Domain::new("MyApp".into(), "/path/to/dir".into());
//! let now = chrono::Utc::now();
//! let range = now.sub(Duration::from_secs(1800))..=now;
//!
//! if let Err(err) = pinenut_log::extract(domain, range, "/path/to/destination") {
//!     println!("Error: {err}");
//! }
//! ```
//!
//!
//! Note: The content of the extracted file is still a binary sequence that has been
//! encoded, compressed, and encrypted. We need to parse it to see the log text
//! content that is easy to read.
//!
//! ### Parsing
//!
//! You can use the `parse` function for log parsing, **and you can specify the
//! format of the log parsed text**. See the API documentation for details.
//!
//!
//! ```rust,no_run
//! // Specifies the `DefaultFormater` as the log formatter.
//! # use pinenut_log::DefaultFormatter;
//! # let (path, output) = ("", "");
//! # let secret_key = None;    
//! if let Err(err) = pinenut_log::parse_to_file(&path, &output, secret_key, DefaultFormatter) {
//!     println!("Error: {err}");
//! }
//! ```
//!
//! Or use the built-in command line tool `pinenut-cli`:
//!
//! ```plain
//! $ pinenut-cli parse ./my_log.pine \
//!     --output ./plain.log          \
//!     --secret-key XXXXXXXXXXX
//! ```
//!
//! ### Keys Generation
//!
//! Before initializing the Logger or parsing the logs, you need to have the public
//! and secret keys ready (The public key is used to initialize the Logger and the
//! secret key is used to parse the logs).
//!
//! You can use `pinenut-cli` to generate this pair of keys:
//!
//! ``` plain
//! $ pinenut-cli gen-keys
//! ```

#![feature(trait_alias)]
#![feature(let_chains)]
#![feature(option_take_if)]

use std::path::PathBuf;

use base64::{prelude::BASE64_STANDARD, Engine};
use chrono::Timelike;

use crate::compress::ZstdCompressor;

pub mod record;
pub use record::*;

pub mod compress;
pub use compress::{CompressionError, DecompressionError};

pub mod encrypt;
pub use encrypt::{
    DecryptionError, EncryptionError, EncryptionKey, PublicKey, SecretKey, PUBLIC_KEY_LEN,
};

pub mod codec;
pub use codec::{DecodingError, EncodingError};

pub mod chunk;
pub use chunk::Error as ChunkError;

pub mod runloop;
pub use runloop::Error as RunloopError;

mod logger;
pub use logger::{Error as LoggerError, Logger};

mod extract;
pub use extract::{extract, Error as ExtractionError};

mod parse;
pub use parse::{parse, parse_to_file, DefaultFormatter, Error as ParsingError, Format};

mod common;
use common::*;

mod buffer;
mod logfile;
mod mmap;

/// The current format version of the Pinenut log structure.
///
/// The current version of Pinenut will use the `zstd` compression algorithm and
/// `AES` encryption algorithm to process the logs.
pub const FORMAT_VERSION: u16 = 1;

/// The extension of the Pinenut mmap buffer file.
pub const MMAP_BUFFER_EXTENSION: &str = "pinebuf";

/// The extension of the Pinenut log file.
pub const FILE_EXTENSION: &str = "pine";

/// The extension of the Pinenut plain log file.
pub const PLAIN_FILE_EXTENSION: &str = "log";

/// The default buffer length (320 KB) for Pinenut.
pub const BUFFER_LEN: usize = 320 * 1024;

/// Represents the domain to which the logger belongs, the logs will be organized by
/// domain.
#[derive(Clone, Debug)]
pub struct Domain {
    /// Used to identity a specific domain for logger.
    pub identifier: String,
    /// Used to specify the directory where the log files for this domian are stored.
    pub directory: PathBuf,
}

impl Domain {
    /// Constructs a new `Domain`.
    #[inline]
    pub fn new(identifier: String, directory: PathBuf) -> Self {
        Self { identifier, directory }
    }

    /// Obtains a logger with a specified configuration.
    #[inline]
    pub fn logger(self, config: Config) -> Logger {
        Logger::new(self, config)
    }

    /// Obtains a logger with the default configuration.
    #[inline]
    pub fn logger_with_default_config(self) -> Logger {
        self.logger(Config::default())
    }
}

/// Represents the dimension of datetime, used for log rotation.
#[repr(u8)]
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum TimeDimension {
    Day = 1,
    Hour,
    Minute,
}

impl TimeDimension {
    /// Checks whether two datetimes match on a specified dimension.
    fn check_match(self, left: DateTime, right: DateTime) -> bool {
        let (left, right) = (left.naive_local(), right.naive_local());
        let mut is_matched = true;

        if self >= Self::Day {
            is_matched &= left.date() == right.date();
        }
        if self >= Self::Hour {
            is_matched &= left.hour() == right.hour();
        }
        if self >= Self::Minute {
            is_matched &= left.minute() == right.minute();
        }

        is_matched
    }
}

/// Represents a tracker used to track errors occurred from the logger operations.
pub trait Track {
    /// Handles the error on the code location.
    fn track(&self, error: LoggerError, file: &'static str, line: u32);
}

impl<F> Track for F
where
    F: Fn(LoggerError, &'static str, u32),
{
    #[inline]
    fn track(&self, error: LoggerError, file: &'static str, line: u32) {
        self(error, file, line)
    }
}

/// Trait object type for [`Track`].
pub type Tracker = Box<dyn Track + Send + Sync>;

/// Configuration of a logger instance.
pub struct Config {
    use_mmap: bool,
    buffer_len: usize,
    rotation: TimeDimension,
    key: Option<PublicKey>,
    compression_level: i32,
    tracker: Option<Tracker>,
}

impl Config {
    /// Constructs a new `Config`.
    #[inline]
    pub fn new() -> Self {
        Default::default()
    }

    /// Whether or not to use `mmap` as the underlying storage for the buffer.
    ///
    /// With mmap, if the application terminates unexpectedly, the log data in the
    /// buffer is written to the mmap buffer file by the OS at a certain time, and
    /// then when the logger is restarted, the log data is written back to the log
    /// file, avoiding loss of log data.
    ///
    /// It is enabled by default.
    #[inline]
    pub fn use_mmap(mut self, flag: bool) -> Self {
        self.use_mmap = flag;
        self
    }

    /// The buffer length.
    ///
    /// If mmap is used, it is rounded up to a multiple of pagesize.
    /// Pinenut uses a double cache system, so the buffer that is actually written
    /// to will be less than half of this.
    ///
    /// The default value is `320 KB`.
    #[inline]
    pub fn buffer_len(mut self, len: usize) -> Self {
        self.buffer_len = len;
        self
    }

    /// Time granularity of log extraction.
    ///
    /// The default value is `Minute`.
    #[inline]
    pub fn rotation(mut self, rotation: TimeDimension) -> Self {
        self.rotation = rotation;
        self
    }

    /// The encryption key, the public key in ECDH.
    ///
    /// It is used to negotiate the key for symmetric encryption of the log.
    /// If the value is `None`, there is no encryption.
    ///
    /// The default value is `None`.
    #[inline]
    pub fn key(mut self, key: Option<PublicKey>) -> Self {
        self.key = key;
        self
    }

    /// The encryption key, the public key in ECDH, represented in `Base64`.
    ///
    /// It is used to negotiate the key for symmetric encryption of the log.
    /// If the value is `None` or invalid, there is no encryption.
    ///
    /// The default value is `None`.
    #[inline]
    pub fn key_str(self, key: Option<impl AsRef<[u8]>>) -> Self {
        let key = key.and_then(|k| BASE64_STANDARD.decode(k).ok()).and_then(|k| k.try_into().ok());
        self.key(key)
    }

    /// The compression level.
    ///
    /// Pinenut uses `zstd` as the compression algorithm, which supports compression
    /// levels from 1 up to 22, it also offers negative compression levels.
    ///
    /// As the `std`'s documentation says: The lower the level, the faster the
    /// speed (at the cost of compression).
    ///
    /// The default value is `10`.
    #[inline]
    pub fn compression_level(mut self, level: i32) -> Self {
        self.compression_level = level;
        self
    }

    /// The tracker used to track errors occurred from the logger operations.
    ///
    /// Errors are printed to standard output by default.
    #[inline]
    pub fn tracker(mut self, tracker: Option<Tracker>) -> Self {
        self.tracker = tracker;
        self
    }

    /// Obtains a logger with a specified domain.
    #[inline]
    pub fn logger(self, domain: Domain) -> Logger {
        Logger::new(domain, self)
    }
}

impl Default for Config {
    #[inline]
    fn default() -> Self {
        Self {
            use_mmap: true,
            buffer_len: BUFFER_LEN,
            rotation: TimeDimension::Minute,
            key: None,
            compression_level: ZstdCompressor::DEFAULT_LEVEL,
            tracker: Some(Box::new(|err, file, line| {
                println!("[Pinenut Error] {file}:{line} | {err}")
            })),
        }
    }
}
