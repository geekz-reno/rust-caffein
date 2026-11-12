//! A persistent display-idle assertion, held until its owner drops it.

use std::ffi::{CStr, c_char, c_void};
use std::ptr::{self, NonNull};

const ASSERTION_LEVEL_ON: u32 = 255;
const UTF8_ENCODING: u32 = 0x0800_0100;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPMAssertionCreateWithName(
        assertion_type: *const c_void,
        assertion_level: u32,
        assertion_name: *const c_void,
        assertion_id: *mut u32,
    ) -> i32;
    fn IOPMAssertionRelease(assertion_id: u32) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        text: *const c_char,
        encoding: u32,
    ) -> *const c_void;
    fn CFRelease(value: *const c_void);
}

#[link(name = "System")]
unsafe extern "C" {
    fn mach_error_string(error: i32) -> *const c_char;
}

struct CfString(NonNull<c_void>);

impl CfString {
    fn new(text: &CStr) -> Result<Self, String> {
        // The input is NUL-terminated UTF-8; a null allocator selects the default.
        let value = unsafe { CFStringCreateWithCString(ptr::null(), text.as_ptr(), UTF8_ENCODING) };
        NonNull::new(value.cast_mut())
            .map(Self)
            .ok_or_else(|| format!("CFStringCreateWithCString failed for {:?}", text))
    }

    fn as_ptr(&self) -> *const c_void {
        self.0.as_ptr()
    }
}

impl Drop for CfString {
    fn drop(&mut self) {
        // Create-rule ownership: this wrapper releases exactly one reference.
        unsafe { CFRelease(self.as_ptr()) };
    }
}

fn assertion_error(operation: &str, result: i32) -> String {
    // mach_error_string returns a borrowed, NUL-terminated system string.
    let message = unsafe { mach_error_string(result) };
    if message.is_null() {
        format!(
            "{operation} failed: IOReturn {result} (0x{:08x})",
            result as u32
        )
    } else {
        let message = unsafe { CStr::from_ptr(message) }.to_string_lossy();
        format!(
            "{operation} failed: {message}; IOReturn {result} (0x{:08x})",
            result as u32
        )
    }
}

/// Prevents idle display sleep, not explicit sleep, lid closure, or low-battery sleep.
///
/// This assertion has no timeout and never needs a refresh timer. The caller must
/// drop it when disabled, quitting, or leaving the active user session.
#[must_use = "dropping the assertion immediately allows idle sleep again"]
pub struct PowerAssertion {
    id: u32,
}

impl PowerAssertion {
    pub fn acquire() -> Result<Self, String> {
        let assertion_type = CfString::new(c"PreventUserIdleDisplaySleep")?;
        let reason = CfString::new(c"Caffeine prevents idle display sleep")?;
        let mut id = 0;
        // Both CFStrings remain alive for this call; IOKit retains its own data.
        let result = unsafe {
            IOPMAssertionCreateWithName(
                assertion_type.as_ptr(),
                ASSERTION_LEVEL_ON,
                reason.as_ptr(),
                &mut id,
            )
        };
        if result != 0 {
            return Err(assertion_error("IOPMAssertionCreateWithName", result));
        }
        if id == 0 {
            return Err(
                "IOPMAssertionCreateWithName returned success with a null assertion ID".into(),
            );
        }
        Ok(Self { id })
    }
}

impl Drop for PowerAssertion {
    fn drop(&mut self) {
        // This type is neither Clone nor Copy: the assertion has one owner.
        let result = unsafe { IOPMAssertionRelease(self.id) };
        if result != 0 {
            eprintln!("{}", assertion_error("IOPMAssertionRelease", result));
        }
    }
}
