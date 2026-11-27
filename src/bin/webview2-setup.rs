//! webview2-setup: puts the WebView2 engine of Dota 2 Mod Manager's own on a Windows machine whose
//! system one is missing or broken, in a window that looks like the app.
//!
//! Dota 2 Mod Manager starts it when its own window cannot open (INTEGRATION.md). It needs nothing
//! of the system that stripped or "optimized" Windows builds take out: no EdgeUpdate, no
//! expand.exe or icacls, no certificate store (TLS with built-in roots), no WebView2 of course -
//! the window is drawn with GDI.
//!
//!   webview2-setup.exe [--base DIR] [--result FILE] [--quiet] [--lang ru|en]
//!
//! --base    where engines live; default %LOCALAPPDATA%\dota2-mod-manager\webview-runtime
//! --result  a JSON file written at the end: {"ok":true,"folder":...,"version":...} or
//!           {"ok":false,"error":...,"cancelled":bool}
//! --quiet   no window
//!
//! Exit code: 0 the engine is there (now or already), 1 it failed, 2 cancelled, 3 bad arguments.
//! The engine's folder is also in <base>\current.json; WebView2 uses it through
//! WEBVIEW2_BROWSER_EXECUTABLE_FOLDER.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;

use webview_runtime::{store, Error, Manifest, Platform, MANIFEST};

struct Args {
    base: PathBuf,
    result: Option<PathBuf>,
    quiet: bool,
    russian: bool,
}

fn default_base() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| ".".into())
        .join("dota2-mod-manager")
        .join("webview-runtime")
}

fn parse() -> Result<Args, String> {
    let mut a = Args { base: default_base(), result: None, quiet: false, russian: system_is_russian() };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--base" => a.base = it.next().ok_or("--base needs a folder")?.into(),
            "--result" => a.result = Some(it.next().ok_or("--result needs a file")?.into()),
            "--quiet" => a.quiet = true,
            "--lang" => a.russian = it.next().ok_or("--lang needs ru or en")? == "ru",
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(a)
}

#[cfg(windows)]
fn system_is_russian() -> bool {
    // primary language of the UI: LANG_RUSSIAN is 0x19
    (unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() } & 0x3ff) == 0x19
}

#[cfg(not(windows))]
fn system_is_russian() -> bool {
    false
}

/// What the run ended with, for the exit code and the result file.
enum Outcome {
    Ready(store::Installed),
    Failed(String),
    Cancelled,
}

fn write_result(path: &Option<PathBuf>, outcome: &Outcome) {
    let Some(path) = path else { return };
    let json = match outcome {
        Outcome::Ready(i) => serde_json::json!({ "ok": true, "folder": i.dir, "version": i.runtime.version }),
        Outcome::Failed(e) => serde_json::json!({ "ok": false, "error": e, "cancelled": false }),
        Outcome::Cancelled => serde_json::json!({ "ok": false, "error": "cancelled", "cancelled": true }),
    };
    let _ = std::fs::write(path, serde_json::to_vec_pretty(&json).unwrap_or_default());
}

fn exit_code(outcome: &Outcome) -> i32 {
    match outcome {
        Outcome::Ready(_) => 0,
        Outcome::Failed(_) => 1,
        Outcome::Cancelled => 2,
    }
}

