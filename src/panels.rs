//! The overlays' own windows. They stay on screen when the app hides: ⌘H in Settings, an
//! Option-click on another app while Pointer is in front, or another app's Hide Others would
//! otherwise take the glove off every display with them.

/// Marks the overlay window `number` (its CGWindowID) as one the app's hiding leaves on
/// screen. On the main thread, once the window exists (an overlay's `on_created`).
pub fn keep_on_screen(number: u32) {
    #[cfg(target_os = "macos")]
    mac::keep_on_screen(number);
    #[cfg(not(target_os = "macos"))]
    let _ = number;
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    pub fn keep_on_screen(number: u32) {
        let Some(class) = AnyClass::get(c"NSApplication") else { return };
        // SAFETY: AppKit calls on the main thread (`on_created` runs there); the window is one
        // of the app's own, found by its number, and only read and set here.
        unsafe {
            let app: *mut AnyObject = msg_send![class, sharedApplication];
            if app.is_null() { return; }
            let window: *mut AnyObject = msg_send![app, windowWithWindowNumber: number as isize];
            if window.is_null() {
                eprintln!("pointer: overlay window {number} not found; it may hide with the app");
                return;
            }
            let _: () = msg_send![window, setCanHide: false];
        }
    }
}
