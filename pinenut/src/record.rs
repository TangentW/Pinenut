//!  The `Pinenut` log record.

use pinenut_derive::{Builder, Decode, Encode};

/// Represents logging levels of a `Pinenut` log.
///
/// The default value in [`Meta`] is [`Level::Info`].
#[repr(u8)]
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Level {
    /// The `error` log level.
    ///
    /// It is typically the highest level of severity and is used when an operation
    /// fails.
    Error = 1,
    /// The `warning` log level.
    ///
    /// It is used when something unexpected happened, or there might be a problem in
    /// the near future.
    Warn,
    /// The `informational` log level.
    ///
    /// Infomational messages to track the general flow of the application.
    Info,
    /// The `debug` log level.
    ///
    /// Logs that contain information useful for debugging during development and
    /// troubleshooting.
    Debug,
    /// The `verbose` log level.
    ///
    /// Logs may include more information than the `Debug` level and are usually not
    /// enabled in a production environment.
    Verbose,
}

impl Level {
    /// Returns the underlying primitive representation.
    #[inline]
    pub(crate) fn primitive(&self) -> u8 {
        *self as u8
    }

    /// Constructs from the underlying primitive representation.
    pub(crate) fn from_primitive(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Error),
            2 => Some(Self::Warn),
            3 => Some(Self::Info),
            4 => Some(Self::Debug),
            5 => Some(Self::Verbose),
            _ => None,
        }
    }
}

/// Represents a location in the code where a `Pinenut` log was generated.
///
/// The default options are:
///
/// - [`Location::file`] : `None`
/// - [`Location::func`] : `None`
/// - [`Location::line`] : `None`
///
/// `Location` supports `Builder Pattern`, it can be constructed by
/// `LocationBuilder`.
#[derive(Encode, Decode, Builder, Default, Clone, PartialEq, Eq, Debug)]
pub struct Location<'a> {
    file: Option<&'a str>,
    func: Option<&'a str>,
    line: Option<u32>,
}

impl<'a> Location<'a> {
    /// Constructs a new `Location`.
    #[inline]
    pub fn new(file: Option<&'a str>, func: Option<&'a str>, line: Option<u32>) -> Self {
        Self { file, func, line }
    }

    /// The code file where the log was generated. `None` if not available.
    #[inline]
    pub fn file(&self) -> Option<&'a str> {
        self.file
    }

    /// The function where the log was generated. `None` if not available.
    #[inline]
    pub fn func(&self) -> Option<&'a str> {
        self.func
    }

    /// The code line in the file where the log was generated. `None` if not
    /// available.
    #[inline]
    pub fn line(&self) -> Option<u32> {
        self.line
    }
}

/// Represents a date and time in the UTC time zone.
pub type DateTime = chrono::DateTime<chrono::Utc>;

/// Represents metadata associated with a `Pinenut` log.
///
/// The default options are:
///
/// - [`Meta::level`] : [`Level::Info`]
/// - [`Meta::datetime`] : [`chrono::Utc::now()`]
/// - [`Meta::location`] : [`Location::default()`]
/// - [`Meta::tag`] : [`None`]
/// - [`Meta::thread_id`] : [`None`]
///
/// `Meta` supports `Builder Pattern`, it can be constructed by `MetaBuilder`.
#[derive(Encode, Decode, Builder, Clone, PartialEq, Eq, Debug)]
pub struct Meta<'a> {
    level: Level,
    datetime: DateTime,
    location: Location<'a>,
    tag: Option<&'a str>,
    thread_id: Option<u64>,
}

impl<'a> Meta<'a> {
    /// Constructs a new `Meta`.
    #[inline]
    pub fn new(
        level: Level,
        datetime: DateTime,
        location: Location<'a>,
        tag: Option<&'a str>,
        thread_id: Option<u64>,
    ) -> Self {
        Self { level, datetime, location, tag, thread_id }
    }

    /// The level of the log.
    #[inline]
    pub fn level(&self) -> Level {
        self.level
    }

    /// The datetime when the log was generated.
    #[inline]
    pub fn datetime(&self) -> DateTime {
        self.datetime
    }

    /// The location in the code where the log was generated.
    #[inline]
    pub fn location(&self) -> &Location<'a> {
        &self.location
    }

    /// An optional tag associated with the log.
    #[inline]
    pub fn tag(&self) -> Option<&'a str> {
        self.tag
    }

    /// The identifier of the thread where the log was generated.
    #[inline]
    pub fn thread_id(&self) -> Option<u64> {
        self.thread_id
    }
}

impl<'a> Default for Meta<'a> {
    #[inline]
    fn default() -> Self {
        Meta::new(Level::Info, chrono::Utc::now(), Location::default(), None, None)
    }
}

/// Represents a `Pinenut` log record.
///
/// `Record` supports `Builder Pattern`, it can be constructed by `RecordBuilder`.
#[derive(Encode, Decode, Builder, Default, Clone, PartialEq, Eq, Debug)]
pub struct Record<'a> {
    meta: Meta<'a>,
    content: &'a str,
}

impl<'a> Record<'a> {
    /// Constructs a new `Record`.
    #[inline]
    pub fn new(meta: Meta<'a>, content: &'a str) -> Self {
        Self { meta, content }
    }

    /// The metadata associated with the log.
    #[inline]
    pub fn meta(&self) -> &Meta<'a> {
        &self.meta
    }

    /// The content of the log.
    #[inline]
    pub fn content(&self) -> &'a str {
        self.content
    }
}
