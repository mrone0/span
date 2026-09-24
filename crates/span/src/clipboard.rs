use std::io;
#[cfg(target_os = "windows")]
use std::sync::mpsc::{self, Receiver, Sender};
#[cfg(target_os = "windows")]
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub trait Clipboard {
    fn change_count(&mut self) -> io::Result<Option<u64>> {
        Ok(None)
    }

    fn wait_for_change(&mut self, timeout: Duration) -> io::Result<bool> {
        std::thread::sleep(timeout);
        Ok(false)
    }

    fn read_text(&mut self) -> io::Result<Option<String>>;
    fn write_text(&mut self, text: &str) -> io::Result<()>;
}

pub fn system_clipboard() -> Box<dyn Clipboard> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacOsClipboard)
    }

    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsClipboard::new())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Box::new(UnsupportedClipboard::new("linux native adapter pending"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        Box::new(UnsupportedClipboard::new("unsupported platform"))
    }
}

#[cfg(target_os = "windows")]
struct WindowsClipboard {
    listener: Option<WindowsClipboardListener>,
}

#[cfg(target_os = "windows")]
struct WindowsClipboardListener {
    receiver: Receiver<()>,
}

#[cfg(target_os = "windows")]
static WINDOWS_CLIPBOARD_UPDATE_SENDER: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();

#[cfg(target_os = "windows")]
impl WindowsClipboard {
    fn new() -> Self {
        Self { listener: None }
    }
}

#[cfg(target_os = "windows")]
impl WindowsClipboardListener {
    fn start() -> Self {
        let (sender, receiver) = mpsc::channel();
        let sender_slot = WINDOWS_CLIPBOARD_UPDATE_SENDER.get_or_init(|| Mutex::new(None));
        *sender_slot.lock().expect("clipboard listener lock") = Some(sender);

        std::thread::spawn(move || {
            if let Err(error) = windows_clipboard_event_loop() {
                eprintln!("windows clipboard listener stopped: {error}");
            }
        });

        Self { receiver }
    }
}

#[cfg(target_os = "windows")]
impl Clipboard for WindowsClipboard {
    fn change_count(&mut self) -> io::Result<Option<u64>> {
        use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;

        let sequence = unsafe { GetClipboardSequenceNumber() };
        Ok((sequence != 0).then_some(u64::from(sequence)))
    }

    fn wait_for_change(&mut self, timeout: Duration) -> io::Result<bool> {
        let listener = self
            .listener
            .get_or_insert_with(WindowsClipboardListener::start);
        match listener.receiver.recv_timeout(timeout) {
            Ok(()) => Ok(true),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(false),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "clipboard listener disconnected",
            )),
        }
    }

    fn read_text(&mut self) -> io::Result<Option<String>> {
        windows_read_text()
    }

    fn write_text(&mut self, text: &str) -> io::Result<()> {
        windows_write_text(text)
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn windows_clipboard_window_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{DefWindowProcW, WM_CLIPBOARDUPDATE};

    if msg == WM_CLIPBOARDUPDATE {
        if let Some(slot) = WINDOWS_CLIPBOARD_UPDATE_SENDER.get() {
            if let Ok(guard) = slot.lock() {
                if let Some(sender) = guard.as_ref() {
                    let _ = sender.send(());
                }
            }
        }
        return 0;
    }

    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(target_os = "windows")]
fn windows_clipboard_event_loop() -> io::Result<()> {
    use std::ptr;

    use windows_sys::Win32::Foundation::{HINSTANCE, HWND};
    use windows_sys::Win32::System::DataExchange::AddClipboardFormatListener;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DispatchMessageW, GetMessageW,
        HWND_MESSAGE, MSG, RegisterClassW, TranslateMessage, WNDCLASSW,
    };

    let class_name = wide_string("span_clipboard_listener");

    unsafe {
        let hinstance = GetModuleHandleW(ptr::null()) as HINSTANCE;
        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(windows_clipboard_window_proc),
            hInstance: hinstance,
            lpszClassName: class_name.as_ptr(),
            ..std::mem::zeroed()
        };

        if RegisterClassW(&wnd_class) == 0 {
            return Err(io::Error::last_os_error());
        }

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            HWND_MESSAGE,
            ptr::null_mut(),
            hinstance,
            ptr::null_mut(),
        );

        if hwnd == 0 as HWND {
            return Err(io::Error::last_os_error());
        }

        if AddClipboardFormatListener(hwnd) == 0 {
            return Err(io::Error::last_os_error());
        }

        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, 0 as HWND, 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn wide_string(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(target_os = "windows")]
