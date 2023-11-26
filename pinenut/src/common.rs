use std::{
    fs::{self, File, OpenOptions},
    io,
    marker::PhantomData,
    ops::Deref,
    path::Path,
    ptr, slice,
};

/// Represents a target for processed data.
///
/// For the sake of generality, generics are used to define the type of errors that
/// can be occurred during a series of processing steps.
pub(crate) trait Sink<E> {
    /// Type of errors that can be occurred by self.
    type Error: From<E>;

    /// Where the actual writing of bytes happens.
    fn sink(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
}

// For testing, implement `Sink` for `Vec<u8>`.
#[cfg(test)]
impl<E> Sink<E> for Vec<u8> {
    type Error = E;

    #[inline]
    fn sink(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.extend_from_slice(bytes);
        Ok(())
    }
}

/// A closure wrapper that implements the `Sink` trait.
pub(crate) struct FnSink<F, Error> {
    inner: F,
    _error: PhantomData<Error>,
}

impl<F, Error> FnSink<F, Error> {
    /// Constructs a new `FnSink` with a closure.
    #[inline]
    pub(crate) fn new(inner: F) -> Self {
        Self { inner, _error: PhantomData }
    }
}

impl<F, E, Error> Sink<E> for FnSink<F, Error>
where
    F: FnMut(&[u8]) -> Result<(), Error>,
    Error: From<E>,
{
    type Error = Error;

    #[inline]
    fn sink(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        (self.inner)(bytes)
    }
}

/// A special sequence of bytes that is used at the beginning of the specific data
/// structure for validation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub(crate) struct Magic(u32);

impl Magic {
    /// Constructs a new `Magic`.
    #[inline]
    pub(crate) const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Constructs a new `Magic` with raw value of type `[u8; 4]`.
    #[inline]
    pub(crate) const fn from_raw(raw: [u8; 4]) -> Self {
        Self(u32::from_le_bytes(raw))
    }

    /// Returns a raw value of type `[u8; 4]`.
    #[inline]
    pub(crate) const fn raw(&self) -> [u8; 4] {
        self.0.to_le_bytes()
    }
}

impl From<[u8; 4]> for Magic {
    #[inline]
    fn from(value: [u8; 4]) -> Self {
        Self::from_raw(value)
    }
}

impl From<Magic> for [u8; 4] {
    #[inline]
    fn from(value: Magic) -> Self {
        value.raw()
    }
}

/// Represents a bytes data buffer.
///
/// Used to store temporary bytes data during processing.
pub(crate) struct BytesBuf(Vec<u8>);

impl BytesBuf {
    #[inline]
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self(Vec::with_capacity(capacity))
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub(crate) fn as_buffer_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: Here the length is guaranteed to be correct.
        unsafe { slice::from_raw_parts_mut(self.0.as_mut_ptr(), self.0.capacity()) }
    }

    /// Pushes bytes to fill the buffer as much as possible, and returns the length
    /// of the bytes that were pushed.
    pub(crate) fn buffer(&mut self, bytes: &[u8]) -> usize {
        let spare_capacity = self.0.capacity() - self.0.len();
        let buffered = bytes.len().min(spare_capacity);
        let old_len = self.0.len();
        // SAFETY: It's impossible for buffer and bytes to overlap, and the input length
        // (buffered) is less than or equal to spare capacity.
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), self.0.as_mut_ptr().add(old_len), buffered);
            self.0.set_len(old_len + buffered);
        }
        buffered
    }

    /// Removes the previous bytes of the specified length from the buffer.
    pub(crate) fn drain(&mut self, len: usize) {
        debug_assert!(len <= self.0.len());
        let len = len.min(self.0.len());
        let remaining = self.0.len() - len;
        // SAFETY: Copy `buffer[len..]` to `buffer[..remaining]`. The slices can overlap, so
        // `copy_nonoverlapping` cannot be used.
        unsafe {
            ptr::copy(self.0.as_ptr().add(len), self.0.as_mut_ptr(), remaining);
            self.0.set_len(remaining);
        }
    }

    /// Clears the buffer.
    pub(crate) fn clear(&mut self) {
        self.0.clear()
    }
}

impl Deref for BytesBuf {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

/// Represents a file writer which opens the file only when it is actually ready to
/// be written to.
pub(crate) struct LazyFileWriter<'a> {
    path: &'a Path,
    inner: Option<File>,
}

impl<'a> LazyFileWriter<'a> {
    #[inline]
    pub(crate) fn new(path: &'a Path) -> Self {
        Self { path, inner: None }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.inner.is_none()
    }
}

impl<'a> io::Write for LazyFileWriter<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.inner.is_none() {
            // Creates all intermediate directories if they are missing.
            if let Some(parent_path) = self.path.parent() {
                fs::create_dir_all(parent_path)?;
            }

            let file =
                OpenOptions::new().create(true).truncate(true).write(true).open(self.path)?;
            self.inner = Some(file);
        }

        // SAFETY: a `None` variant for `inner` would have been replaced by a `Some` variant
        // in the code above.
        let file = unsafe { self.inner.as_mut().unwrap_unchecked() };
        file.write(buf)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = self.inner.as_mut() {
            file.flush()
        } else {
            Ok(())
        }
    }
}

/// This trait being unreachable from outside the crate prevents outside
/// implementations of our specified traits.
pub trait Sealed {}

impl<T> Sealed for Option<T> where T: Sealed {}

/// Decodes the hex string to bytes slice.
#[allow(dead_code)] // Maybe we'll use it later...
pub(crate) fn decode_hex(str: &str) -> Option<Vec<u8>> {
    if str.len() % 2 != 0 {
        return None;
    }
    (0..str.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&str[i..i + 1], 16))
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

#[cfg(test)]
mod tests {
    use zstd_safe::WriteBuf;

    use crate::common::BytesBuf;

    #[test]
    fn test_bytesbuf() {
        let mut buffer = BytesBuf::with_capacity(4);

        assert_eq!(buffer.buffer(&[1, 2, 3, 4, 5, 6]), 4);
        assert_eq!(buffer.as_slice(), &[1, 2, 3, 4]);

        buffer.drain(3);
        assert_eq!(buffer.as_slice(), &[4]);

        assert_eq!(buffer.buffer(&[1, 2, 3, 4, 5, 6]), 3);
        assert_eq!(buffer.as_slice(), &[4, 1, 2, 3]);
    }
}
