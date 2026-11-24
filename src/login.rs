//! Open at login, through ServiceManagement's `SMAppService` (macOS 13 and later). Only an
//! app bundle can register itself, so a development build reports `None`.

#[cfg(target_os = "macos")]
mod mac {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSString;

    #[link(name = "ServiceManagement", kind = "framework")]
    unsafe extern "C" {}

    fn service() -> Option<*mut AnyObject> {
        let class = AnyClass::get(c"SMAppService")?;
        // SAFETY: `mainAppService` is a class property returning an autoreleased object.
        let service: *mut AnyObject = unsafe { msg_send![class, mainAppService] };
        (!service.is_null()).then_some(service)
    }

    pub fn status() -> Option<bool> {
        if !crate::update::bundled() { return None; }
        let service = service()?;
        // SAFETY: `status` is an NSInteger property (SMAppServiceStatus: 1 is enabled).
        let status: isize = unsafe { msg_send![service, status] };
        Some(status == 1)
    }

    pub fn set(on: bool) -> Result<(), String> {
        let service = service().ok_or("Open at login needs macOS 13 or later")?;
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let out: *mut *mut AnyObject = &mut error;
        // SAFETY: both selectors take an NSError** and return BOOL.
        let ok: bool = unsafe {
            if on { msg_send![service, registerAndReturnError: out] } else { msg_send![service, unregisterAndReturnError: out] }
        };
        if ok { return Ok(()); }
        if error.is_null() { return Err("macOS didn't say why".into()); }
        // SAFETY: a non-null NSError; its description is an NSString it owns.
        let text: *mut NSString = unsafe { msg_send![error, localizedDescription] };
        Err(if text.is_null() { "macOS didn't say why".into() } else { unsafe { (*text).to_string() } })
    }
}

/// Whether Pointer opens at login, or `None` where it can't (a development build).
pub fn status() -> Option<bool> {
    #[cfg(target_os = "macos")]
    { mac::status() }
    #[cfg(not(target_os = "macos"))]
    { None }
}

pub fn set(on: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    { mac::set(on) }
    #[cfg(not(target_os = "macos"))]
    { let _ = on; Err("Only on macOS".into()) }
}