fn windows_read_text() -> io::Result<Option<String>> {
    use std::ffi::c_void;

    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

    unsafe {
        if IsClipboardFormatAvailable(u32::from(CF_UNICODETEXT)) == 0 {
            return Ok(None);
        }
        windows_open_clipboard_with_retry()?;

        let result = (|| {
            let handle = GetClipboardData(u32::from(CF_UNICODETEXT));
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }

            let pointer = GlobalLock(handle) as *const u16;
            if pointer.is_null() {
                return Err(io::Error::last_os_error());
            }

            let mut len = 0;
            while *pointer.add(len) != 0 {
                len += 1;
            }

            let slice = std::slice::from_raw_parts(pointer, len);
            let text = String::from_utf16_lossy(slice);
            GlobalUnlock(handle as *mut c_void);

            Ok((!text.is_empty()).then_some(text))
        })();

        CloseClipboard();
        result
    }
}

#[cfg(target_os = "windows")]
fn windows_write_text(text: &str) -> io::Result<()> {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    };
    use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

    let mut utf16 = text.encode_utf16().collect::<Vec<u16>>();
    utf16.push(0);
    let byte_len = utf16.len() * std::mem::size_of::<u16>();

    unsafe {
        // Prepare the replacement before emptying the system clipboard. An
        // allocation or encoding failure must leave the user's current Copy
        // operation untouched.
        let handle = GlobalAlloc(GMEM_MOVEABLE, byte_len);
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }

        let pointer = GlobalLock(handle) as *mut u16;
        if pointer.is_null() {
            let error = io::Error::last_os_error();
            GlobalFree(handle);
            return Err(error);
        }

        std::ptr::copy_nonoverlapping(utf16.as_ptr(), pointer, utf16.len());
        GlobalUnlock(handle);

        if let Err(error) = windows_open_clipboard_with_retry() {
            GlobalFree(handle);
            return Err(error);
        }

        let mut ownership_transferred = false;
        let result = (|| {
            if EmptyClipboard() == 0 {
                return Err(io::Error::last_os_error());
            }

            if SetClipboardData(u32::from(CF_UNICODETEXT), handle).is_null() {
                return Err(io::Error::last_os_error());
            }

            ownership_transferred = true;
            Ok(())
        })();

        CloseClipboard();
        if !ownership_transferred {
            GlobalFree(handle);
        }
        result
    }
}

#[cfg(target_os = "windows")]
fn windows_open_clipboard_with_retry() -> io::Result<()> {
    use windows_sys::Win32::System::DataExchange::OpenClipboard;

    let mut last_error = None;
    for attempt in 0..8_u64 {
        if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
            return Ok(());
        }
        last_error = Some(io::Error::last_os_error());
        if attempt < 7 {
            // Copy providers such as Office and browsers can own the clipboard
            // briefly while rendering formats. Back off instead of fighting
            // that normal operation or dropping the synchronized update.
            std::thread::sleep(Duration::from_millis(5 * (attempt + 1)));
        }
    }

    Err(last_error.unwrap_or_else(|| io::Error::other("clipboard unavailable")))
}

#[cfg(target_os = "macos")]
struct MacOsClipboard;

#[cfg(target_os = "macos")]
impl Clipboard for MacOsClipboard {
    fn change_count(&mut self) -> io::Result<Option<u64>> {
        macos_change_count().map(Some)
    }

    fn read_text(&mut self) -> io::Result<Option<String>> {
        macos_read_text()
    }

    fn write_text(&mut self, text: &str) -> io::Result<()> {
        macos_write_text(text)
    }
}

#[cfg(target_os = "macos")]
type ObjcId = *mut std::ffi::c_void;

#[cfg(target_os = "macos")]
type Sel = *mut std::ffi::c_void;

#[cfg(target_os = "macos")]
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    #[link_name = "NSPasteboardTypeString"]
    static NSPASTEBOARD_TYPE_STRING: ObjcId;
}

#[cfg(target_os = "macos")]
#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const std::ffi::c_char) -> ObjcId;
    fn sel_registerName(name: *const std::ffi::c_char) -> Sel;
    fn objc_autoreleasePoolPush() -> *mut std::ffi::c_void;
    fn objc_autoreleasePoolPop(pool: *mut std::ffi::c_void);

    fn objc_msgSend();
}

#[cfg(target_os = "macos")]
fn macos_change_count() -> io::Result<u64> {
    unsafe {
        with_autorelease_pool(|| {
            let pasteboard = general_pasteboard()?;
            Ok(send_u64(pasteboard, selector("changeCount")?))
        })
    }
}

#[cfg(target_os = "macos")]
fn macos_read_text() -> io::Result<Option<String>> {
    unsafe {
        with_autorelease_pool(|| {
            let pasteboard = general_pasteboard()?;
            let ns_string = send_id_id(
                pasteboard,
                selector("stringForType:")?,
                NSPASTEBOARD_TYPE_STRING,
            );
            if ns_string.is_null() {
                return Ok(None);
            }

            let c_string = send_id(ns_string, selector("UTF8String")?) as *const std::ffi::c_char;
            if c_string.is_null() {
                return Ok(None);
            }

            let text = std::ffi::CStr::from_ptr(c_string)
                .to_string_lossy()
                .into_owned();
            Ok((!text.is_empty()).then_some(text))
        })
    }
}

