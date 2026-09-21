//! Native Windows settings window. Workers own data only; all HWND access stays on the UI thread.
#![allow(unsafe_op_in_unsafe_fn)]
use super::io;
use span_core::{DeviceId, DeviceInfo, TrustState};
use std::{
    cell::RefCell,
    ptr,
    sync::mpsc,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, COLOR_WINDOW, CreateFontW, CreateSolidBrush, DC_BRUSH, DC_PEN, DT_CENTER,
        DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FillRect,
        GetStockObject, HFONT, InvalidateRect, PAINTSTRUCT, RoundRect, SelectObject, SetBkMode,
        SetDCBrushColor, SetDCPenColor, SetTextColor, TRANSPARENT,
    },
    System::{
        LibraryLoader::GetModuleHandleW,
        SystemServices::{SS_CENTER, SS_ENDELLIPSIS, SS_RIGHT},
    },
    UI::{
        Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED},
        HiDpi::*,
        Input::KeyboardAndMouse::EnableWindow,
        Shell::{
            NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETFOCUS,
            NIM_SETVERSION, NIN_SELECT, NINF_KEY, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
            Shell_NotifyIconW,
        },
        WindowsAndMessaging::*,
    },
};

const ADD: u16 = 1001;
const REMOVE: u16 = 1002;
const PICKER: u16 = 1003;
const TRAY_OPEN: u16 = 1101;
const TRAY_EXIT: u16 = 1102;
const TIMER: usize = 1;
const APP_ICON_ID: u16 = 101;
const TRAY_ICON_ID: u32 = 1;
const TRAY_MESSAGE: u32 = WM_APP + 1;
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;
const WIDTH: i32 = 540;
const HEIGHT: i32 = 440;
const STYLE: u32 = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextStyle {
    Header,
    Running,
    Hero,
    Lead,
    CardKind,
    CardValue,
    Connection,
    Status,
    Footer,
}

struct Control {
    hwnd: HWND,
    rect: (i32, i32, i32, i32),
    style: TextStyle,
}
struct State {
    controls: Vec<Control>,
    fonts: Vec<HFONT>,
    dpi: u32,
    picker: HWND,
    add: HWND,
    remove: HWND,
    status: HWND,
    title: HWND,
    lead: HWND,
    remote_kind: HWND,
    remote_summary: HWND,
    devices: Vec<DeviceInfo>,
    receiver: Option<mpsc::Receiver<io::Result<Outcome>>>,
    busy: bool,
    has_trusted: bool,
    last_refresh: Instant,
    store_initialized: bool,
    tray_icon: NOTIFYICONDATAW,
    tray_added: bool,
    taskbar_created_message: u32,
}
enum Outcome {
    Discovered(Vec<DeviceInfo>),
    Done(String),
}
thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }

pub fn prompt_pairing(device_id: &str, name: &str, platform: &str) -> io::Result<()> {
    let id = DeviceId::new(device_id.to_owned())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid device id"))?;
    unsafe {
        if confirm(
            ptr::null_mut(),
            &format!("{name}（{platform}）请求连接 Span。\r\n\r\n是否信任此设备并开启剪贴板同步？"),
        ) {
            let mut store =
                crate::trust_store::TrustStore::load(crate::config::trust_store_path()?)?;
            store.trust_existing(&id)?;
            let _ = crate::notify_pairing_accept(&id);
            alert(
                ptr::null_mut(),
                &format!("已信任 {name}，剪贴板同步已开启。"),
                MB_OK,
            );
        }
    }
    Ok(())
}

pub fn open() -> io::Result<()> {
    let start_hidden = std::env::args_os().any(|argument| argument == "--hidden");
    unsafe {
        let existing = FindWindowW(wide("SpanGuiWindow").as_ptr(), ptr::null());
        if !existing.is_null() {
            if !start_hidden {
                show_main_window(existing);
            }
            return Ok(());
        }
    }
    let local = crate::config::load_or_create_local_device()?;
    let autostart_error = crate::autostart::install().err();
    // The installer can replace span.exe while the previous background
    // process is still alive. Restart here so opening the updated UI always
    // activates the matching daemon and protocol implementation.
    let daemon_error = crate::daemon_control::stop_daemon()
        .and_then(|()| {
            std::thread::sleep(Duration::from_millis(150));
            crate::daemon_control::start_daemon()
        })
        .err();
    unsafe {
        // Thread-local context also works if another entry point already set process awareness.
        let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let result = run_window(&local, autostart_error, daemon_error, start_hidden);
        if !previous.is_null() {
            SetThreadDpiAwarenessContext(previous);
        }
        result
    }
}

