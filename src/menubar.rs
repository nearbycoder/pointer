//! The menu bar item: a pointing hand with a menu for the things you'd otherwise need a
//! hotkey for. Rune has no macOS status item yet, so this is a little AppKit through objc2:
//! an `NSStatusItem` whose menu items all send `pick:` to one target, which hands the item's
//! tag to the app on its own thread.

use std::sync::OnceLock;

use rune::AppHandle;

use crate::ctl::Ctl;

/// What each menu item does (its tag).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Toggle = 1,
    NextStyle = 2,
    ToolPointer = 10,
    ToolSpotlight = 11,
    ToolLaser = 12,
    ToolPen = 13,
    ToolFocus = 14,
    Settings = 20,
    Updates = 21,
    Quit = 30,
}

impl Pick {
    fn from_tag(tag: isize) -> Option<Pick> {
        [Pick::Toggle, Pick::NextStyle, Pick::ToolPointer, Pick::ToolSpotlight, Pick::ToolLaser, Pick::ToolPen, Pick::ToolFocus, Pick::Settings, Pick::Updates, Pick::Quit]
            .into_iter()
            .find(|p| *p as isize == tag)
    }
}

static HANDLE: OnceLock<AppHandle<Ctl>> = OnceLock::new();

fn picked(tag: isize) {
    if let (Some(handle), Some(pick)) = (HANDLE.get(), Pick::from_tag(tag)) {
        handle.update(move |c, tx| c.menu_pick(pick, tx));
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{AnyThread, class, define_class, msg_send, sel};
    use objc2_foundation::NSString;

    use super::Pick;

    define_class!(
        // SAFETY: NSObject has no subclassing requirements, and the class holds no state and
        // adds no `Drop`.
        #[unsafe(super(NSObject))]
        #[name = "PointerMenuTarget"]
        struct MenuTarget;

        impl MenuTarget {
            // SAFETY: AppKit sends an action `pick:` with the menu item that was chosen.
            #[unsafe(method(pick:))]
            fn pick(&self, sender: *mut AnyObject) {
                if sender.is_null() { return; }
                // SAFETY: the sender is the NSMenuItem that was chosen.
                let tag: isize = unsafe { msg_send![sender, tag] };
                super::picked(tag);
            }
        }
    );

    /// The "Check for Updates…" item, to retitle when an update turns up.
    static UPDATE_ITEM: AtomicUsize = AtomicUsize::new(0);

    fn ns(s: &str) -> Retained<NSString> {
        NSString::from_str(s)
    }

    /// Builds the item. Must run on the main thread.
    pub fn install() {
        // SAFETY: AppKit calls on the main thread (an `AppHandle::update`); every object
        // made here is kept for the life of the process.
        unsafe {
            let target: Retained<MenuTarget> = msg_send![MenuTarget::alloc(), init];
            let target: &'static MenuTarget = Box::leak(Box::new(target));
            let bar: *mut AnyObject = msg_send![class!(NSStatusBar), systemStatusBar];
            if bar.is_null() { return; }
            let Some(item): Option<Retained<AnyObject>> = msg_send![bar, statusItemWithLength: -1.0f64] else { return };
            // The status bar holds its items weakly: keep this one for good.
            let item: *mut AnyObject = Retained::into_raw(item);
            let button: *mut AnyObject = msg_send![item, button];
            let image: *mut AnyObject = msg_send![class!(NSImage), imageWithSystemSymbolName: &*ns("hand.point.up.left.fill"), accessibilityDescription: &*ns("Pointer")];
            if !image.is_null() && !button.is_null() {
                let _: () = msg_send![image, setTemplate: true];
                let _: () = msg_send![button, setImage: image];
            } else if !button.is_null() {
                let _: () = msg_send![button, setTitle: &*ns("☝︎")];
            }
            let menu: Retained<AnyObject> = msg_send![class!(NSMenu), new];
            // The menu holds each item it's given, so a pointer to one stays good as long as the
            // menu, which the status item keeps for the life of the process.
            let add = |title: &str, pick: Option<Pick>, key: &str| -> *mut AnyObject {
                if pick.is_none() {
                    let sep: *mut AnyObject = msg_send![class!(NSMenuItem), separatorItem];
                    let _: () = msg_send![&*menu, addItem: sep];
                    return sep;
                }
                let raw: Allocated<AnyObject> = msg_send![class!(NSMenuItem), alloc];
                let entry: Retained<AnyObject> = msg_send![raw, initWithTitle: &*ns(title), action: sel!(pick:), keyEquivalent: &*ns(key)];
                let _: () = msg_send![&*entry, setTarget: target];
                let _: () = msg_send![&*entry, setTag: pick.map_or(0, |p| p as isize)];
                let _: () = msg_send![&*menu, addItem: &*entry];
                Retained::as_ptr(&entry).cast_mut()
            };
            add("Show or Hide Pointer", Some(Pick::Toggle), "");
            add("Next Pointer", Some(Pick::NextStyle), "");
            add("", None, "");
            add("Pointer", Some(Pick::ToolPointer), "");
            add("Spotlight", Some(Pick::ToolSpotlight), "");
            add("Laser", Some(Pick::ToolLaser), "");
            add("Pen", Some(Pick::ToolPen), "");
            add("Focus", Some(Pick::ToolFocus), "");
            add("", None, "");
            add("Settings…", Some(Pick::Settings), ",");
            let update = add("Check for Updates…", Some(Pick::Updates), "");
            UPDATE_ITEM.store(update as usize, Ordering::Relaxed);
            add("", None, "");
            add("Quit Pointer", Some(Pick::Quit), "q");
            let _: () = msg_send![item, setMenu: &*menu];
        }
    }

    /// Retitles the updates item. Must run on the main thread.
    pub fn set_update(version: Option<&str>) {
        let item = UPDATE_ITEM.load(Ordering::Relaxed) as *mut AnyObject;
        if item.is_null() { return; }
        let title = version.map_or_else(|| "Check for Updates…".to_string(), |v| format!("Update to Pointer {v}…"));
        // SAFETY: the item lives for the process; main thread.
        unsafe { let _: () = msg_send![item, setTitle: &*ns(&title)]; }
    }
}

/// Adds the menu bar item (main thread: call from an `AppHandle::update`).
pub fn install(handle: AppHandle<Ctl>) {
    if HANDLE.set(handle).is_err() { return; }
    #[cfg(target_os = "macos")]
    mac::install();
}

/// Shows an available update in the menu (main thread).
pub fn set_update(version: Option<&str>) {
    #[cfg(target_os = "macos")]
    mac::set_update(version);
    #[cfg(not(target_os = "macos"))]
    let _ = version;
}
