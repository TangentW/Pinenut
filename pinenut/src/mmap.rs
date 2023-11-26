//! Memory-mapped.

use std::{
    fs,
    io::Error,
    ops::{Deref, DerefMut},
    os::fd::{IntoRawFd, RawFd},
    path::Path,
    ptr::{self, NonNull},
    slice,
    sync::atomic::{AtomicUsize, Ordering},
};

/// A handle to a fixed-length `memory-mapped` structure of the underlying file.
///
/// It wraps around the unsafe `mmap` call, exposing the safe interfaces. When it is
/// dropped, the `munmap` will be called automatically.
pub(crate) struct Mmap {
    ptr: NonNull<u8>,
    len: usize,
}

impl Mmap {
    /// Maps the entire underlying file to memory.
    ///
    /// # Arguments
    ///
    /// * `path` - The path of the underlying file. Make sure the file is readable
    ///   and writable.
    /// * `len` - The expected length of the entire file. It will be rounded up to a
    ///   multiple of the operating system's memory page size. The underlying file
    ///   will be resized to match the length.
    pub(crate) fn new(path: impl AsRef<Path>, len: usize) -> Result<Self, Error> {
        // Processes the input parameters.
        let (path, len) = (path.as_ref(), round_up_page_size(len));

        // Creates all intermediate directories if they are missing.
        if let Some(parent_path) = path.parent() {
            fs::create_dir_all(parent_path)?;
        }

        let file = fs::OpenOptions::new().read(true).write(true).create(true).open(path)?;

        // Adjusts the underlying file length.
        if file.metadata()?.len() != len as u64 {
            file.set_len(len as u64)?;
        }

        Self::map(file.into_raw_fd(), len).map(|ptr| Self { ptr, len })
    }

    /// A thin wrapper around the `mmap` system call.
    fn map(file: RawFd, len: usize) -> Result<NonNull<u8>, Error> {
        // SAFETY: Just a few FFI calls to libc.
        unsafe {
            let ptr = libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file,
                0,
            );

            if ptr == libc::MAP_FAILED {
                return Err(Error::last_os_error());
            }
            debug_assert_eq!(ptr as usize % page_size(), 0, "ptr is not page-aligned");

            // It might be a good idea to read some pages ahead.
            libc::madvise(ptr, len, libc::MADV_WILLNEED);

            Ok(NonNull::new_unchecked(ptr as *mut u8))
        }
    }

    /// Returns the number of bytes in the mmap.
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Acquires the underlying `*mut` pointer.
    #[inline]
    pub(crate) fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Extracts a slice of the entire mmap.
    #[inline]
    pub(crate) fn as_slice(&self) -> &[u8] {
        // SAFETY: Mmap is used, here the pointer and the length are guaranteed to be
        // correctly associated.
        unsafe { slice::from_raw_parts(self.as_ptr(), self.len()) }
    }

    /// Extracts a mutable slice of the entire mmap.
    #[inline]
    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: Mmap is used, here the pointer and the length are guaranteed to be
        // correctly associated.
        unsafe { slice::from_raw_parts_mut(self.as_ptr(), self.len()) }
    }
}

impl Deref for Mmap {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for Mmap {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl Drop for Mmap {
    #[inline]
    fn drop(&mut self) {
        let ptr = self.ptr.as_ptr() as *mut libc::c_void;
        // We just ignore the thrown error inside the `Drop` method.
        _ = unsafe { libc::munmap(ptr, self.len) };
    }
}

unsafe impl Send for Mmap {}
unsafe impl Sync for Mmap {}

/// Rounds up to a multiple of the operating system's memory page size.
#[inline]
fn round_up_page_size(value: usize) -> usize {
    let page_size = page_size();
    ((value - 1) / page_size + 1) * page_size
}

/// Obtains the operating system's memory page size.
fn page_size() -> usize {
    static PAGE_SIZE: AtomicUsize = AtomicUsize::new(0);
    // It is not guaranteed that `sysconf` will be called only once in multiple threads,
    // but it is possible to reduce the number of times it is called.
    match PAGE_SIZE.load(Ordering::Acquire) {
        0 => {
            let page_size = unsafe { libc::sysconf(libc::_SC_PAGE_SIZE) } as usize;
            PAGE_SIZE.store(page_size, Ordering::Release);
            page_size
        }
        page_size => page_size,
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io, io::Read};

    use tempfile::tempdir;

    use crate::mmap::{page_size, Mmap};

    #[test]
    fn test_mmap() -> io::Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("test");

        let mut mmap = Mmap::new(&path, page_size() + 1)?;
        assert_eq!(mmap.len(), 2 * page_size());

        const SLICE: &[u8] = b"Hello World";
        mmap[..SLICE.len()].copy_from_slice(SLICE);
        drop(mmap);

        let mut file = File::open(&path)?;
        assert_eq!(file.metadata()?.len(), 2 * page_size() as u64);

        let mut content = [0; SLICE.len()];
        file.read_exact(&mut content)?;
        assert_eq!(content, SLICE);

        Ok(())
    }
}
