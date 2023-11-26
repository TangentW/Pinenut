//! A `double-buffering` system that wraps around the underlying memory.
//!
//! It isolates the two buffers from each other, so that when one buffer is read or
//! written, the other buffer can also be operated in parallel.
//!
//! # The underlying structure
//!
//! ```plain
//!     ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─   n   ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─
//!    ├──── 8 ─────┬───── (n - 8) / 2 ──────┬───── (n - 8) / 2 ───────┤
//!    ▼────────────▼────────────────────────▼─────────────────────────▼
//! ┌──│   Header   │         Alpha          │          Beta           │
//! │  └────────────┴────────────────────────┴─────────────────────────┘
//! │  ┌───────────┬──────────────┐ (n: length of the underlying memory)
//! └─▶│   Magic   │  Alpha Side  │                                     
//!    ▲───────────▲──────────────▲                                     
//!    └──── 4 ────┴────── 4 ─────┘                                     
//! ```

use std::{
    cell::UnsafeCell,
    mem,
    ops::{Deref, DerefMut, Not},
    sync::{Arc, RwLock, RwLockReadGuard},
};

use crate::{mmap::Mmap, Magic, Sealed};

/// The underlying memory wrapped in the `double-buffering` system.
#[allow(clippy::len_without_is_empty)]
pub(crate) trait Memory: DerefMut<Target = [u8]> + Send + Sync + 'static + Sealed {
    /// Returns the number of bytes in the memory.
    fn len(&self) -> usize;

    /// Acquires the underlying `*const` pointer.
    fn as_ptr(&self) -> *const u8;

    /// Acquires the underlying `*mut` pointer.
    fn as_mut_ptr(&mut self) -> *mut u8;
}

/// Represents the two buffers (the `left` component and the `right` component) in
/// the double buffering system.
pub(crate) type Couple<M> = (Buffer<M>, Buffer<M>);

/// Initializes the double buffering system with the underlying memory.
///
/// The return value is a couple of buffers that can operate on the same underlying
/// data, but at different offsets.
pub(crate) fn initialize<M>(memory: M) -> Couple<M>
where
    M: Memory,
{
    let inner = BufferInner::new(memory);
    let inner = Arc::new(RwLock::new(inner));

    let left = Buffer { inner: inner.clone(), side: Side::Left };
    let right = Buffer { inner, side: Side::Right };

    (left, right)
}

/// Represents a buffer in the double buffering system.
///
/// It provides methods to read from, write to, and switch the buffer.
pub(crate) struct Buffer<M> {
    inner: Arc<RwLock<BufferInner<M>>>,
    side: Side,
}

impl<M> Buffer<M>
where
    M: Memory,
{
    /// Prepare to read or write the buffer.
    ///
    /// It returns a `BufferHandle` for reading and writing the buffer.
    #[inline]
    pub(crate) fn handle(&mut self) -> BufferHandle<M> {
        let inner = self.inner.read().unwrap();
        BufferHandle { inner, side: self.side }
    }

    /// Switches the side of the buffer.
    ///
    /// It swaps the underlying memory of the couple buffers.
    #[inline]
    pub(crate) fn switch(&mut self) {
        self.inner.write().unwrap().switch();
    }
}

/// A handle for reading and writing the buffer.
pub(crate) struct BufferHandle<'a, M> {
    inner: RwLockReadGuard<'a, BufferInner<M>>,
    side: Side,
}

impl<'a, M> BufferHandle<'a, M>
where
    M: Memory,
{
    /// Extracts a slice of the entire buffer.
    #[inline]
    pub(crate) fn as_slice(&self) -> &[u8] {
        // SAFETY: It's safe to call here because we ensure that the side is valid and the
        // buffer is properly initialized.
        unsafe { self.inner.buffer(self.side) }
    }

    /// Extracts a mutable slice of the entire buffer.
    #[inline]
    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: It's safe to call here because we ensure that the side is valid and the
        // buffer is properly initialized.
        unsafe { self.inner.buffer(self.side) }
    }
}

impl<'a, M> Deref for BufferHandle<'a, M>
where
    M: Memory,
{
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<'a, M> DerefMut for BufferHandle<'a, M>
where
    M: Memory,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

// ============ Memorys ============

pub(crate) enum EitherMemory {
    Mmap(Mmap),
    Vec(Vec<u8>),
}

impl Deref for EitherMemory {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Mmap(mmap) => mmap.deref(),
            Self::Vec(vec) => vec.deref(),
        }
    }
}

impl DerefMut for EitherMemory {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Mmap(mmap) => mmap.deref_mut(),
            Self::Vec(vec) => vec.deref_mut(),
        }
    }
}

impl Memory for EitherMemory {
    #[inline]
    fn len(&self) -> usize {
        match self {
            Self::Mmap(mmap) => mmap.len(),
            Self::Vec(vec) => vec.len(),
        }
    }

    #[inline]
    fn as_ptr(&self) -> *const u8 {
        match self {
            Self::Mmap(mmap) => mmap.as_ptr(),
            Self::Vec(vec) => vec.as_ptr(),
        }
    }

    #[inline]
    fn as_mut_ptr(&mut self) -> *mut u8 {
        match self {
            Self::Mmap(mmap) => mmap.as_mut_ptr(),
            Self::Vec(vec) => vec.as_mut_ptr(),
        }
    }
}

impl Sealed for EitherMemory {}

impl Sealed for Mmap {}

impl Memory for Mmap {
    #[inline]
    fn len(&self) -> usize {
        self.len()
    }

    #[inline]
    fn as_ptr(&self) -> *const u8 {
        self.as_ptr()
    }

    #[inline]
    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.as_ptr()
    }
}

impl Sealed for Vec<u8> {}

