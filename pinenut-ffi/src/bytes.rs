use core::slice;
use std::{ffi::c_void, mem::ManuallyDrop, ptr, str};

use crate::{call::ffi_call, FFICallState};

#[repr(C)]
pub struct FFIBytes {
    ptr: *const c_void,
    len: u64,
}

impl FFIBytes {
    #[inline]
    pub(crate) fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    #[inline]
    pub(crate) unsafe fn as_slice(&self) -> Option<&[u8]> {
        if self.is_null() {
            return None;
        }
        Some(slice::from_raw_parts(
            self.ptr as *const u8,
            self.len.try_into().expect("len cannot fit into usize"),
        ))
    }

    #[inline]
    pub(crate) unsafe fn as_str(&self) -> Option<&str> {
        if let Some(slice) = self.as_slice() {
            Some(str::from_utf8_unchecked(slice))
        } else {
            None
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn pinenut_bytes_null() -> FFIBytes {
    FFIBytes { ptr: ptr::null(), len: 0 }
}

#[repr(C)]
pub struct FFIBytesBuf {
    ptr: *mut c_void,
    len: u64,
    capacity: u64,
}

impl FFIBytesBuf {
    pub(crate) const NULL: Self = Self { ptr: ptr::null_mut(), len: 0, capacity: 0 };

    #[inline]
    pub(crate) fn new(inner: Vec<u8>) -> Self {
        let mut inner = ManuallyDrop::new(inner);
        let ptr = inner.as_mut_ptr() as *mut c_void;
        let len = inner.len().try_into().expect("len cannot fit into u64");
        let capacity = inner.capacity().try_into().expect("capacity cannot fit into u64");
        Self { ptr, len, capacity }
    }

    #[inline]
    pub(crate) unsafe fn dealloc(self) {
        if !self.ptr.is_null() {
            drop(self.lift());
        }
    }

    #[inline]
    unsafe fn lift(self) -> Vec<u8> {
        debug_assert!(!self.ptr.is_null());

        let len = self.len.try_into().expect("len cannot fit into usize");
        let capacity = self.capacity.try_into().expect("capacity cannot fit into usize");
        assert!(len <= capacity);

        let ptr = self.ptr as *mut u8;
        Vec::from_raw_parts(ptr, len, capacity)
    }
}

impl Default for FFIBytesBuf {
    #[inline]
    fn default() -> Self {
        Self::NULL
    }
}

impl From<Vec<u8>> for FFIBytesBuf {
    #[inline]
    fn from(value: Vec<u8>) -> Self {
        Self::new(value)
    }
}

#[no_mangle]
pub unsafe extern "C" fn pinenut_dealloc_bytes(bytes: FFIBytesBuf, state: &mut FFICallState) {
    ffi_call(state, || bytes.dealloc());
}