#[cfg(target_os = "macos")]
fn macos_write_text(text: &str) -> io::Result<()> {
    let text = std::ffi::CString::new(text)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "text contains NUL byte"))?;

    unsafe {
        with_autorelease_pool(|| {
            let pasteboard = general_pasteboard()?;
            send_u64(pasteboard, selector("clearContents")?);

            let ns_string = send_id_cstr(
                class("NSString")?,
                selector("stringWithUTF8String:")?,
                text.as_ptr(),
            );
            if ns_string.is_null() {
                return Err(io::Error::other("NSString allocation failed"));
            }

            if send_bool_id_id(
                pasteboard,
                selector("setString:forType:")?,
                ns_string,
                NSPASTEBOARD_TYPE_STRING,
            ) == 0
            {
                Err(io::Error::other("NSPasteboard write failed"))
            } else {
                Ok(())
            }
        })
    }
}

#[cfg(target_os = "macos")]
unsafe fn send_id(receiver: ObjcId, selector: Sel) -> ObjcId {
    let function: unsafe extern "C" fn(ObjcId, Sel) -> ObjcId =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { function(receiver, selector) }
}

#[cfg(target_os = "macos")]
unsafe fn send_u64(receiver: ObjcId, selector: Sel) -> u64 {
    let function: unsafe extern "C" fn(ObjcId, Sel) -> u64 =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { function(receiver, selector) }
}

#[cfg(target_os = "macos")]
unsafe fn send_id_id(receiver: ObjcId, selector: Sel, arg: ObjcId) -> ObjcId {
    let function: unsafe extern "C" fn(ObjcId, Sel, ObjcId) -> ObjcId =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { function(receiver, selector, arg) }
}

#[cfg(target_os = "macos")]
unsafe fn send_bool_id_id(receiver: ObjcId, selector: Sel, arg1: ObjcId, arg2: ObjcId) -> i8 {
    let function: unsafe extern "C" fn(ObjcId, Sel, ObjcId, ObjcId) -> i8 =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { function(receiver, selector, arg1, arg2) }
}

#[cfg(target_os = "macos")]
unsafe fn send_id_cstr(receiver: ObjcId, selector: Sel, arg: *const std::ffi::c_char) -> ObjcId {
    let function: unsafe extern "C" fn(ObjcId, Sel, *const std::ffi::c_char) -> ObjcId =
        unsafe { std::mem::transmute(objc_msgSend as *const ()) };
    unsafe { function(receiver, selector, arg) }
}

#[cfg(target_os = "macos")]
unsafe fn with_autorelease_pool<T>(operation: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let pool = unsafe { objc_autoreleasePoolPush() };
    let result = operation();
    unsafe { objc_autoreleasePoolPop(pool) };
    result
}

#[cfg(target_os = "macos")]
fn general_pasteboard() -> io::Result<ObjcId> {
    unsafe {
        let pasteboard = send_id(class("NSPasteboard")?, selector("generalPasteboard")?);
        if pasteboard.is_null() {
            Err(io::Error::other("NSPasteboard unavailable"))
        } else {
            Ok(pasteboard)
        }
    }
}

#[cfg(target_os = "macos")]
fn class(name: &str) -> io::Result<ObjcId> {
    let name = std::ffi::CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid class name"))?;
    let class = unsafe { objc_getClass(name.as_ptr()) };
    if class.is_null() {
        Err(io::Error::other("Objective-C class not found"))
    } else {
        Ok(class)
    }
}

#[cfg(target_os = "macos")]
fn selector(name: &str) -> io::Result<Sel> {
    let name = std::ffi::CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid selector name"))?;
    let selector = unsafe { sel_registerName(name.as_ptr()) };
    if selector.is_null() {
        Err(io::Error::other("Objective-C selector not found"))
    } else {
        Ok(selector)
    }
}

#[cfg(any(
    all(unix, not(target_os = "macos")),
    not(any(target_os = "macos", target_os = "windows", unix))
))]
struct UnsupportedClipboard {
    reason: &'static str,
}

#[cfg(any(
    all(unix, not(target_os = "macos")),
    not(any(target_os = "macos", target_os = "windows", unix))
))]
impl UnsupportedClipboard {
    fn new(reason: &'static str) -> Self {
        Self { reason }
    }
}

#[cfg(any(
    all(unix, not(target_os = "macos")),
    not(any(target_os = "macos", target_os = "windows", unix))
))]
impl Clipboard for UnsupportedClipboard {
    fn read_text(&mut self) -> io::Result<Option<String>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, self.reason))
    }

    fn write_text(&mut self, _text: &str) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, self.reason))
    }
}
