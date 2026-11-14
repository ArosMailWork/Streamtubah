//! Signal/proc helpers. The calldata accessors in libobs are `static inline`,
//! so they are not exported; we parse the calldata stack layout directly:
//! `[name_size][name\0][data_size][data] ... [0]`.

use std::ffi::c_void;

use super::{cstr, sys};

const USIZE: usize = std::mem::size_of::<usize>();

/// Finds a named parameter and returns its raw bytes.
fn find<'a>(cd: *const sys::calldata_t, name: &str) -> Option<&'a [u8]> {
    // SAFETY: the stack is a contiguous buffer of `size` bytes owned by libobs
    // for the duration of the callback.
    unsafe {
        let cd = cd.as_ref()?;
        if cd.stack.is_null() || cd.size == 0 {
            return None;
        }
        let stack = std::slice::from_raw_parts(cd.stack, cd.size);
        let read_size = |at: usize| -> Option<usize> {
            stack.get(at..at + USIZE).map(|b| usize::from_ne_bytes(b.try_into().unwrap()))
        };
        let mut pos = 0;
        loop {
            let name_size = read_size(pos)?;
            pos += USIZE;
            if name_size == 0 {
                return None;
            }
            let param = stack.get(pos..pos + name_size)?;
            pos += name_size;
            let data_size = read_size(pos)?;
            pos += USIZE;
            let data = stack.get(pos..pos + data_size)?;
            pos += data_size;
            if param.strip_suffix(&[0]) == Some(name.as_bytes()) {
                return Some(data);
            }
        }
    }
}

pub fn get_int(cd: *const sys::calldata_t, name: &str) -> Option<i64> {
    find(cd, name).and_then(|b| b.try_into().ok()).map(i64::from_ne_bytes)
}

pub fn get_ptr(cd: *const sys::calldata_t, name: &str) -> *mut c_void {
    find(cd, name)
        .and_then(|b| b.try_into().ok())
        .map(|b| usize::from_ne_bytes(b) as *mut c_void)
        .unwrap_or(std::ptr::null_mut())
}

pub fn get_string(cd: *const sys::calldata_t, name: &str) -> Option<String> {
    let b = find(cd, name)?;
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    Some(String::from_utf8_lossy(&b[..end]).into_owned())
}

/// Calls a proc on a handler with empty params and returns the filled calldata.
pub struct CallData(sys::calldata_t);

impl CallData {
    pub fn call(handler: *mut sys::proc_handler_t, name: &str) -> Option<Self> {
        let mut cd = CallData(sys::calldata_t {
            stack: std::ptr::null_mut(),
            size: 0,
            capacity: 0,
            fixed: false,
        });
        let n = cstr(name);
        unsafe { sys::proc_handler_call(handler, n.as_ptr(), &mut cd.0) }.then_some(cd)
    }

    pub fn string(&self, name: &str) -> Option<String> {
        get_string(&self.0, name)
    }
}

impl Drop for CallData {
    fn drop(&mut self) {
        if !self.0.fixed && !self.0.stack.is_null() {
            unsafe { sys::bfree(self.0.stack as *mut c_void) }
        }
    }
}

/// Connects a signal callback. `data` must outlive the connection.
pub fn connect(
    handler: *mut sys::signal_handler_t,
    signal: &str,
    callback: unsafe extern "C" fn(*mut c_void, *mut sys::calldata_t),
    data: *mut c_void,
) {
    let s = cstr(signal);
    unsafe { sys::signal_handler_connect(handler, s.as_ptr(), Some(callback), data) }
}
