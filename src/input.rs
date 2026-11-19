//! The real mouse, wherever it is: the overlays pass every click through, so they never hear
//! the mouse themselves. CoreGraphics reports the cursor and buttons system-wide without any
//! permission prompt. Also hides the system cursor while the glove stands in for it.

/// The mouse in global points (top-left of the main display is the origin, y down).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub x: f64,
    pub y: f64,
    pub left: bool,
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{c_char, c_void};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreate(source: *const c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
        fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
        fn CGDisplayHideCursor(display: u32) -> i32;
        fn CGDisplayShowCursor(display: u32) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
        fn CFStringCreateWithCString(alloc: *const c_void, s: *const c_char, encoding: u32) -> *const c_void;
        static kCFBooleanTrue: *const c_void;
    }
    unsafe extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    const COMBINED_SESSION: i32 = 0;
    const LEFT: u32 = 0;
    const UTF8: u32 = 0x0800_0100;

    pub fn sample() -> Option<super::Sample> {
        // SAFETY: CGEventCreate(NULL) returns an event we own (or null), released here.
        unsafe {
            let event = CGEventCreate(std::ptr::null());
            if event.is_null() { return None; }
            let p = CGEventGetLocation(event);
            CFRelease(event);
            Some(super::Sample { x: p.x, y: p.y, left: CGEventSourceButtonState(COMBINED_SESSION, LEFT) })
        }
    }

    static HIDDEN: AtomicBool = AtomicBool::new(false);
    static BACKGROUND: AtomicBool = AtomicBool::new(false);

    /// WindowServer ignores cursor changes from an app that isn't frontmost, which an overlay's
    /// app never is. Its private per-connection `SetsCursorInBackground` property lifts that
    /// (Rune's `macos-background-cursor` feature uses the same switch); looked up at run time
    /// and skipped if it's gone.
    fn allow_background_cursor() {
        if BACKGROUND.swap(true, Ordering::Relaxed) { return; }
        const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;
        // SAFETY: both symbols have these C signatures; the key and value outlive the call.
        unsafe {
            let main = dlsym(RTLD_DEFAULT, c"_CGSDefaultConnection".as_ptr());
            let set = dlsym(RTLD_DEFAULT, c"CGSSetConnectionProperty".as_ptr());
            if main.is_null() || set.is_null() { return; }
            let main: extern "C" fn() -> i32 = std::mem::transmute(main);
            let set: extern "C" fn(i32, i32, *const c_void, *const c_void) -> i32 = std::mem::transmute(set);
            let key = CFStringCreateWithCString(std::ptr::null(), c"SetsCursorInBackground".as_ptr(), UTF8);
            if key.is_null() { return; }
            let connection = main();
            set(connection, connection, key, kCFBooleanTrue);
            CFRelease(key);
        }
    }

    pub fn set_cursor_hidden(hidden: bool) {
        if HIDDEN.load(Ordering::Relaxed) == hidden { return; }
        allow_background_cursor();
        HIDDEN.store(hidden, Ordering::Relaxed);
        // SAFETY: plain CoreGraphics calls; hides and shows are kept balanced by HIDDEN.
        unsafe {
            if hidden { CGDisplayHideCursor(0); } else { CGDisplayShowCursor(0); }
        }
    }
}

/// The mouse right now, or `None` where the platform can't say.
pub fn sample() -> Option<Sample> {
    #[cfg(target_os = "macos")]
    { mac::sample() }
    #[cfg(not(target_os = "macos"))]
    { None }
}

/// Hides or shows the system cursor (idempotent).
pub fn set_cursor_hidden(hidden: bool) {
    #[cfg(target_os = "macos")]
    mac::set_cursor_hidden(hidden);
    #[cfg(not(target_os = "macos"))]
    let _ = hidden;
}
