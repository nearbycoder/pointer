//! The real mouse, wherever it is: the overlays pass every click through, so they never hear
//! the mouse themselves. CoreGraphics reports the cursor and buttons system-wide without any
//! permission prompt. Also hides the system cursor while the glove stands in for it.

use std::time::{Duration, Instant};

/// The mouse in global points (top-left of the main display is the origin, y down).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub x: f64,
    pub y: f64,
    pub left: bool,
}

/// Keeping the system cursor hidden. A hide doesn't stay put: when a click brings another app
/// forward (and sometimes when another app sets its cursor), the window server shows the
/// cursor again, though it still counts our hide. So while the cursor should be hidden, the
/// keeper looks a few times a second and hides it again whenever it's showing. It counts the
/// hides it issued and undoes every one to show the cursor; a show beyond the count changes
/// nothing (the window server's count stops at zero), so a count the window server reset
/// can't leave the cursor hidden.
#[derive(Debug, Default)]
struct Keeper {
    hidden: bool,
    /// Hides issued that no show has undone yet.
    hides: u32,
    checked: Option<Instant>,
    /// Until when to look every time rather than every `CHECK_EVERY`: just after a click,
    /// when another app coming forward shows the cursor.
    eager_until: Option<Instant>,
}

/// What the keeper asks of CoreGraphics: this many shows, then this many hides.
#[derive(Debug, Default, PartialEq)]
struct Calls {
    shows: u32,
    hides: u32,
}

/// How often a hidden cursor is checked, and for how long after a click it's checked at
/// every call (every frame).
const CHECK_EVERY: Duration = Duration::from_millis(50);
const EAGER_FOR: Duration = Duration::from_millis(500);

impl Keeper {
    /// The mouse button went down or up.
    fn click(&mut self, now: Instant) {
        self.eager_until = Some(now + EAGER_FOR);
    }