unsafe fn run_window(
    local: &crate::config::LocalDevice,
    autostart_error: Option<io::Error>,
    daemon_error: Option<io::Error>,
    start_hidden: bool,
) -> io::Result<()> {
    let instance = GetModuleHandleW(ptr::null());
    if instance.is_null() {
        return Err(io::Error::last_os_error());
    }
    let class_name = wide("SpanGuiWindow");
    let existing = FindWindowW(class_name.as_ptr(), ptr::null());
    if !existing.is_null() {
        if !start_hidden {
            show_main_window(existing);
        }
        return Ok(());
    }
    let large_icon = load_app_icon(instance, SM_CXICON, SM_CYICON);
    let small_icon = load_app_icon(instance, SM_CXSMICON, SM_CYSMICON);
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        hbrBackground: (COLOR_WINDOW + 1) as usize as _,
        hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
        hIcon: large_icon,
        ..std::mem::zeroed()
    };
    RegisterClassW(&class);
    let hwnd = CreateWindowExW(
        WS_EX_CONTROLPARENT,
        class_name.as_ptr(),
        wide("Span · 跨设备剪贴板").as_ptr(),
        STYLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        WIDTH,
        HEIGHT,
        ptr::null_mut(),
        ptr::null_mut(),
        instance,
        ptr::null(),
    );
    if hwnd.is_null() {
        return Err(io::Error::last_os_error());
    }
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            controls: vec![],
            fonts: vec![],
            dpi: GetDpiForWindow(hwnd).max(96),
            picker: ptr::null_mut(),
            add: ptr::null_mut(),
            remove: ptr::null_mut(),
            status: ptr::null_mut(),
            title: ptr::null_mut(),
            lead: ptr::null_mut(),
            remote_kind: ptr::null_mut(),
            remote_summary: ptr::null_mut(),
            devices: vec![],
            receiver: None,
            busy: false,
            has_trusted: false,
            last_refresh: Instant::now(),
            store_initialized: false,
            tray_icon: make_tray_icon(hwnd, small_icon),
            tray_added: false,
            taskbar_created_message: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
        })
    });
    restore_tray_icon();
    SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small_icon as LPARAM);
    SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, large_icon as LPARAM);
    if let Err(error) = create_controls(hwnd, local) {
        DestroyWindow(hwnd);
        return Err(error);
    }
    layout(hwnd, None);
    if SetTimer(hwnd, TIMER, 100, None) == 0 {
        let error = io::Error::last_os_error();
        DestroyWindow(hwnd);
        return Err(error);
    }
    if let Err(error) = refresh() {
        set_status(&format!("读取可信设备失败：{error}"));
    }
    if !start_hidden {
        ShowWindow(hwnd, SW_SHOW);
    }
    if let Some(error) = daemon_error {
        set_status("后台同步启动失败，请重新打开 Span 重试。");
        if !start_hidden {
            alert(
                hwnd,
                &format!(
                    "后台同步启动失败，当前无法保证剪贴板同步。\r\n\r\n{error}\r\n\r\n请重新打开 Span 重试。"
                ),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    if let Some(error) = autostart_error {
        if !start_hidden {
            alert(
                hwnd,
                &format!("未能设置开机自启，仍可使用此窗口。\r\n\r\n{error}"),
                MB_OK | MB_ICONWARNING,
            );
        }
    }
    let mut message: MSG = std::mem::zeroed();
    loop {
        match GetMessageW(&mut message, ptr::null_mut(), 0, 0) {
            -1 => {
                let error = io::Error::last_os_error();
                DestroyWindow(hwnd);
                return Err(error);
            }
            0 => break,
            _ => {
                if IsDialogMessageW(hwnd, &message) == 0 {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
    }
    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let taskbar_created_message = STATE.with(|state| {
        state
            .borrow()
            .as_ref()
            .map_or(0, |state| state.taskbar_created_message)
    });
    if taskbar_created_message != 0 && message == taskbar_created_message {
        restore_tray_icon();
        return 0;
    }

    match message {
        WM_COMMAND => {
            let id = (wparam & 0xffff) as u16;
            let code = (wparam >> 16) as u32;
            if id == TRAY_OPEN {
                show_main_window(hwnd);
            } else if id == TRAY_EXIT {
                let busy = STATE.with(|s| s.borrow().as_ref().is_some_and(|s| s.busy));
                if busy {
                    show_main_window(hwnd);
                    set_status("正在处理设备操作，请稍候再退出 Span。");
                } else {
                    DestroyWindow(hwnd);
                }
            } else if id == PICKER && code == CBN_SELCHANGE {
                update_controls();
            } else if code == BN_CLICKED && (id == ADD || id == REMOVE) {
                start_action(hwnd, id);
            }
            0
        }
        TRAY_MESSAGE => {
            let event = (lparam as u32) & 0xffff;
            match event {
                NIN_SELECT | NIN_KEYSELECT | WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                    show_main_window(hwnd);
                }
                WM_CONTEXTMENU | WM_RBUTTONUP => show_tray_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_PAINT => {
            paint_window(hwnd);
            0
        }
        WM_CTLCOLORSTATIC => paint_static(wparam as _, lparam as HWND),
        WM_DRAWITEM => {
            let item = &*(lparam as *const DRAWITEMSTRUCT);
            if item.CtlID == u32::from(ADD) || item.CtlID == u32::from(REMOVE) {
                paint_button(item);
                1
            } else {
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
        }
        WM_TIMER if wparam == TIMER => {
            poll_result(hwnd);
            let should_refresh = STATE.with(|state| {
                let mut state = state.borrow_mut();
                let Some(s) = state.as_mut() else {
                    return false;
                };
                if !s.busy && s.last_refresh.elapsed() >= Duration::from_secs(1) {
                    s.last_refresh = Instant::now();
                    true
                } else {
                    false
                }
            });
            if should_refresh {
                let _ = refresh();
            }
            0
        }
        WM_DPICHANGED => {
            STATE.with(|s| {
                if let Some(s) = s.borrow_mut().as_mut() {
                    s.dpi = (wparam & 0xffff) as u32;
                }
            });
            layout(hwnd, Some(*(lparam as *const RECT)));
            0
        }
        WM_CLOSE => {
            // Keep the window/receiver alive until a committed write has completed.
            let busy = STATE.with(|s| s.borrow().as_ref().is_some_and(|s| s.busy));
            if busy {
                set_status("正在处理设备操作，请稍候再关闭窗口。");
            } else {
                let tray_added = STATE.with(|s| s.borrow().as_ref().is_some_and(|s| s.tray_added));
                if tray_added {
                    ShowWindow(hwnd, SW_HIDE);
                } else {
                    DestroyWindow(hwnd);
                }
            }
            0
        }
        WM_DESTROY => {
            KillTimer(hwnd, TIMER);
            // Dropping the receiver safely discards any late worker result (no HWND or raw payload).
            STATE.with(|s| {
                if let Some(s) = s.borrow_mut().take() {
                    if s.tray_added {
                        Shell_NotifyIconW(NIM_DELETE, &s.tray_icon);
                    }
                    for font in s.fonts {
                        DeleteObject(font);
                    }
                }
            });
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn load_app_icon(
    instance: windows_sys::Win32::Foundation::HINSTANCE,
    cx: i32,
    cy: i32,
) -> HICON {
    let icon = LoadImageW(
        instance,
        APP_ICON_ID as usize as *const u16,
        IMAGE_ICON,
        GetSystemMetrics(cx),
        GetSystemMetrics(cy),
        LR_SHARED,
    ) as HICON;
    if icon.is_null() {
        LoadIconW(ptr::null_mut(), IDI_APPLICATION)
    } else {
        icon
    }
}

fn copy_wide(target: &mut [u16], text: &str) {
    for (slot, value) in target.iter_mut().zip(wide(text)) {
        *slot = value;
    }
}

unsafe fn make_tray_icon(hwnd: HWND, icon: HICON) -> NOTIFYICONDATAW {
    let mut data: NOTIFYICONDATAW = std::mem::zeroed();
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = TRAY_ICON_ID;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    data.uCallbackMessage = TRAY_MESSAGE;
    data.hIcon = icon;
    copy_wide(&mut data.szTip, "Span · 跨设备剪贴板同步中");
    data
}

unsafe fn add_tray_icon(data: &mut NOTIFYICONDATAW) -> bool {
    if Shell_NotifyIconW(NIM_ADD, data) == 0 {
        return false;
    }
    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    Shell_NotifyIconW(NIM_SETVERSION, data);
    true
}

unsafe fn restore_tray_icon() {
    let data = STATE.with(|state| state.borrow().as_ref().map(|state| state.tray_icon));
    let Some(mut data) = data else {
        return;
    };
    let added = add_tray_icon(&mut data);
    STATE.with(|state| {
        if let Some(state) = state.borrow_mut().as_mut() {
            state.tray_icon = data;
            state.tray_added = added;
        }
    });
}

unsafe fn show_main_window(hwnd: HWND) {
    ShowWindow(hwnd, SW_RESTORE);
    SetForegroundWindow(hwnd);
}

unsafe fn show_tray_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    AppendMenuW(
        menu,
        MF_STRING,
        TRAY_OPEN as usize,
        wide("打开 Span").as_ptr(),
    );
    AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
    AppendMenuW(
        menu,
        MF_STRING,
        TRAY_EXIT as usize,
        wide("退出 Span").as_ptr(),
    );
    SetMenuDefaultItem(menu, u32::from(TRAY_OPEN), 0);

    let mut point: POINT = std::mem::zeroed();
    GetCursorPos(&mut point);
    SetForegroundWindow(hwnd);
    let command = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_BOTTOMALIGN,
        point.x,
        point.y,
        0,
        hwnd,
        ptr::null(),
    );
    DestroyMenu(menu);
    PostMessageW(hwnd, WM_NULL, 0, 0);

    let tray_icon = STATE.with(|state| state.borrow().as_ref().map(|state| state.tray_icon));
    if let Some(tray_icon) = tray_icon {
        Shell_NotifyIconW(NIM_SETFOCUS, &tray_icon);
    }
    if command != 0 {
        SendMessageW(hwnd, WM_COMMAND, command as usize, 0);
    }
}

unsafe fn control(
    parent: HWND,
    class: &str,
    text: &str,
    rect: (i32, i32, i32, i32),
    id: u16,
    style: u32,
    text_style: TextStyle,
) -> io::Result<HWND> {
    let hwnd = CreateWindowExW(
        0,
        wide(class).as_ptr(),
        wide(text).as_ptr(),
        WS_CHILD | WS_VISIBLE | style,
        0,
        0,
        0,
        0,
        parent,
        id as usize as _,
        GetModuleHandleW(ptr::null()),
        ptr::null(),
    );
    if hwnd.is_null() {
        return Err(io::Error::last_os_error());
    }
    STATE.with(|s| {
        s.borrow_mut().as_mut().unwrap().controls.push(Control {
            hwnd,
            rect,
            style: text_style,
        })
    });
    Ok(hwnd)
}
unsafe fn create_controls(hwnd: HWND, local: &crate::config::LocalDevice) -> io::Result<()> {
    control(
        hwnd,
        "STATIC",
        "Span",
        (62, 20, 100, 30),
        0,
        0,
        TextStyle::Header,
    )?;
    control(
        hwnd,
        "STATIC",
        "●  同步中",
        (414, 24, 98, 24),
        0,
        SS_RIGHT as u32,
        TextStyle::Running,
    )?;
    let title = control(
        hwnd,
        "STATIC",
        "连接你的设备",
        (28, 72, 484, 34),
        0,
        0,
        TextStyle::Hero,
    )?;
    let lead = control(
        hwnd,
        "STATIC",
        "发现同一网络中的 Span，并建立可信连接。",
        (28, 108, 484, 25),
        0,
        0,
        TextStyle::Lead,
    )?;
    control(
        hwnd,
        "STATIC",
        "这台 Windows",
        (72, 164, 136, 20),
        0,
        0,
        TextStyle::CardKind,
    )?;
    control(
        hwnd,
        "STATIC",
        &local.name,
        (72, 190, 136, 42),
        0,
        SS_ENDELLIPSIS as u32,
        TextStyle::CardValue,
    )?;
    control(
        hwnd,
        "STATIC",
        "⇄",
        (238, 184, 64, 40),
        0,
        SS_CENTER as u32,
        TextStyle::Connection,
    )?;
    let remote_kind = control(
        hwnd,
        "STATIC",
        "等待连接",
        (356, 164, 136, 20),
        0,
        0,
        TextStyle::CardKind,
    )?;
    let remote_summary = control(
        hwnd,
        "STATIC",
        "暂无设备",
        (356, 190, 136, 42),
        0,
        SS_ENDELLIPSIS as u32,
        TextStyle::CardValue,
    )?;
    let status = control(
        hwnd,
        "STATIC",
        "仅向可信设备同步剪贴板。",
        (28, 272, 484, 24),
        0,
        SS_CENTER as u32,
        TextStyle::Status,
    )?;
    let add = control(
        hwnd,
        "BUTTON",
        "添加设备…",
        (176, 306, 188, 36),
        ADD,
        WS_TABSTOP | BS_OWNERDRAW as u32,
        TextStyle::Status,
    )?;
    let picker = control(
        hwnd,
        "COMBOBOX",
        "",
        (96, 354, 258, 180),
        PICKER,
        WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32,
        TextStyle::Status,
    )?;
    let remove = control(
        hwnd,
        "BUTTON",
        "移除",
        (362, 354, 82, 30),
        REMOVE,
        WS_TABSTOP | BS_OWNERDRAW as u32,
        TextStyle::Status,
    )?;
    control(
        hwnd,
        "STATIC",
        "关闭窗口后，后台仍会继续同步。",
        (28, 406, 484, 20),
        0,
        SS_CENTER as u32,
        TextStyle::Footer,
    )?;
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let s = s.as_mut().unwrap();
        s.picker = picker;
        s.add = add;
        s.remove = remove;
        s.status = status;
        s.title = title;
        s.lead = lead;
        s.remote_kind = remote_kind;
        s.remote_summary = remote_summary;
    });
    Ok(())
}
fn scale(value: i32, dpi: u32) -> i32 {
    ((i64::from(value) * i64::from(dpi) + 48) / 96) as i32
}
unsafe fn layout(hwnd: HWND, suggested: Option<RECT>) {
    // Do not hold a RefCell borrow across SetWindowPos (it can dispatch window messages).
    let dpi = STATE.with(|s| s.borrow().as_ref().map_or(96, |s| s.dpi));
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: scale(WIDTH, dpi),
        bottom: scale(HEIGHT, dpi),
    };
    AdjustWindowRectExForDpi(&mut rect, STYLE, 0, WS_EX_CONTROLPARENT, dpi);
    let (x, y, flags) = suggested.map_or((0, 0, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE), |r| {
        (r.left, r.top, SWP_NOZORDER | SWP_NOACTIVATE)
    });
    SetWindowPos(
        hwnd,
        ptr::null_mut(),
        x,
        y,
        rect.right - rect.left,
        rect.bottom - rect.top,
        flags,
    );
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(s) = state.as_mut() else { return };
        let fonts = vec![
            make_font(13, 400, dpi),
            make_font(18, 600, dpi),
            make_font(23, 600, dpi),
            make_font(11, 400, dpi),
            make_font(14, 600, dpi),
            make_font(24, 400, dpi),
        ];
        for c in &s.controls {
            let (x, y, w, h) = c.rect;
            MoveWindow(
                c.hwnd,
                scale(x, dpi),
                scale(y, dpi),
                scale(w, dpi),
                scale(h, dpi),
                1,
            );
            SendMessageW(c.hwnd, WM_SETFONT, fonts[font_index(c.style)] as usize, 1);
        }
        for font in s.fonts.drain(..) {
            DeleteObject(font);
        }
        s.fonts = fonts;
    });
    InvalidateRect(hwnd, ptr::null(), 1);
}

unsafe fn make_font(size: i32, weight: i32, dpi: u32) -> HFONT {
    CreateFontW(
        -scale(size, dpi),
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        1,
        0,
        0,
        5,
        0,
        wide("Microsoft YaHei UI").as_ptr(),
    )
}

fn font_index(style: TextStyle) -> usize {
    match style {
        TextStyle::Header => 1,
        TextStyle::Hero => 2,
        TextStyle::Lead | TextStyle::CardKind | TextStyle::Footer => 3,
        TextStyle::CardValue => 4,
        TextStyle::Connection => 5,
        TextStyle::Running | TextStyle::Status => 0,
    }
}

const fn rgb(red: u32, green: u32, blue: u32) -> u32 {
    red | (green << 8) | (blue << 16)
}

unsafe fn paint_window(hwnd: HWND) {
    let mut paint: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut paint);
    let dpi = STATE.with(|state| state.borrow().as_ref().map_or(96, |s| s.dpi));
    let mut client: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut client);
    let background = CreateSolidBrush(rgb(249, 250, 252));
    FillRect(hdc, &client, background);
    DeleteObject(background);

    let old_brush = SelectObject(hdc, GetStockObject(DC_BRUSH));
    let old_pen = SelectObject(hdc, GetStockObject(DC_PEN));
    SetDCBrushColor(hdc, rgb(244, 246, 249));
    SetDCPenColor(hdc, rgb(228, 232, 238));
    for (x, y, w, h) in [(28, 150, 200, 102), (312, 150, 200, 102)] {
        RoundRect(
            hdc,
            scale(x, dpi),
            scale(y, dpi),
            scale(x + w, dpi),
            scale(y + h, dpi),
            scale(16, dpi),
            scale(16, dpi),
        );
    }

    // App badge and the two device badges are deliberately drawn rather than
    // relying on icon fonts that vary across Windows editions.
    STATE.with(|state| {
        let state = state.borrow();
        let Some(s) = state.as_ref() else {
            return;
        };
        SetDCBrushColor(hdc, rgb(20, 34, 55));
        SetDCPenColor(hdc, rgb(20, 34, 55));
        RoundRect(
            hdc,
            scale(28, dpi),
            scale(18, dpi),
            scale(52, dpi),
            scale(42, dpi),
            scale(8, dpi),
            scale(8, dpi),
        );
        for (x, text) in [(40, "W"), (324, if s.has_trusted { "✓" } else { "+" })] {
            SetDCBrushColor(hdc, rgb(226, 239, 255));
            SetDCPenColor(hdc, rgb(226, 239, 255));
            RoundRect(
                hdc,
                scale(x, dpi),
                scale(180, dpi),
                scale(x + 24, dpi),
                scale(204, dpi),
                scale(12, dpi),
                scale(12, dpi),
            );
            SetBkMode(hdc, TRANSPARENT as i32);
            SetTextColor(hdc, rgb(10, 105, 220));
            if let Some(font) = s.fonts.get(4) {
                SelectObject(hdc, *font);
            }
            let mut rect = RECT {
                left: scale(x, dpi),
                top: scale(180, dpi),
                right: scale(x + 24, dpi),
                bottom: scale(204, dpi),
            };
            let value = wide(text);
            DrawTextW(
                hdc,
                value.as_ptr(),
                -1,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        }
        if let Some(font) = s.fonts.get(4) {
            SelectObject(hdc, *font);
        }
        SetTextColor(hdc, rgb(255, 255, 255));
        let mut logo = RECT {
            left: scale(28, dpi),
            top: scale(18, dpi),
            right: scale(52, dpi),
            bottom: scale(42, dpi),
        };
        let value = wide("N");
        DrawTextW(
            hdc,
            value.as_ptr(),
            -1,
            &mut logo,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        SetDCBrushColor(hdc, rgb(69, 207, 216));
        SetDCPenColor(hdc, rgb(69, 207, 216));
        RoundRect(
            hdc,
            scale(34, dpi),
            scale(34, dpi),
            scale(45, dpi),
            scale(37, dpi),
            scale(3, dpi),
            scale(3, dpi),
        );
    });
    SelectObject(hdc, old_pen);
    SelectObject(hdc, old_brush);
    EndPaint(hwnd, &paint);
}

unsafe fn paint_static(hdc: windows_sys::Win32::Graphics::Gdi::HDC, control: HWND) -> LRESULT {
    SetBkMode(hdc, TRANSPARENT as i32);
    let (style, has_trusted) = STATE.with(|state| {
        let state = state.borrow();
        let Some(s) = state.as_ref() else {
            return (None, false);
        };
        (
            s.controls
                .iter()
                .find(|c| c.hwnd == control)
                .map(|c| c.style),
            s.has_trusted,
        )
    });
    let color = match style {
        Some(TextStyle::Running) => rgb(36, 163, 76),
        Some(TextStyle::Lead | TextStyle::CardKind | TextStyle::Footer) => rgb(112, 118, 128),
        Some(TextStyle::Connection) if has_trusted => rgb(10, 132, 255),
        Some(TextStyle::Connection) => rgb(160, 166, 176),
        Some(TextStyle::Status) => rgb(82, 88, 98),
        _ => rgb(28, 31, 36),
    };
    SetTextColor(hdc, color);
    let background = if matches!(style, Some(TextStyle::CardKind | TextStyle::CardValue)) {
        rgb(244, 246, 249)
    } else {
        rgb(249, 250, 252)
    };
    SetDCBrushColor(hdc, background);
    GetStockObject(DC_BRUSH) as LRESULT
}

unsafe fn paint_button(item: &DRAWITEMSTRUCT) {
    let primary = item.CtlID == u32::from(ADD);
    let disabled = item.itemState & ODS_DISABLED != 0;
    let pressed = item.itemState & ODS_SELECTED != 0;
    let focused = item.itemState & ODS_FOCUS != 0;
    let fill = if primary {
        if disabled {
            rgb(165, 202, 238)
        } else if pressed {
            rgb(0, 98, 205)
        } else {
            rgb(10, 132, 255)
        }
    } else if disabled {
        rgb(239, 241, 244)
    } else if pressed {
        rgb(220, 224, 230)
    } else {
        rgb(232, 235, 240)
    };
    let old_brush = SelectObject(item.hDC, GetStockObject(DC_BRUSH));
    let old_pen = SelectObject(item.hDC, GetStockObject(DC_PEN));
    SetDCBrushColor(item.hDC, fill);
    SetDCPenColor(item.hDC, if focused { rgb(0, 92, 190) } else { fill });
    RoundRect(
        item.hDC,
        item.rcItem.left,
        item.rcItem.top,
        item.rcItem.right,
        item.rcItem.bottom,
        scale(
            10,
            STATE.with(|s| s.borrow().as_ref().map_or(96, |s| s.dpi)),
        ),
        scale(
            10,
            STATE.with(|s| s.borrow().as_ref().map_or(96, |s| s.dpi)),
        ),
    );
    let length = GetWindowTextLengthW(item.hwndItem);
    let mut text = vec![0_u16; usize::try_from(length).unwrap_or(0) + 1];
    GetWindowTextW(item.hwndItem, text.as_mut_ptr(), text.len() as i32);
    SetBkMode(item.hDC, TRANSPARENT as i32);
    SetTextColor(
        item.hDC,
        if primary {
            rgb(255, 255, 255)
        } else if disabled {
            rgb(156, 162, 172)
        } else {
            rgb(45, 49, 56)
        },
    );
    let font = SendMessageW(item.hwndItem, WM_GETFONT, 0, 0);
    if font != 0 {
        SelectObject(item.hDC, font as _);
    }
    let mut rect = item.rcItem;
    DrawTextW(
        item.hDC,
        text.as_ptr(),
        length,
        &mut rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
    SelectObject(item.hDC, old_pen);
    SelectObject(item.hDC, old_brush);
}

unsafe fn refresh() -> io::Result<()> {
    let store = crate::trust_store::TrustStore::load(crate::config::trust_store_path()?)?;
    let devices: Vec<_> = store.trusted_devices().into_iter().cloned().collect();
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let s = state.as_mut().unwrap();
        if s.store_initialized && s.devices == devices {
            return;
        }
        s.store_initialized = true;
        let index = SendMessageW(s.picker, CB_GETCURSEL, 0, 0);
        let selected = usize::try_from(index)
            .ok()
            .and_then(|i| s.devices.get(i))
            .map(|d| d.id.clone());
        SendMessageW(s.picker, CB_RESETCONTENT, 0, 0);
        for device in &devices {
            let text = wide(&format!(
                "{} · {}",
                device.name,
                crate::config::platform_name(device.platform)
            ));
            SendMessageW(s.picker, CB_ADDSTRING, 0, text.as_ptr() as LPARAM);
        }
        let selected_index = devices
            .iter()
            .position(|d| Some(&d.id) == selected.as_ref())
            .unwrap_or(0);
        s.devices = devices;
        if s.devices.is_empty() {
            let empty = wide("暂无可信设备");
            SendMessageW(s.picker, CB_ADDSTRING, 0, empty.as_ptr() as LPARAM);
            SendMessageW(s.picker, CB_SETCURSEL, 0, 0);
        } else {
            SendMessageW(s.picker, CB_SETCURSEL, selected_index, 0);
        }
        let has_trusted = !s.devices.is_empty();
        s.has_trusted = has_trusted;
        SetWindowTextW(
            s.add,
            wide(if !has_trusted {
                "添加设备…"
            } else {
                "添加另一台设备…"
            })
            .as_ptr(),
        );
        SetWindowTextW(
            s.title,
            wide(if has_trusted {
                "已添加可信设备"
            } else {
                "连接你的设备"
            })
            .as_ptr(),
        );
        SetWindowTextW(
            s.lead,
            wide(if has_trusted {
                "剪贴板会在可信设备之间自动同步。"
            } else {
                "发现同一网络中的 Span，并建立可信连接。"
            })
            .as_ptr(),
        );
        SetWindowTextW(
            s.remote_kind,
            wide(if has_trusted {
                "可信设备"
            } else {
                "等待连接"
            })
            .as_ptr(),
        );
        let summary = match s.devices.as_slice() {
            [] => "暂无设备".to_string(),
            [device] => device.name.clone(),
            [first, rest @ ..] => format!("{} 等 {} 台", first.name, rest.len() + 1),
        };
        SetWindowTextW(s.remote_summary, wide(&summary).as_ptr());
        InvalidateRect(GetParent(s.add), ptr::null(), 1);
    });
    update_controls();
    Ok(())
}
unsafe fn update_controls() {
    STATE.with(|state| {
        let state = state.borrow();
        let Some(s) = state.as_ref() else { return };
        let selected = SendMessageW(s.picker, CB_GETCURSEL, 0, 0);
        EnableWindow(s.add, (!s.busy) as i32);
        EnableWindow(s.picker, (!s.busy && !s.devices.is_empty()) as i32);
        EnableWindow(
            s.remove,
            (!s.busy && selected >= 0 && (selected as usize) < s.devices.len()) as i32,
        );
    });
}
unsafe fn set_status(text: &str) {
    STATE.with(|s| {
        if let Some(s) = s.borrow().as_ref() {
            SetWindowTextW(s.status, wide(text).as_ptr());
        }
    });
}
unsafe fn set_busy(busy: bool) {
    STATE.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut() {
            s.busy = busy;
        }
    });
    update_controls();
}
unsafe fn launch(work: impl FnOnce() -> io::Result<Outcome> + Send + 'static) {
    let (sender, receiver) = mpsc::channel();
    STATE.with(|s| s.borrow_mut().as_mut().unwrap().receiver = Some(receiver));
    if let Err(error) = std::thread::Builder::new()
        .name("span-gui-action".into())
        .spawn(move || {
            let _ = sender.send(work());
        })
    {
        STATE.with(|s| s.borrow_mut().as_mut().unwrap().receiver = None);
        set_busy(false);
        set_status(&format!("无法启动操作：{error}"));
    }
}
unsafe fn start_action(hwnd: HWND, id: u16) {
    if STATE.with(|s| s.borrow().as_ref().is_none_or(|s| s.busy)) {
        return;
    }
    if id == ADD {
        set_busy(true);
        set_status("正在发现同一网络中的设备…");
        launch(|| {
            let local = crate::config::load_or_create_local_device()?;
            let devices = crate::scan_devices(&local, Duration::from_millis(700))?;
            Ok(Outcome::Discovered(
                devices
                    .into_iter()
                    .filter(|d| {
                        d.trust_state != TrustState::Trusted
                            && d.trust_state != TrustState::Blocked
                            && d.id != local.id
                    })
                    .collect(),
            ))
        });
    } else {
        // Snapshot the displayed device ID on the UI thread, never re-index a newly loaded store.
        let device = STATE.with(|s| {
            let s = s.borrow();
            let s = s.as_ref().unwrap();
            usize::try_from(SendMessageW(s.picker, CB_GETCURSEL, 0, 0))
                .ok()
                .and_then(|i| s.devices.get(i))
                .cloned()
        });
        let Some(device) = device else { return };
        set_busy(true);
        if !confirm(
            hwnd,
            &format!(
                "移除设备“{}”？\r\n\r\n移除后将停止与此设备同步剪贴板。再次同步需要重新添加并信任。",
                device.name
            ),
        ) {
            set_busy(false);
            set_status("已取消移除。");
            return;
        }
        set_status("正在移除设备…");
        launch(move || {
            let mut store =
                crate::trust_store::TrustStore::load(crate::config::trust_store_path()?)?;
            store.revoke(&device.id)?;
            Ok(Outcome::Done(format!("已移除 {}。", device.name)))
        });
    }
}
unsafe fn poll_result(hwnd: HWND) {
    let result = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let s = state.as_mut()?;
        let result = match s.receiver.as_ref()?.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(io::Error::other("后台操作意外结束，请重试。"))
            }
        };
        s.receiver = None;
        Some(result)
    });
    let Some(result) = result else { return };
    match result {
        Ok(Outcome::Discovered(devices)) => {
            if devices.is_empty() {
                set_status("没有发现新设备。请确认另一台设备已打开 Span，且位于同一网络。");
                set_busy(false);
                return;
            }
            let mut accepted = vec![];
            for device in devices {
                if confirm(
                    hwnd,
                    &format!(
                        "添加设备“{}”（{}）？\r\n\r\n设备标识：{}\r\n\r\n仅信任你认识的设备。信任后将开启剪贴板同步。",
                        device.name,
                        crate::config::platform_name(device.platform),
                        device.id
                    ),
                ) {
                    accepted.push(device.id);
                }
            }
            if accepted.is_empty() {
                set_status("已取消添加，未共享剪贴板内容。");
                set_busy(false);
                return;
            }
            set_status("正在添加可信设备…");
            launch(move || {
                let mut store =
                    crate::trust_store::TrustStore::load(crate::config::trust_store_path()?)?;
                for id in &accepted {
                    store.trust_existing(id)?;
                }
                for id in &accepted {
                    let _ = crate::notify_pairing_accept(id);
                }
                Ok(Outcome::Done(format!(
                    "已添加 {} 台可信设备。",
                    accepted.len()
                )))
            });
        }
        result => {
            let refresh_result = refresh();
            set_busy(false);
            match result {
                Ok(Outcome::Done(text)) => set_status(&text),
                Err(error) => {
                    let text = format!("操作失败：{error}");
                    set_status(&text);
                    alert(hwnd, &text, MB_OK | MB_ICONERROR);
                }
                _ => unreachable!(),
            }
            if let Err(error) = refresh_result {
                set_status(&format!("刷新设备列表失败：{error}"));
            }
        }
    }
}
unsafe fn confirm(hwnd: HWND, text: &str) -> bool {
    // Default to No for both trust and destructive operations.
    alert(hwnd, text, MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2) == IDYES
}
unsafe fn alert(hwnd: HWND, text: &str, flags: u32) -> i32 {
    MessageBoxW(hwnd, wide(text).as_ptr(), wide("Span").as_ptr(), flags)
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dpi_scaling_preserves_logical_layout() {
        assert_eq!(scale(WIDTH, 96), WIDTH);
        assert_eq!(scale(WIDTH, 144), 810);
        assert_eq!(scale(HEIGHT, 192), 880);
    }
    #[test]
    fn windows_text_is_utf16_and_terminated() {
        let text = wide("添加设备…");
        assert_eq!(text.last(), Some(&0));
        assert_eq!(
            String::from_utf16(&text[..text.len() - 1]).unwrap(),
            "添加设备…"
        );
    }
}
