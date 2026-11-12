//! Permission checks and one-shot idle activity simulation; scheduling belongs to the app.

use std::ffi::c_void;
use std::ptr::{self, NonNull};

const IDLE_THRESHOLD_SECONDS: f64 = 90.0;
const COMBINED_SESSION_STATE: i32 = 0;
const ANY_INPUT_EVENT: u32 = u32::MAX;
const MOUSE_MOVED: u32 = 5;
const HID_EVENT_TAP: u32 = 0;
const MOUSE_DELTA_X: u32 = 4;
const MOUSE_DELTA_Y: u32 = 5;

#[repr(C)]
#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
    static kAXTrustedCheckOptionPrompt: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        count: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> *const c_void;
    fn CFRelease(value: *const c_void);
    static kCFBooleanTrue: *const c_void;
    // Opaque callback tables: only their native addresses cross the FFI boundary.
    static kCFTypeDictionaryKeyCallBacks: [u8; 0];
    static kCFTypeDictionaryValueCallBacks: [u8; 0];
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    fn CGEventCreate(source: *const c_void) -> *const c_void;
    fn CGEventGetLocation(event: *const c_void) -> Point;
    fn CGEventSetType(event: *const c_void, event_type: u32);
    fn CGEventSetLocation(event: *const c_void, point: Point);
    fn CGEventSetIntegerValueField(event: *const c_void, field: u32, value: i64);
    fn CGEventPost(tap: u32, event: *const c_void);
}

struct CfOwned(NonNull<c_void>);

impl CfOwned {
    fn from_created(value: *const c_void) -> Option<Self> {
        NonNull::new(value.cast_mut()).map(Self)
    }

    fn as_ptr(&self) -> *const c_void {
        self.0.as_ptr()
    }
}

impl Drop for CfOwned {
    fn drop(&mut self) {
        // Dictionaries and CGEvents both follow Core Foundation's Create rule.
        unsafe { CFRelease(self.as_ptr()) };
    }
}

/// Returns the current Accessibility trust state.
///
/// A requested prompt is asynchronous: `false` still means permission has not
/// been granted yet. Failure to allocate prompt options also returns `false`.
pub fn permitted(prompt: bool) -> bool {
    // A null options dictionary performs a check without prompting.
    let trusted = unsafe { AXIsProcessTrustedWithOptions(ptr::null()) != 0 };
    if trusted || !prompt {
        return trusted;
    }

    // The dictionary retains these borrowed constants with the native CF callbacks.
    let options = unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        CfOwned::from_created(CFDictionaryCreate(
            ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            (&raw const kCFTypeDictionaryKeyCallBacks).cast(),
            (&raw const kCFTypeDictionaryValueCallBacks).cast(),
        ))
    };
    let Some(options) = options else {
        return false;
    };
    unsafe { AXIsProcessTrustedWithOptions(options.as_ptr()) != 0 }
}

/// Posts a stationary mouse-move event after at least 90 seconds of session idle.
///
/// The caller must invoke this only while Caffeine, activity simulation, and the
/// user session are active. This function creates no timers and never prompts.
/// `Ok(())` means no event was needed or an event was submitted; Core Graphics
/// provides no delivery acknowledgement for `CGEventPost`.
pub fn simulate_if_idle() -> Result<(), String> {
    // Combined session state includes all keyboard, mouse, and tablet event
    // sources in this login session, including our previously posted event.
    let idle =
        unsafe { CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION_STATE, ANY_INPUT_EVENT) };
    if !idle.is_finite() || idle < 0.0 {
        return Err(format!(
            "CGEventSourceSecondsSinceLastEventType returned invalid idle seconds: {idle}"
        ));
    }
    if idle < IDLE_THRESHOLD_SECONDS {
        return Ok(());
    }
    if !permitted(false) {
        return Err(
            "Accessibility permission is required to simulate activity. Enable Caffeine in System Settings > Privacy & Security > Accessibility."
                .into(),
        );
    }

    // CGEventCreate captures the current global cursor location. Reuse that
    // event instead of allocating a second event or converting screen origins.
    let event = CfOwned::from_created(unsafe { CGEventCreate(ptr::null()) })
        .ok_or_else(|| "CGEventCreate failed to allocate a cursor event".to_owned())?;
    unsafe {
        let point = CGEventGetLocation(event.as_ptr());
        CGEventSetType(event.as_ptr(), MOUSE_MOVED);
        CGEventSetLocation(event.as_ptr(), point);
        CGEventSetIntegerValueField(event.as_ptr(), MOUSE_DELTA_X, 0);
        CGEventSetIntegerValueField(event.as_ptr(), MOUSE_DELTA_Y, 0);
        CGEventPost(HID_EVENT_TAP, event.as_ptr());
    }
    Ok(())
}