impl Memory for Vec<u8> {
    #[inline]
    fn len(&self) -> usize {
        self.len()
    }

    #[inline]
    fn as_ptr(&self) -> *const u8 {
        self.as_ptr()
    }

    #[inline]
    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.as_mut_ptr()
    }
}

// ============ Internal ============

/// Represents which side the component is in the double buffer.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Left = 0xABC,
    Right = 0xDEF,
}

impl Side {
    /// Returns a raw value of type `[u8; 4]`.
    #[inline]
    const fn raw(&self) -> [u8; 4] {
        (*self as u32).to_le_bytes()
    }
}

impl TryFrom<[u8; 4]> for Side {
    type Error = ();

    #[inline]
    fn try_from(value: [u8; 4]) -> Result<Self, Self::Error> {
        const LEFT_RAW: [u8; 4] = Side::Left.raw();
        const RIGHT_RAW: [u8; 4] = Side::Right.raw();

        match value {
            LEFT_RAW => Ok(Self::Left),
            RIGHT_RAW => Ok(Self::Right),
            _ => Err(()),
        }
    }
}

impl Not for Side {
    type Output = Self;

    #[inline]
    fn not(self) -> Self::Output {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// Represents the header of the buffer file.
#[repr(C)]
#[derive(Debug)]
struct Header {
    /// The header identifier.
    magic: [u8; 4],
    /// Which side of the alpha component.
    alpha_side: [u8; 4],
}

impl Header {
    /// Length of a header in bytes. (8 bytes)
    const LEN: usize = mem::size_of::<Self>();

    /// It means: `Feed Cat Buffer`.
    const MAGIC: Magic = Magic::new(0xFEEDCA7B);
}

impl Default for Header {
    #[inline]
    fn default() -> Self {
        Self { magic: Self::MAGIC.into(), alpha_side: Side::Left.raw() }
    }
}

/// The underlying buffer.
#[repr(transparent)]
struct BufferInner<M>(UnsafeCell<M>);

unsafe impl<M> Send for BufferInner<M> where M: Send {}
unsafe impl<M> Sync for BufferInner<M> where M: Sync {}

impl<M> BufferInner<M>
where
    M: Memory,
{
    fn new(memory: M) -> Self {
        // Check length and alignment.
        // The alignment of Header is `1`, so memory always conforms to this.
        debug_assert!(memory.len() >= Header::LEN, "the memory is too small");
        let mut buffer = Self(UnsafeCell::new(memory));
        buffer.initialize();
        buffer
    }

    #[inline]
    fn initialize(&mut self) {
        // If the buffer file is invalid (which is not initialized or modified incorrectly),
        // just re-initialize the header.
        if !self.validate() {
            *self.header_mut() = Header::default();
        }
    }

    /// Checks the correctness of the buffer file.
    ///
    /// Returns `false` when the buffer file is invalid.
    #[inline]
    fn validate(&self) -> bool {
        let header = self.header();
        (Header::MAGIC == header.magic.into())
            && ([Side::Left.raw(), Side::Right.raw()].contains(&header.alpha_side))
    }

    #[inline]
    fn switch(&mut self) {
        let header = self.header_mut();
        let side = !Side::try_from(header.alpha_side).unwrap_or(Side::Left);
        header.alpha_side = side.raw();
    }

    #[allow(clippy::mut_from_ref)]
    unsafe fn buffer(&self, side: Side) -> &mut [u8] {
        let memory = self.memory();
        let len = (memory.len() - Header::LEN) / 2;

        // Determines whether it is alpha component.
        let is_alpha = self.header().alpha_side == side.raw();
        let offset = if is_alpha { Header::LEN } else { Header::LEN + len };

        &mut memory[offset..offset + len]
    }

    #[inline]
    fn header(&self) -> &Header {
        // SAFETY: The pointer to the memory is properly aligned for a `Header`. Also, it has
        // been verified at construction to ensure that there are no pointer out-of-bounds
        // issues here.
        unsafe {
            let ptr = self.memory().as_ptr() as *const Header;
            &*ptr
        }
    }

    #[inline]
    fn header_mut(&mut self) -> &mut Header {
        // SAFETY: The pointer to the memory is properly aligned for a `Header`. Also, it has
        // been verified at construction to ensure that there are no pointer out-of-bounds
        // issues here.
        unsafe {
            let ptr = self.memory().as_mut_ptr() as *mut Header;
            &mut *ptr
        }
    }

    #[inline]
    #[allow(clippy::mut_from_ref)]
    unsafe fn memory(&self) -> &mut M {
        &mut *self.0.get()
    }
}

#[cfg(test)]
mod tests {
    use std::{io, thread};

    use tempfile::tempdir;

    use crate::{
        buffer::{self, Memory},
        mmap::Mmap,
    };

    #[test]
    fn test_mmap_buffer() -> io::Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("test.pinebuf");
        let mmap = Mmap::new(path, 4096)?;
        test_buffer(mmap)
    }

    #[test]
    fn test_vec_buffer() -> io::Result<()> {
        let mut vec = Vec::with_capacity(256);
        unsafe {
            vec.set_len(256);
        }
        test_buffer(vec)
    }

    fn test_buffer<M>(memory: M) -> io::Result<()>
    where
        M: Memory,
    {
        let (mut left, mut right) = buffer::initialize(memory);

        left.handle()[0..5].copy_from_slice(b"Alpha");

        thread::spawn(move || {
            right.handle()[0..4].copy_from_slice(b"Beta");
            right.switch();

            assert_eq!(&right.handle()[0..5], b"Alpha");
        })
        .join()
        .unwrap();

        assert_eq!(&left.handle()[0..4], b"Beta");

        Ok(())
    }
}