/// Install (or find) this platform's engine. `progress` gets (done, size) while downloading.
fn run(base: &std::path::Path, progress: &mut dyn FnMut(u64, u64)) -> Outcome {
    let manifest = match Manifest::parse(MANIFEST) {
        Ok(m) => m,
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    let Some(runtime) = manifest.for_platform(Platform::WindowsX64) else {
        return Outcome::Failed("this build knows no Windows engine".into());
    };
    if let Some(have) = store::find(base, runtime) {
        return Outcome::Ready(have);
    }
    match store::install(base, runtime, progress) {
        Ok(i) => Outcome::Ready(i),
        Err(Error::Cancelled) => Outcome::Cancelled,
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

fn main() {
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("webview2-setup: {e}");
            std::process::exit(3);
        }
    };
    let outcome = if args.quiet { run(&args.base, &mut |_, _| {}) } else { gui::show(&args) };
    write_result(&args.result, &outcome);
    std::process::exit(exit_code(&outcome));
}

#[cfg(not(windows))]
mod gui {
    pub fn show(args: &super::Args) -> super::Outcome {
        super::run(&args.base, &mut |_, _| {})
    }
}

#[cfg(windows)]
mod gui {
    //! The window, drawn by hand in Dota 2 Mod Manager's colours: there is no WebView to draw it.

    use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
    use std::sync::Mutex;

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    use super::{run, Args, Outcome};
    use webview_runtime::fetch::CANCEL;

    // the app's palette (its dark theme, the Play button's lavender)
    const BG: u32 = rgb(0x13, 0x10, 0x19);
    const CARD: u32 = rgb(0x1c, 0x18, 0x24);
    const BORDER: u32 = rgb(0x2c, 0x26, 0x36);
    const TEXT: u32 = rgb(0xec, 0xe8, 0xf4);
    const MUTED: u32 = rgb(0x9c, 0x94, 0xab);
    const ACCENT: u32 = rgb(0xc9, 0xb6, 0xff);
    const ACCENT_HOVER: u32 = rgb(0xd8, 0xca, 0xff);
    const ON_ACCENT: u32 = rgb(0x1a, 0x14, 0x24);
    const DANGER: u32 = rgb(0xff, 0x8a, 0x8a);
    const GOOD: u32 = rgb(0x7d, 0xe0, 0x9b);

    const fn rgb(r: u8, g: u8, b: u8) -> u32 {
        r as u32 | (g as u32) << 8 | (b as u32) << 16
    }

    // logical size, scaled by the window's DPI: the window is the card itself, no frame
    const W: i32 = 476;
    const H: i32 = 246;
    const RADIUS: i32 = 16;

    const STAGE_DOWNLOAD: u8 = 0;
    const STAGE_UNPACK: u8 = 1;
    const STAGE_DONE: u8 = 2;
    const STAGE_FAILED: u8 = 3;
    const STAGE_CANCELLED: u8 = 4;
    const STAGE_CANCELLING: u8 = 5;

    const WM_PROGRESS: u32 = WM_APP + 1;
    /// WM_MOUSELEAVE (winuser.h), which windows-sys keeps outside WindowsAndMessaging
    const WM_MOUSELEAVE: u32 = 0x02A3;
    const TIMER_CLOSE: usize = 1;

    static STAGE: AtomicU8 = AtomicU8::new(STAGE_DOWNLOAD);
    /// download progress, in thousandths
    static PERMILLE: AtomicU32 = AtomicU32::new(0);
    static MB_DONE: AtomicU32 = AtomicU32::new(0);
    static MB_TOTAL: AtomicU32 = AtomicU32::new(0);
    static HOVER: AtomicU8 = AtomicU8::new(0);

    struct State {
        outcome: Option<Outcome>,
        error: String,
        russian: bool,
        base: std::path::PathBuf,
    }

    static STATE: Mutex<Option<State>> = Mutex::new(None);

    /// Dota 2 Mod Manager's own icon (its build/icon.ico), for the taskbar and Alt+Tab.
    const ICON: &[u8] = include_bytes!("../../assets/icon.ico");

    /// The image of `ICON` closest to `px` from above, as a Windows icon. An .ico file holds PNG
    /// images here; CreateIconFromResourceEx takes one image's bytes, PNG included.
    unsafe fn icon(px: i32) -> HICON {
        let u16_at = |i: usize| u16::from_le_bytes([ICON[i], ICON[i + 1]]) as usize;
        let u32_at = |i: usize| u32::from_le_bytes([ICON[i], ICON[i + 1], ICON[i + 2], ICON[i + 3]]) as usize;
        let count = u16_at(4);
        let mut best: Option<(i32, usize, usize)> = None; // size, offset, length
        for e in 0..count {
            let at = 6 + 16 * e;
            let size = if ICON[at] == 0 { 256 } else { ICON[at] as i32 };
            let (len, off) = (u32_at(at + 8), u32_at(at + 12));
            let better = match best {
                None => true,
                Some((b, ..)) => (size >= px && (b < px || size < b)) || (b < px && size > b),
            };
            if better && off + len <= ICON.len() {
                best = Some((size, off, len));
            }
        }
        match best {
            Some((_, off, len)) => CreateIconFromResourceEx(ICON[off..].as_ptr(), len as u32, 1, 0x0003_0000, px, px, LR_DEFAULTCOLOR),
            None => std::ptr::null_mut(),
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn t(russian: bool, ru: &'static str, en: &'static str) -> &'static str {
        if russian { ru } else { en }
    }

    pub fn show(args: &Args) -> Outcome {
        *STATE.lock().unwrap() = Some(State { outcome: None, error: String::new(), russian: args.russian, base: args.base.clone() });
        // SAFETY: plain Win32 window setup and message loop on this thread
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = GetModuleHandleW(std::ptr::null());
            let class = wide("D2mmWebView2Setup");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                // CS_DROPSHADOW: a frameless window gets no shadow of its own
                style: CS_HREDRAW | CS_VREDRAW | 0x0002_0000,
                lpfnWndProc: Some(proc_),
                hInstance: instance,
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                hIcon: icon(GetSystemMetrics(SM_CXICON)),
                hIconSm: icon(GetSystemMetrics(SM_CXSMICON)),
                hbrBackground: CreateSolidBrush(BG),
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassExW(&wc);
            let title = wide(t(args.russian, "Dota 2 Mod Manager — движок окна", "Dota 2 Mod Manager — window engine"));
            // no title bar or frame: the card is the whole window, dragged by any part of it
            let style = WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX;
            let hwnd = CreateWindowExW(WS_EX_APPWINDOW, class.as_ptr(), title.as_ptr(), style, CW_USEDEFAULT, CW_USEDEFAULT, W, H,
                std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null());
            if hwnd.is_null() {
                return run(&args.base, &mut |_, _| {});
            }
            size_and_center(hwnd);
            // at this window's DPI, which the class icons above were not sized for
            let dpi = GetDpiForWindow(hwnd).max(96) as i32;
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, icon(32 * dpi / 96) as isize);
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, icon(16 * dpi / 96) as isize);
            ShowWindow(hwnd, SW_SHOW);
            start_worker(hwnd);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        STATE.lock().unwrap().as_mut().and_then(|s| s.outcome.take()).unwrap_or(Outcome::Cancelled)
    }

    unsafe fn size_and_center(hwnd: HWND) {
        let dpi = GetDpiForWindow(hwnd).max(96) as i32;
        let (w, h) = (W * dpi / 96, H * dpi / 96);
        let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        SetWindowPos(hwnd, std::ptr::null_mut(), (sw - w) / 2, (sh - h) / 2, w, h, SWP_NOZORDER);
        round_corners(hwnd, w, h, dpi);
    }

    /// Windows 11 rounds a frameless window itself (DWMWA_WINDOW_CORNER_PREFERENCE, with its
    /// border and smooth edges); Windows 10 does not know the attribute, so the window is cut to
    /// a rounded region there.
    unsafe fn round_corners(hwnd: HWND, w: i32, h: i32, dpi: i32) {
        const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
        const DWMWCP_ROUND: i32 = 2;
        let pref = DWMWCP_ROUND;
        let hr = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, (&pref as *const i32).cast(), 4);
        if hr < 0 {
            let r = RADIUS * 2 * dpi / 96;
            SetWindowRgn(hwnd, CreateRoundRectRgn(0, 0, w + 1, h + 1, r, r), 1);
        }
    }

    fn start_worker(hwnd: HWND) {
        CANCEL.store(false, Ordering::Relaxed);
        STAGE.store(STAGE_DOWNLOAD, Ordering::Relaxed);
        PERMILLE.store(0, Ordering::Relaxed);
        let base = STATE.lock().unwrap().as_ref().map(|s| s.base.clone()).unwrap_or_default();
        let hwnd = hwnd as usize;
        std::thread::spawn(move || {
            let post = |m: u32| unsafe { PostMessageW(hwnd as HWND, m, 0, 0) };
            let mut last = u32::MAX;
            let outcome = run(&base, &mut |done, size| {
                let p = (done * 1000 / size.max(1)) as u32;
                MB_DONE.store((done / 1_000_000) as u32, Ordering::Relaxed);
                MB_TOTAL.store((size / 1_000_000) as u32, Ordering::Relaxed);
                PERMILLE.store(p, Ordering::Relaxed);
                if done == size {
                    STAGE.store(STAGE_UNPACK, Ordering::Relaxed);
                }
                if p != last {
                    last = p;
                    post(WM_PROGRESS);
                }
            });
            let stage = match &outcome {
                Outcome::Ready(_) => STAGE_DONE,
                Outcome::Failed(_) => STAGE_FAILED,
                Outcome::Cancelled => STAGE_CANCELLED,
            };
            if let Some(s) = STATE.lock().unwrap().as_mut() {
                if let Outcome::Failed(e) = &outcome {
                    s.error = e.clone();
                }
                s.outcome = Some(outcome);
            }
            STAGE.store(stage, Ordering::Relaxed);
            post(WM_PROGRESS);
        });
    }

    /// Pixels per logical unit: the monitor's DPI, and how much bigger than its smallest size the
    /// person has made the window (the content grows with it).
    unsafe fn scale(hwnd: HWND) -> f32 {
        let dpi = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let mut c: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut c);
        let zoom = (c.right as f32 / (W as f32 * dpi)).min(c.bottom as f32 / (H as f32 * dpi));
        dpi * zoom.clamp(1.0, 3.0)
    }

    unsafe fn client(hwnd: HWND) -> RECT {
        let mut c: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut c);
        c
    }

    /// The one button, in the bottom right corner: Cancel while working, Retry after a failure,
    /// Close when done.
    unsafe fn button(hwnd: HWND) -> RECT {
        let (k, c) = (scale(hwnd), client(hwnd));
        let s = |v: i32| (v as f32 * k) as i32;
        RECT { left: c.right - s(20 + 132), top: c.bottom - s(20 + 40), right: c.right - s(20), bottom: c.bottom - s(20) }
    }

    /// The cross in the top right corner: the same as closing the window.
    unsafe fn cross(hwnd: HWND) -> RECT {
        let (k, c) = (scale(hwnd), client(hwnd));
        let s = |v: i32| (v as f32 * k) as i32;
        RECT { left: c.right - s(12 + 30), top: s(12), right: c.right - s(12), bottom: s(12 + 30) }
    }

    const HOVER_BUTTON: u8 = 1;
    const HOVER_CROSS: u8 = 2;

    unsafe fn hover_at(hwnd: HWND, x: i32, y: i32) -> u8 {
        if inside(&button(hwnd), x, y) {
            HOVER_BUTTON
        } else if inside(&cross(hwnd), x, y) {
            HOVER_CROSS
        } else {
            0
        }
    }

    fn inside(r: &RECT, x: i32, y: i32) -> bool {
        x >= r.left && x < r.right && y >= r.top && y < r.bottom
    }

    unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_PAINT => {
                paint(hwnd);
                0
            }
            WM_ERASEBKGND => 1,
            WM_NCCALCSIZE if wparam != 0 => 0, // the whole window is client: no visible frame
            WM_NCHITTEST => {
                let mut pt = POINT { x: (lparam & 0xffff) as i16 as i32, y: ((lparam >> 16) & 0xffff) as i16 as i32 };
                windows_sys::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut pt);
                let c = client(hwnd);
                let edge = (6.0 * scale(hwnd)) as i32;
                let (l, r, t, b) = (pt.x < edge, pt.x >= c.right - edge, pt.y < edge, pt.y >= c.bottom - edge);
                let hit = match (l, r, t, b) {
                    (true, _, true, _) => HTTOPLEFT,
                    (_, true, true, _) => HTTOPRIGHT,
                    (true, _, _, true) => HTBOTTOMLEFT,
                    (_, true, _, true) => HTBOTTOMRIGHT,
                    (true, ..) => HTLEFT,
                    (_, true, ..) => HTRIGHT,
                    (_, _, true, _) => HTTOP,
                    (.., true) => HTBOTTOM,
                    _ if hover_at(hwnd, pt.x, pt.y) != 0 => HTCLIENT,
                    _ => HTCAPTION,
                };
                hit as LRESULT
            }
            WM_GETMINMAXINFO => {
                // never smaller than the size it opens at
                let info = &mut *(lparam as *mut MINMAXINFO);
                let dpi = GetDpiForWindow(hwnd).max(96) as i32;
                info.ptMinTrackSize = POINT { x: W * dpi / 96, y: H * dpi / 96 };
                0
            }
            WM_SIZE => {
                let c = client(hwnd);
                round_corners(hwnd, c.right, c.bottom, GetDpiForWindow(hwnd).max(96) as i32);
                InvalidateRect(hwnd, std::ptr::null(), 0);
                0
            }
            WM_KEYDOWN if wparam == 0x1B => {
                // Esc: the same as the button while installing
                if matches!(STAGE.load(Ordering::Relaxed), STAGE_DOWNLOAD | STAGE_UNPACK) {
                    click(hwnd);
                }
                0
            }
            WM_NCMOUSEMOVE => {
                if HOVER.swap(0, Ordering::Relaxed) != 0 {
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                0
            }
            WM_PROGRESS => {
                InvalidateRect(hwnd, std::ptr::null(), 0);
                if STAGE.load(Ordering::Relaxed) == STAGE_DONE {
                    SetTimer(hwnd, TIMER_CLOSE, 900, None);
                }
                if STAGE.load(Ordering::Relaxed) == STAGE_CANCELLED {
                    DestroyWindow(hwnd);
                }
                0
            }
            WM_TIMER if wparam == TIMER_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            WM_MOUSEMOVE => {
                let (x, y) = ((lparam & 0xffff) as i16 as i32, ((lparam >> 16) & 0xffff) as i16 as i32);
                let over = hover_at(hwnd, x, y);
                if HOVER.swap(over, Ordering::Relaxed) != over {
                    let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    TrackMouseEvent(&mut tme);
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                SetCursor(LoadCursorW(std::ptr::null_mut(), if over != 0 { IDC_HAND } else { IDC_ARROW }));
                0
            }
            WM_MOUSELEAVE => {
                HOVER.store(0, Ordering::Relaxed);
                InvalidateRect(hwnd, std::ptr::null(), 0);
                0
            }
            WM_LBUTTONUP => {
                let (x, y) = ((lparam & 0xffff) as i16 as i32, ((lparam >> 16) & 0xffff) as i16 as i32);
                match hover_at(hwnd, x, y) {
                    HOVER_BUTTON => click(hwnd),
                    HOVER_CROSS => {
                        PostMessageW(hwnd, WM_CLOSE, 0, 0);
                    }
                    _ => {}
                }
                0
            }
            WM_CLOSE => {
                match STAGE.load(Ordering::Relaxed) {
                    STAGE_DOWNLOAD | STAGE_UNPACK => click(hwnd),
                    STAGE_CANCELLING => {}
                    _ => {
                        DestroyWindow(hwnd);
                    }
                }
                0
            }
            WM_DPICHANGED => {
                let r = &*(lparam as *const RECT);
                SetWindowPos(hwnd, std::ptr::null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER);
                round_corners(hwnd, r.right - r.left, r.bottom - r.top, GetDpiForWindow(hwnd) as i32);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    unsafe fn click(hwnd: HWND) {
        match STAGE.load(Ordering::Relaxed) {
            STAGE_DOWNLOAD | STAGE_UNPACK => {
                CANCEL.store(true, Ordering::Relaxed);
                STAGE.store(STAGE_CANCELLING, Ordering::Relaxed);
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            STAGE_FAILED => start_worker(hwnd),
            STAGE_DONE => {
                DestroyWindow(hwnd);
            }
            _ => {}
        }
    }

    unsafe fn paint(hwnd: HWND) {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let screen = BeginPaint(hwnd, &mut ps);
        let mut client: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut client);
        let (cw, ch) = (client.right, client.bottom);
        // drawn off screen, then copied in one go: no flicker while the bar moves
        let dc = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, cw, ch);
        let old_bmp = SelectObject(dc, bmp as _);
        let k = scale(hwnd);
        let s = |v: i32| (v as f32 * k) as i32;
        let russian = STATE.lock().unwrap().as_ref().map(|st| st.russian).unwrap_or(false);
        let error = STATE.lock().unwrap().as_ref().map(|st| st.error.clone()).unwrap_or_default();
        let stage = STAGE.load(Ordering::Relaxed);

        // the whole window is the card; its edge is the app's card border
        fill(dc, &client, CARD);
        frame(dc, &client, BORDER);

        // logo: the app's shield, and its name
        shield(dc, s(20), s(18), s(26));
        let bold = font(s(15), 700);
        let regular = font(s(13), 400);
        let heading = font(s(19), 600);
        text(dc, bold, "Dota 2 Mod Manager", RECT { left: s(56), top: s(16), right: cw - s(56), bottom: s(36) }, TEXT, DT_LEFT);
        text(dc, regular, t(russian, "Движок окна · WebView2", "Window engine · WebView2"),
            RECT { left: s(56), top: s(35), right: cw - s(56), bottom: s(54) }, MUTED, DT_LEFT);

        let (title, line, line_color) = match stage {
            STAGE_DOWNLOAD => (
                t(russian, "Ставим движок окна", "Installing the window engine"),
                format!("{} {} / {} {}", t(russian, "Скачивание с сервера Microsoft:", "Downloading from Microsoft:"),
                    MB_DONE.load(Ordering::Relaxed), MB_TOTAL.load(Ordering::Relaxed), t(russian, "МБ", "MB")),
                MUTED,
            ),
            STAGE_UNPACK => (t(russian, "Ставим движок окна", "Installing the window engine"),
                t(russian, "Распаковка и проверка…", "Unpacking and checking…").to_string(), MUTED),
            STAGE_DONE => (t(russian, "Готово", "Done"),
                t(russian, "Mod Manager откроется на своём движке.", "Mod Manager will open on its own engine.").to_string(), GOOD),
            STAGE_CANCELLING => (t(russian, "Отмена…", "Cancelling…"), String::new(), MUTED),
            STAGE_CANCELLED => (t(russian, "Отменено", "Cancelled"), String::new(), MUTED),
            _ => (t(russian, "Не получилось", "It did not work"), error, DANGER),
        };
        text(dc, heading, title, RECT { left: s(20), top: s(74), right: cw - s(20), bottom: s(100) }, TEXT, DT_LEFT);
        let explain = t(russian,
            "Системный WebView2 не найден или сломан. Ставим свой — один раз, около 440 МБ, только для Mod Manager.",
            "The system's WebView2 is missing or broken. Installing one of the app's own: once, about 440 MB, for Mod Manager only.");
        text(dc, regular, explain, RECT { left: s(20), top: s(102), right: cw - s(20), bottom: s(138) }, MUTED, DT_LEFT | DT_WORDBREAK);

        // progress
        let track = RECT { left: s(20), top: s(148), right: cw - s(20), bottom: s(156) };
        round(dc, track, s(8), BORDER, BORDER);
        let frac = match stage {
            STAGE_DOWNLOAD | STAGE_CANCELLING => PERMILLE.load(Ordering::Relaxed) as i32,
            STAGE_UNPACK | STAGE_DONE => 1000,
            _ => 0,
        };
        if frac > 0 {
            let w = (track.right - track.left) * frac / 1000;
            let fill_color = if stage == STAGE_DONE { GOOD } else { ACCENT };
            round(dc, RECT { left: track.left, top: track.top, right: track.left + w.max(s(8)), bottom: track.bottom }, s(8), fill_color, fill_color);
        }
        text(dc, regular, &line, RECT { left: s(20), top: s(164), right: cw - s(20), bottom: s(184) }, line_color, DT_LEFT | DT_END_ELLIPSIS);

        // the button
        // the cross
        let x = cross(hwnd);
        let over_cross = HOVER.load(Ordering::Relaxed) == HOVER_CROSS;
        if over_cross {
            round(dc, x, s(10), BORDER, BORDER);
        }
        let pen = CreatePen(PS_SOLID, s(2).max(1), if over_cross { TEXT } else { MUTED });
        let op = SelectObject(dc, pen as _);
        let (cx, cy, arm) = ((x.left + x.right) / 2, (x.top + x.bottom) / 2, s(5));
        MoveToEx(dc, cx - arm, cy - arm, std::ptr::null_mut());
        LineTo(dc, cx + arm + 1, cy + arm + 1);
        MoveToEx(dc, cx + arm, cy - arm, std::ptr::null_mut());
        LineTo(dc, cx - arm - 1, cy + arm + 1);
        SelectObject(dc, op);
        DeleteObject(pen as _);

        let b = button(hwnd);
        let hover = HOVER.load(Ordering::Relaxed) == HOVER_BUTTON;
        let (label, primary) = match stage {
            STAGE_DOWNLOAD | STAGE_UNPACK => (t(russian, "Отмена", "Cancel"), false),
            STAGE_CANCELLING => (t(russian, "Отмена…", "Cancelling…"), false),
            STAGE_FAILED => (t(russian, "Повторить", "Retry"), true),
            _ => (t(russian, "Закрыть", "Close"), true),
        };
        let (bg, fg) = if primary { (if hover { ACCENT_HOVER } else { ACCENT }, ON_ACCENT) } else { (if hover { BORDER } else { CARD }, TEXT) };
        round(dc, b, s(20), bg, if primary { bg } else { BORDER });
        let label_font = font(s(14), 600);
        text(dc, label_font, label, b, fg, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

        BitBlt(screen, 0, 0, cw, ch, dc, 0, 0, SRCCOPY);
        for f in [bold, regular, heading, label_font] {
            DeleteObject(f as _);
        }
        SelectObject(dc, old_bmp);
        DeleteObject(bmp as _);
        DeleteDC(dc);
        EndPaint(hwnd, &ps);
    }

    unsafe fn frame(dc: HDC, r: &RECT, color: u32) {
        let brush = CreateSolidBrush(color);
        FrameRect(dc, r, brush);
        DeleteObject(brush as _);
    }

    unsafe fn fill(dc: HDC, r: &RECT, color: u32) {
        let brush = CreateSolidBrush(color);
        FillRect(dc, r, brush);
        DeleteObject(brush as _);
    }

    unsafe fn round(dc: HDC, r: RECT, radius: i32, color: u32, border: u32) {
        let brush = CreateSolidBrush(color);
        let pen = CreatePen(PS_SOLID, 1, border);
        let (ob, op) = (SelectObject(dc, brush as _), SelectObject(dc, pen as _));
        RoundRect(dc, r.left, r.top, r.right, r.bottom, radius, radius);
        SelectObject(dc, ob);
        SelectObject(dc, op);
        DeleteObject(brush as _);
        DeleteObject(pen as _);
    }

    /// The app's mark: a shield with a plus, outlined in the accent colour.
    unsafe fn shield(dc: HDC, x: i32, y: i32, size: i32) {
        let p = |fx: f32, fy: f32| POINT { x: x + (fx * size as f32) as i32, y: y + (fy * size as f32) as i32 };
        let outline = [p(0.5, 0.0), p(0.95, 0.16), p(0.92, 0.55), p(0.5, 1.0), p(0.08, 0.55), p(0.05, 0.16)];
        let pen = CreatePen(PS_SOLID, (size / 11).max(2), ACCENT);
        let brush = CreateSolidBrush(CARD);
        let (op, ob) = (SelectObject(dc, pen as _), SelectObject(dc, brush as _));
        Polygon(dc, outline.as_ptr(), outline.len() as i32);
        let plus = [(p(0.5, 0.3), p(0.5, 0.66)), (p(0.32, 0.48), p(0.68, 0.48))];
        for (a, b) in plus {
            MoveToEx(dc, a.x, a.y, std::ptr::null_mut());
            LineTo(dc, b.x, b.y);
        }
        SelectObject(dc, op);
        SelectObject(dc, ob);
        DeleteObject(pen as _);
        DeleteObject(brush as _);
    }

    unsafe fn font(px: i32, weight: i32) -> HFONT {
        let face = wide("Segoe UI");
        CreateFontW(-px, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET as u32, OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32, CLEARTYPE_QUALITY as u32, (DEFAULT_PITCH | FF_SWISS) as u32, face.as_ptr())
    }

    unsafe fn text(dc: HDC, f: HFONT, s: &str, mut r: RECT, color: u32, format: DRAW_TEXT_FORMAT) {
        // DrawTextW read through an empty Vec's dangling pointer and the window crashed when the
        // status line went blank on Cancel; nothing to draw is now nothing to call
        if s.is_empty() {
            return;
        }
        let old = SelectObject(dc, f as _);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color);
        let w = wide(s);
        DrawTextW(dc, w.as_ptr(), -1, &mut r, format | DT_NOPREFIX);
        SelectObject(dc, old);
    }
}
