//! What differs between systems: typing into the app in front, which app
//! and window are in front, and the system's privacy switches.
//!
//! Where a system has no implementation yet, the function says so in
//! words rather than failing to build.

use serde::{Deserialize, Serialize};

/// What this system can do, for the Settings page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Support {
    pub os: String,
    /// Words can be typed into other apps.
    pub typing: bool,
    /// The other side of a call can be heard.
    pub system_audio: bool,
    /// A call in a meeting app can be noticed.
    pub meeting_detection: bool,
    /// Typing needs a permission this app does not have yet (macOS).
    pub needs_accessibility: bool,
}

pub fn support() -> Support {
    Support {
        os: std::env::consts::OS.to_string(),
        typing: cfg!(any(target_os = "macos", windows)) || linux_typer().is_some(),
        system_audio: crate::system_audio::available(),
        meeting_detection: cfg!(any(target_os = "macos", windows)),
        needs_accessibility: needs_accessibility(),
    }
}

// ---------------------------------------------------------------- typing

/// Types `text` at the cursor of the app in front. A line break is typed
/// as Shift+Return: in a chat a bare Return sends.
pub fn type_text(text: &str) -> Result<(), String> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    if text.is_empty() {
        return Ok(());
    }
    type_impl(&text)
}

#[cfg(target_os = "macos")]
fn type_impl(text: &str) -> Result<(), String> {
    use objc2_core_graphics::{CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation};
    if !mac::trusted() {
        return Err("To type into other apps, allow this app in System Settings, Privacy and Security, Accessibility.".into());
    }
    // A HID-system source makes the events look like real typing.
    let Some(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
        return Err("Typing could not start. Check the Accessibility permission.".into());
    };
    for c in text.chars() {
        if c == '\n' {
            for key_down in [true, false] {
                let Some(event) = CGEvent::new_keyboard_event(Some(&source), 36, key_down) else { return Err("Typing stopped.".into()) };
                CGEvent::set_flags(Some(&event), CGEventFlags(0x0002_0000)); // Shift
                CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
            }
            continue;
        }
        // One character per event, as a keyboard sends them: a web editor
        // that handles its own keys drops a many-character "key".
        let mut buf = [0u16; 2];
        let units = c.encode_utf16(&mut buf);
        for key_down in [true, false] {
            let Some(event) = CGEvent::new_keyboard_event(Some(&source), 0, key_down) else { return Err("Typing stopped.".into()) };
            // SAFETY: `units` outlives this call and its length is passed.
            unsafe {
                CGEvent::keyboard_set_unicode_string(Some(&event), units.len() as core::ffi::c_ulong, units.as_ptr());
            }
            // No modifiers, whatever keys are physically down: a held
            // shortcut key would otherwise turn letters into shortcuts.
            CGEvent::set_flags(Some(&event), CGEventFlags(0));
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn type_impl(text: &str) -> Result<(), String> {
    if win::type_text(text) {
        Ok(())
    } else {
        Err("Windows did not take the typing. An app run as administrator takes no keys from other apps.".into())
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn type_impl(text: &str) -> Result<(), String> {
    let Some((tool, args)) = linux_typer() else {
        return Err("Typing into other apps needs xdotool (X11) or wtype (Wayland). The words are in the pill to copy.".into());
    };
    let status = std::process::Command::new(tool).args(args).arg(text).status().map_err(|_| "Typing could not start.".to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("Typing did not go through. The words are in the pill to copy.".into())
    }
}

/// The typing tool a Linux desktop has, with its arguments before the text.
#[cfg(not(any(target_os = "macos", windows)))]
fn linux_typer() -> Option<(&'static str, &'static [&'static str])> {
    let on_path = |name: &str| std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()));
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    if wayland && on_path("wtype") {
        return Some(("wtype", &["--"]));
    }
    if on_path("xdotool") {
        return Some(("xdotool", &["type", "--delay", "4", "--"]));
    }
    None
}

#[cfg(any(target_os = "macos", windows))]
fn linux_typer() -> Option<(&'static str, &'static [&'static str])> {
    None
}

// -------------------------------------------------------- the app in front

/// The app in front and the title of its window, for noticing a call.
#[cfg(target_os = "macos")]
pub fn front_window() -> Option<(String, Option<String>)> {
    use objc2_app_kit::NSWorkspace;
    let (name, pid) = objc2::rc::autoreleasepool(|_| {
        let front = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let name = front.localizedName().map(|s| s.to_string()).unwrap_or_default();
        Some((name, front.processIdentifier()))
    })?;
    let title = if mac::trusted() { mac::focused_window_title(pid).filter(|t| !t.trim().is_empty()) } else { None };
    Some((name, title))
}

#[cfg(windows)]
pub fn front_window() -> Option<(String, Option<String>)> {
    win::front_window()
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn front_window() -> Option<(String, Option<String>)> {
    None
}

/// Whether the app in front is holding the microphone: a call on a site
/// no rule knows. macOS only; elsewhere the window title has to say it.
#[cfg(target_os = "macos")]
pub fn front_app_holds_microphone() -> bool {
    use objc2_app_kit::NSWorkspace;
    let bundle = objc2::rc::autoreleasepool(|_| {
        let front = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        front.bundleURL().and_then(|u| u.path()).map(|p| p.to_string())
    });
    let own = std::env::current_exe()
        .ok()
        .and_then(|p| p.ancestors().find(|a| a.extension().is_some_and(|e| e == "app")).map(|a| a.to_string_lossy().to_string()))
        .unwrap_or_default();
    bundle.filter(|b| !b.is_empty()).is_some_and(|b| crate::system_audio_mac::bundle_is_using_microphone(&b, &own))
}

#[cfg(not(target_os = "macos"))]
pub fn front_app_holds_microphone() -> bool {
    false
}

// ------------------------------------------------------------ permissions

#[cfg(target_os = "macos")]
fn needs_accessibility() -> bool {
    !mac::trusted()
}

#[cfg(not(target_os = "macos"))]
fn needs_accessibility() -> bool {
    false
}

/// Asks for the Accessibility permission (macOS shows its own prompt once).
pub fn request_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    {
        mac::trusted_with_prompt()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Opens the system's own page for one privacy switch: "microphone",
/// "accessibility" or "audio". False where the system has no such page.
pub fn open_privacy_settings(pane: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        let anchor = match pane {
            "microphone" => "Privacy_Microphone",
            "accessibility" => "Privacy_Accessibility",
            "audio" => "Privacy_AudioCapture",
            _ => return false,
        };
        let url = format!("x-apple.systempreferences:com.apple.preference.security?{anchor}");
        std::process::Command::new("open").arg(url).status().map(|s| s.success()).unwrap_or(false)
    }
    #[cfg(windows)]
    {
        let page = match pane {
            "microphone" | "audio" => "ms-settings:privacy-microphone",
            _ => return false,
        };
        std::process::Command::new("cmd").args(["/C", "start", "", page]).status().map(|s| s.success()).unwrap_or(false)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = pane;
        false
    }
}

// ------------------------------------------------------------------ macOS

/// The Accessibility calls, made directly: whether this app may read and
/// type into other apps, and the title of the app in front's window.
#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;

    use objc2::rc::Retained;
    use objc2_foundation::{NSDictionary, NSNumber, NSString};

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
        fn AXUIElementCreateApplication(pid: i32) -> *const c_void;
        fn AXUIElementCopyAttributeValue(element: *const c_void, attribute: *const c_void, value: *mut *const c_void) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
        fn CFGetTypeID(cf: *const c_void) -> usize;
        fn CFStringGetTypeID() -> usize;
    }

    pub fn trusted() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    /// Asks, with the system's own prompt the first time.
    pub fn trusted_with_prompt() -> bool {
        let key = NSString::from_str("AXTrustedCheckOptionPrompt");
        let yes = NSNumber::new_bool(true);
        let options: Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> = NSDictionary::from_slices(&[&*key], &[&*yes as &objc2::runtime::AnyObject]);
        // NSDictionary is toll-free bridged to CFDictionary.
        unsafe { AXIsProcessTrustedWithOptions(Retained::as_ptr(&options) as *const c_void) }
    }

    /// One attribute of an element, retained; the caller releases it.
    unsafe fn copy_attribute(element: *const c_void, name: &str) -> Option<*const c_void> {
        let attribute = NSString::from_str(name);
        let mut value: *const c_void = std::ptr::null();
        let status = AXUIElementCopyAttributeValue(element, Retained::as_ptr(&attribute) as *const c_void, &mut value);
        (status == 0 && !value.is_null()).then_some(value)
    }

    pub fn focused_window_title(pid: i32) -> Option<String> {
        unsafe {
            let app = AXUIElementCreateApplication(pid);
            if app.is_null() {
                return None;
            }
            let window = copy_attribute(app, "AXFocusedWindow");
            CFRelease(app);
            let window = window?;
            let title = copy_attribute(window, "AXTitle");
            CFRelease(window);
            let title = title?;
            let text = if CFGetTypeID(title) == CFStringGetTypeID() {
                // CFString is toll-free bridged to NSString.
                Some((*(title as *const NSString)).to_string())
            } else {
                None
            };
            CFRelease(title);
            text
        }
    }
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
mod win {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};

    /// Marks a key press as this app's own.
    const OURS: usize = 0x4A56_4E31;

    const VK_RETURN: u16 = 0x0D;
    const VK_SHIFT: u16 = 0x10;
    const VK_LWIN: u16 = 0x5B;
    const VK_RWIN: u16 = 0x5C;
    const VK_LSHIFT: u16 = 0xA0;
    const VK_RSHIFT: u16 = 0xA1;
    const VK_LCONTROL: u16 = 0xA2;
    const VK_RCONTROL: u16 = 0xA3;
    const VK_LMENU: u16 = 0xA4;
    const VK_RMENU: u16 = 0xA5;
    /// A key no keyboard has. Pressed before Alt or the Windows key is let
    /// go for the person, so letting go opens no menu and no Start.
    const VK_MASK: u16 = 0xE8;

    fn extended(vk: u16) -> bool {
        matches!(vk, VK_RCONTROL | VK_RMENU | VK_LWIN | VK_RWIN | 0x21..=0x28 | 0x2D | 0x2E)
    }

    fn key(vk: u16, up: bool) -> INPUT {
        let mut flags = KEYBD_EVENT_FLAGS(0);
        if up {
            flags |= KEYEVENTF_KEYUP;
        }
        if extended(vk) {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: OURS } } }
    }

    /// One UTF-16 unit as a key that carries the character itself.
    fn unit(u: u16, up: bool) -> INPUT {
        let flags = if up { KEYEVENTF_UNICODE | KEYEVENTF_KEYUP } else { KEYEVENTF_UNICODE };
        INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: u, dwFlags: flags, time: 0, dwExtraInfo: OURS } } }
    }

    fn is_down(vk: u16) -> bool {
        (unsafe { GetAsyncKeyState(vk as i32) } as u16) & 0x8000 != 0
    }

    /// Lets go, for the system only, of any modifier the person still
    /// holds: a character typed while Ctrl or Alt is down is a shortcut.
    fn release_held(out: &mut Vec<INPUT>) {
        let held: Vec<u16> = [VK_LCONTROL, VK_RCONTROL, VK_LSHIFT, VK_RSHIFT, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN].into_iter().filter(|k| is_down(*k)).collect();
        if held.iter().any(|k| matches!(*k, VK_LMENU | VK_RMENU | VK_LWIN | VK_RWIN)) {
            out.push(key(VK_MASK, false));
            out.push(key(VK_MASK, true));
        }
        out.extend(held.into_iter().map(|k| key(k, true)));
    }

    fn send(body: Vec<INPUT>) -> bool {
        if body.is_empty() {
            return true;
        }
        let mut all = Vec::with_capacity(body.len() + 10);
        release_held(&mut all);
        all.extend(body);
        let sent = unsafe { SendInput(&all, std::mem::size_of::<INPUT>() as i32) };
        sent as usize == all.len()
    }

    /// Types `text` at the cursor, each character as its own key.
    pub fn type_text(text: &str) -> bool {
        let mut run: Vec<INPUT> = Vec::new();
        let mut chars = 0usize;
        for c in text.chars() {
            if c == '\n' {
                run.extend([key(VK_SHIFT, false), key(VK_RETURN, false), key(VK_RETURN, true), key(VK_SHIFT, true)]);
            } else {
                // Both halves of a two-unit character go down before
                // either comes up, so the box never sees half of one.
                let mut buf = [0u16; 2];
                let units = c.encode_utf16(&mut buf);
                run.extend(units.iter().map(|u| unit(*u, false)));
                run.extend(units.iter().map(|u| unit(*u, true)));
            }
            chars += 1;
            // Short runs with a breath between: an editor that works on
            // every key falls behind a burst of thousands.
            if chars % 40 == 0 {
                if !send(std::mem::take(&mut run)) {
                    return false;
                }
                std::thread::sleep(std::time::Duration::from_millis(4));
            }
        }
        send(run)
    }

    /// The app's name as people call it, from its program file.
    fn app_name_of(exe_stem: &str) -> String {
        match exe_stem.to_ascii_lowercase().as_str() {
            "chrome" => "Google Chrome".into(),
            "msedge" => "Microsoft Edge".into(),
            "firefox" => "Firefox".into(),
            "brave" => "Brave Browser".into(),
            "arc" => "Arc".into(),
            "zoom" => "zoom.us".into(),
            "ms-teams" | "teams" => "Microsoft Teams".into(),
            "webex" | "ciscocollabhost" => "Webex".into(),
            _ => exe_stem.to_string(),
        }
    }

    pub fn front_window() -> Option<(String, Option<String>)> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return None;
            }
            let mut buf = [0u16; 512];
            let n = GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let mut name = String::new();
            if pid != 0 {
                if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    let mut path = [0u16; 1024];
                    let mut len = path.len() as u32;
                    if QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(path.as_mut_ptr()), &mut len).is_ok() {
                        let full = String::from_utf16_lossy(&path[..len as usize]);
                        let stem = std::path::Path::new(&full).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                        name = app_name_of(&stem);
                    }
                    let _ = CloseHandle(process);
                }
            }
            Some((name, Some(title).filter(|t| !t.trim().is_empty())))
        }
    }
}