    /// `visible`: whether the cursor is showing now (`None` if the system can't say), read
    /// only when asked through `look`.
    fn apply(&mut self, hidden: bool, now: Instant, look: impl FnOnce() -> Option<bool>) -> Calls {
        let changed = hidden != self.hidden;
        self.hidden = hidden;
        if !hidden {
            // Undo every hide (none is a no-op).
            return Calls { shows: std::mem::take(&mut self.hides), hides: 0 };
        }
        if self.hides == 0 {
            self.checked = Some(now);
            self.hides = 1;
            return Calls { shows: 0, hides: 1 };
        }
        let eager = self.eager_until.is_some_and(|until| now < until);
        if !changed && !eager && self.checked.is_some_and(|at| now.duration_since(at) < CHECK_EVERY) {
            return Calls::default();
        }
        self.checked = Some(now);
        if look() != Some(true) { return Calls::default(); }
        // Showing in spite of our hide: start the count over from zero, so this hide is the one
        // that hides it whether or not the window server still counted the old one.
        Calls { shows: std::mem::replace(&mut self.hides, 1), hides: 1 }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{c_char, c_void};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    use super::Keeper;

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
    const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

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

    static KEEPER: Mutex<Keeper> = Mutex::new(Keeper { hidden: false, hides: 0, checked: None, eager_until: None });
    static BACKGROUND: AtomicBool = AtomicBool::new(false);

    /// WindowServer ignores cursor changes from an app that isn't frontmost, which an overlay's
    /// app never is. Its private per-connection `SetsCursorInBackground` property lifts that
    /// (Rune's `macos-background-cursor` feature uses the same switch); looked up at run time
    /// and skipped if it's gone.
    fn allow_background_cursor() {
        if BACKGROUND.swap(true, Ordering::Relaxed) { return; }
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

    /// Whether the cursor is showing, wherever it is. `CGCursorIsVisible` is deprecated with no
    /// replacement but still answers; looked up at run time, so if it goes the keeper just
    /// stops checking.
    fn visible() -> Option<bool> {
        static FN: std::sync::OnceLock<Option<extern "C" fn() -> i32>> = std::sync::OnceLock::new();
        let f = FN.get_or_init(|| {
            // SAFETY: CGCursorIsVisible takes nothing and returns a boolean_t.
            unsafe {
                let p = dlsym(RTLD_DEFAULT, c"CGCursorIsVisible".as_ptr());
                (!p.is_null()).then(|| std::mem::transmute::<*mut c_void, extern "C" fn() -> i32>(p))
            }
        });
        f.map(|f| f() != 0)
    }

    pub fn set_cursor_hidden(hidden: bool) {
        let mut keeper = KEEPER.lock().unwrap_or_else(|e| e.into_inner());
        let calls = keeper.apply(hidden, Instant::now(), visible);
        if calls.hides > 0 { allow_background_cursor(); }
        // SAFETY: plain CoreGraphics calls; the keeper keeps hides and shows balanced.
        unsafe {
            for _ in 0..calls.shows { CGDisplayShowCursor(0); }
            for _ in 0..calls.hides { CGDisplayHideCursor(0); }
        }
    }

    pub fn cursor_click() {
        KEEPER.lock().unwrap_or_else(|e| e.into_inner()).click(Instant::now());
    }

    /// Whether the cursor should be hidden but is showing (another app showed it).
    pub fn cursor_escaped() -> bool {
        let hidden = KEEPER.lock().unwrap_or_else(|e| e.into_inner()).hidden;
        hidden && visible() == Some(true)
    }
}

/// The mouse right now, or `None` where the platform can't say.
pub fn sample() -> Option<Sample> {
    #[cfg(target_os = "macos")]
    { mac::sample() }
    #[cfg(not(target_os = "macos"))]
    { None }
}

/// Hides or shows the system cursor. Call it often while the cursor should be hidden: a few
/// times a second it checks that the cursor still is, and hides it again if not.
pub fn set_cursor_hidden(hidden: bool) {
    #[cfg(target_os = "macos")]
    mac::set_cursor_hidden(hidden);
    #[cfg(not(target_os = "macos"))]
    let _ = hidden;
}

/// The mouse button went down or up: a click can bring another app forward, which shows the
/// cursor, so for a moment the keeper looks every frame rather than a few times a second.
pub fn cursor_click() {
    #[cfg(target_os = "macos")]
    mac::cursor_click();
}

/// Whether the cursor should be hidden but something showed it, so the overlay with the
/// cursor should wake and hide it again. Safe from any thread.
pub fn cursor_escaped() -> bool {
    #[cfg(target_os = "macos")]
    { mac::cursor_escaped() }
    #[cfg(not(target_os = "macos"))]
    { false }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_once_then_only_when_something_showed_it() {
        let mut k = Keeper::default();
        let t0 = Instant::now();
        assert_eq!(k.apply(true, t0, || panic!("no need to look yet")), Calls { shows: 0, hides: 1 });
        // Within the check interval it doesn't look at all.
        assert_eq!(k.apply(true, t0 + Duration::from_millis(10), || panic!("too soon")), Calls::default());
        // Still hidden: nothing to do.
        assert_eq!(k.apply(true, t0 + Duration::from_millis(60), || Some(false)), Calls::default());
        // Another app showed it: undo our hide, then hide again from a count of zero.
        assert_eq!(k.apply(true, t0 + Duration::from_millis(120), || Some(true)), Calls { shows: 1, hides: 1 });
        assert_eq!(k.hides, 1);
        // Showing undoes every hide still counted.
        assert_eq!(k.apply(false, t0 + Duration::from_millis(130), || panic!("no need to look")), Calls { shows: 1, hides: 0 });
        assert_eq!(k.apply(false, t0 + Duration::from_millis(140), || None), Calls::default());
    }

    #[test]
    fn after_a_click_it_looks_every_time() {
        let mut k = Keeper::default();
        let t0 = Instant::now();
        k.apply(true, t0, || None);
        k.click(t0 + Duration::from_millis(5));
        let mut looked = false;
        assert_eq!(k.apply(true, t0 + Duration::from_millis(8), || { looked = true; Some(true) }), Calls { shows: 1, hides: 1 });
        assert!(looked);
        // Long after the click it's back to a few times a second.
        k.apply(true, t0 + Duration::from_secs(1), || Some(false));
        assert_eq!(k.apply(true, t0 + Duration::from_millis(1010), || panic!("too soon")), Calls::default());
    }

    #[test]
    fn without_a_way_to_look_it_hides_once() {
        let mut k = Keeper::default();
        let t0 = Instant::now();
        assert_eq!(k.apply(true, t0, || None), Calls { shows: 0, hides: 1 });
        assert_eq!(k.apply(true, t0 + Duration::from_secs(1), || None), Calls::default());
        assert_eq!(k.apply(false, t0 + Duration::from_secs(2), || None), Calls { shows: 1, hides: 0 });
    }
}
