#![allow(clippy::missing_safety_doc)]

mod bytes;

use std::mem;

pub use bytes::*;

mod call;
pub use call::*;
use pinenut_log::{Config, DateTime, Domain, Level, Location, Meta, Record, TimeDimension};

#[repr(C)]
pub struct FFIDomain {
    identifier: FFIBytes,
    directory: FFIBytes,
}

impl FFIDomain {
    #[inline]
    unsafe fn to_domain(&self) -> Domain {
        Domain::new(
            self.identifier.as_str().unwrap_or_default().to_string(),
            self.directory.as_str().unwrap_or_default().into(),
        )
    }
}

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum FFITimeDimension {
    Day = 1,
    Hour,
    Minute,
}

impl FFITimeDimension {
    #[inline]
    unsafe fn to_time_dimension(self) -> TimeDimension {
        mem::transmute(self as u8)
    }
}

#[repr(C)]
pub struct FFIConfig {
    use_mmap: bool,
    buffer_len: u64,
    rotation: FFITimeDimension,
    key_str: FFIBytes,
    compression_level: i32,
}

impl FFIConfig {
    #[inline]
    unsafe fn to_config(&self) -> Config {
        Config::new()
            .use_mmap(self.use_mmap)
            .buffer_len(self.buffer_len.try_into().expect("len cannot fit into usize"))
            .rotation(self.rotation.to_time_dimension())
            .key_str(self.key_str.as_slice())
            .compression_level(self.compression_level)
    }
}

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum FFILevel {
    Error = 1,
    Warn,
    Info,
    Debug,
    Verbose,
}

impl FFILevel {
    #[inline]
    unsafe fn to_level(self) -> Level {
        mem::transmute(self as u8)
    }
}

#[repr(C)]
pub struct FFIRecord {
    level: FFILevel,
    datetime_secs: i64,
    datetime_nsecs: u32,
    tag: FFIBytes,
    file: FFIBytes,
    func: FFIBytes,
    line: u32,
    thread_id: u64,
    content: FFIBytes,
}

impl FFIRecord {
    #[inline]
    unsafe fn to_record(&self) -> Record {
        let datetime =
            DateTime::from_timestamp(self.datetime_secs, self.datetime_nsecs).unwrap_or_default();
        let location = Location::new(
            self.file.as_str(),
            self.func.as_str(),
            (self.line != u32::MAX).then_some(self.line),
        );
        let meta = Meta::new(
            self.level.to_level(),
            datetime,
            location,
            self.tag.as_str(),
            (self.thread_id != u64::MAX).then_some(self.thread_id),
        );
        Record::new(meta, self.content.as_str().unwrap_or_default())
    }
}

pub mod logger {
    use std::ffi::c_void;

    use pinenut_log::Logger;

    use crate::{call::ffi_call, FFICallState, FFIConfig, FFIDomain, FFIRecord};

    #[no_mangle]
    pub unsafe extern "C" fn pinenut_logger_new(
        domain: FFIDomain,
        config: FFIConfig,
        state: &mut FFICallState,
    ) -> *mut c_void {
        ffi_call(state, || {
            let logger = Logger::new(domain.to_domain(), config.to_config());
            Box::into_raw(Box::new(logger)) as *mut c_void
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn pinenut_logger_log(
        ptr: *const c_void,
        record: FFIRecord,
        state: &mut FFICallState,
    ) {
        ffi_call(state, || {
            if !ptr.is_null() {
                let logger = &*(ptr as *const Logger);
                logger.log(&record.to_record());
            }
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn pinenut_logger_flush(ptr: *const c_void, state: &mut FFICallState) {
        ffi_call(state, || {
            if !ptr.is_null() {
                let logger = &*(ptr as *const Logger);
                logger.flush();
            }
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn pinenut_logger_trim(
        ptr: *const c_void,
        lifetime: u64,
        state: &mut FFICallState,
    ) {
        ffi_call(state, || {
            if !ptr.is_null() {
                let logger = &*(ptr as *const Logger);
                logger.trim(lifetime);
            }
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn pinenut_logger_shutdown(ptr: *mut c_void, state: &mut FFICallState) {
        ffi_call(state, || {
            if !ptr.is_null() {
                Box::from_raw(ptr as *mut Logger).shutdown()
            }
        })
    }

    /// In most cases, the upper layer just calls the [`pinenut_logger_shutdown`]
    /// function when the logger instance is deallocated.
    #[no_mangle]
    pub unsafe extern "C" fn pinenut_dealloc_logger(ptr: *mut c_void, state: &mut FFICallState) {
        ffi_call(state, || {
            if !ptr.is_null() {
                drop(Box::from_raw(ptr as *mut Logger));
            }
        })
    }
}

pub mod extract {
    use pinenut_log::{extract, DateTime};

    use crate::{call::ffi_call_result, FFIBytes, FFICallState, FFIDomain};

    #[no_mangle]
    pub unsafe extern "C" fn pinenut_extract(
        domain: FFIDomain,
        start_time: i64,
        end_time: i64,
        dest_path: FFIBytes,
        state: &mut FFICallState,
    ) {
        ffi_call_result(state, || {
            let start_time = DateTime::from_timestamp(start_time, 0).unwrap_or_default();
            let end_time = DateTime::from_timestamp(end_time, 0).unwrap_or_default();
            extract(
                domain.to_domain(),
                start_time..=end_time,
                dest_path.as_str().unwrap_or_default(),
            )
        })
    }
}

pub mod parser {
    use pinenut_log::{parse_to_file, DefaultFormatter};

    use crate::{call::ffi_call_result, FFIBytes, FFICallState};

    #[no_mangle]
    pub unsafe extern "C" fn pinenut_parse_to_file(
        path: FFIBytes,
        dest_path: FFIBytes,
        secret_key: FFIBytes,
        state: &mut FFICallState,
    ) {
        ffi_call_result(state, || {
            let secret_key = secret_key.as_slice().and_then(|k| k.try_into().ok());
            parse_to_file(
                path.as_str().unwrap_or_default(),
                dest_path.as_str().unwrap_or_default(),
                secret_key,
                DefaultFormatter,
            )
        })
    }
}
