use std::{any::Any, error::Error, panic, ptr};

use FFICallCode::*;

use crate::FFIBytesBuf;

#[repr(C)]
pub enum FFICallCode {
    FFICallSucces = 0,
    FFICallError,
    FFICallPanic,
}

#[repr(C)]
pub struct FFICallState {
    code: FFICallCode,
    err_desc: FFIBytesBuf,
}

pub(crate) fn ffi_call<T, F>(state: &mut FFICallState, call: F) -> T
where
    T: FFIDefault,
    F: FnOnce() -> T + panic::UnwindSafe,
{
    match panic::catch_unwind(call) {
        Ok(value) => {
            *state = FFICallState::SUCCESS;
            value
        }
        Err(error) => {
            *state = FFICallState::panic(error);
            T::default()
        }
    }
}

pub(crate) fn ffi_call_result<T, E, F>(state: &mut FFICallState, call: F) -> T
where
    T: FFIDefault,
    E: Error,
    F: FnOnce() -> Result<T, E> + panic::UnwindSafe,
{
    match panic::catch_unwind(call) {
        Ok(result) => match result {
            Ok(value) => {
                *state = FFICallState::SUCCESS;
                value
            }
            Err(error) => {
                *state = FFICallState::error(error);
                T::default()
            }
        },
        Err(error) => {
            *state = FFICallState::panic(error);
            T::default()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn pinenut_call_state_success() -> FFICallState {
    FFICallState::SUCCESS
}

impl FFICallState {
    pub(crate) const SUCCESS: Self = Self { code: FFICallSucces, err_desc: FFIBytesBuf::NULL };

    #[inline]
    fn error<E>(error: E) -> Self
    where
        E: Error,
    {
        let err_desc = error.to_string().into_bytes().into();
        Self { code: FFICallError, err_desc }
    }

    fn panic(error: Box<dyn Any + Send + 'static>) -> Self {
        let err_desc = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            if let Some(str) = error.downcast_ref::<&'static str>() {
                str.to_string().into_bytes().into()
            } else if let Some(str) = error.downcast_ref::<String>() {
                str.clone().into_bytes().into()
            } else {
                "panic".to_string().into_bytes().into()
            }
        }))
        .unwrap_or_default();

        Self { code: FFICallPanic, err_desc }
    }
}

pub(crate) trait FFIDefault {
    fn default() -> Self;
}

impl<T> FFIDefault for *mut T {
    #[inline]
    fn default() -> Self {
        ptr::null_mut()
    }
}

impl FFIDefault for () {
    #[inline]
    fn default() -> Self {}
}
