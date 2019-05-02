#![allow(non_snake_case)]
// #![windows_subsystem = "windows"]  // prevent console

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::mem;
use std::ptr::{null, null_mut};
use std::io::Error;
use std::path::PathBuf;

use winapi::Interface;
use winapi::shared::minwindef::*;
use winapi::shared::windef::*;
use winapi::shared::winerror::*;
use winapi::shared::dxgiformat::*;
use winapi::shared::windowsx::*;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::winbase::*;
use winapi::um::winuser::*;
use winapi::um::errhandlingapi::*;
use winapi::um::dcommon::*;
use winapi::um::d2d1::*;
use winapi::um::dwrite::*;
use winapi::um::d2d1::{
    D2D1_SIZE_U,
    D2D1_POINT_2F,
};

mod com_ptr;
mod text_layout;
mod line_gap_buffer;
mod view_state;

use com_ptr::ComPtr;
use view_state::ViewState;

fn win32_string(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

fn create_window(class_name : &str, title : &str) -> Result<HWND, Error> {
    let class_name = win32_string(class_name);
    let title = win32_string(title);
    unsafe {
        let hInstance = GetModuleHandleW(null_mut());
        if hInstance.is_null() {
            Err(Error::last_os_error())?
        }

        let cursor = LoadCursorW(0 as HINSTANCE, IDC_IBEAM);
        if cursor.is_null() {
            Err(Error::last_os_error())?
        }

        let wnd_class = WNDCLASSW {
            style : CS_OWNDC | CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc : Some(my_window_proc),
            lpszClassName : class_name.as_ptr(),
            hInstance,
            cbClsExtra : 0,
            cbWndExtra : 0,
            hIcon: null_mut(),
            hCursor: cursor,
            hbrBackground: null_mut(),
            lpszMenuName: null_mut(),
        };

        let class_atom = RegisterClassW(&wnd_class);
        if class_atom == 0 {
            Err(Error::last_os_error())?
        }

        let handle = CreateWindowExW(
            0,  // dwExStyle
            class_name.as_ptr(),  // lpClassName
            title.as_ptr(),  // lpWindowName
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,  // dwStyle
            CW_USEDEFAULT,  // x
            CW_USEDEFAULT,  // y
            CW_USEDEFAULT,  // nWidth
            CW_USEDEFAULT,  // nHeight
            null_mut(),  // hWndParent
            null_mut(),  // hMenu
            hInstance,  // hInstance
            null_mut(),  // lpParam
        );

        if handle.is_null() {
            Err(Error::last_os_error())
        } else {
            Ok(handle)
        }
    }
}

struct AppState {
    hwnd: HWND,

    resources: Resources,
    view_state: ViewState,

    filename: Option<PathBuf>,
    initially_modified: bool,
}

impl AppState {
    fn new(hwnd: HWND) -> Self {
        let d2d_factory = unsafe {
            let factory_options = D2D1_FACTORY_OPTIONS {
                debugLevel: D2D1_DEBUG_LEVEL_INFORMATION,
            };
            let mut d2d_factory = null_mut();
            let hr = D2D1CreateFactory(
                D2D1_FACTORY_TYPE_SINGLE_THREADED,
                &ID2D1Factory::uuidof(),
                &factory_options as *const D2D1_FACTORY_OPTIONS,
                &mut d2d_factory as *mut _ as *mut *mut _,
            );
            assert!(hr == S_OK, "0x{:x}", hr);
            ComPtr::from_raw(d2d_factory)
        };
        let dwrite_factory = unsafe {
            let mut dwrite_factory = null_mut();
            let hr = DWriteCreateFactory(
                DWRITE_FACTORY_TYPE_SHARED,
                &IDWriteFactory::uuidof(),
                &mut dwrite_factory,
            );
            assert!(hr == S_OK, "0x{:x}", hr);
            ComPtr::from_raw(dwrite_factory as * mut _)
        };

        let resources = Resources::new(hwnd, &d2d_factory, &dwrite_factory);
        // At this point the window is not fully created yet and render target
        // has size 0x0, so we just specify arbitrary size for the view state.
        // It will be changed right away on WM_SIZE.
        let width = 50.0;
        let height = 50.0;
        let view_state = ViewState::new(
            width, height,
            resources.text_format.clone(),
            dwrite_factory.clone(),
        );

        AppState {
            hwnd,
            resources,
            view_state,

            filename: None,
            initially_modified: false,
        }
    }

    pub fn get_title(&self) -> String {
        let mut s = String::new();
        if self.initially_modified || self.view_state.modified {
            s.push_str("* ");
        }
        match &self.filename {
            Some(p) => s.push_str(&p.file_name().unwrap().to_string_lossy()),
            None => s.push_str("untitled"),
        };
        s
    }
}

struct Resources {
    render_target: ComPtr<ID2D1HwndRenderTarget>,
    brush: ComPtr<ID2D1Brush>,
    sel_brush: ComPtr<ID2D1Brush>,
    text_format: ComPtr<IDWriteTextFormat>,
}

impl Resources {
    fn new(
        hwnd: HWND,
        d2d_factory: &ComPtr<ID2D1Factory>,
        dwrite_factory: &ComPtr<IDWriteFactory>,
    ) -> Self {
        let render_target = unsafe {
            let render_properties = D2D1_RENDER_TARGET_PROPERTIES {
                _type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 0.0,
                dpiY: 0.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let mut rc: RECT = mem::uninitialized();
            let res = GetClientRect(hwnd, &mut rc);
            assert!(res != 0);
            let hwnd_render_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd,
                pixelSize: D2D1_SIZE_U {
                    width: (rc.right - rc.left) as u32,
                    height: (rc.bottom - rc.top) as u32,
                },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };
            let mut render_target = null_mut();
            let hr = d2d_factory.CreateHwndRenderTarget(
                &render_properties,
                &hwnd_render_properties,
                &mut render_target,
            );
            assert!(hr == S_OK, "0x{:x}", hr);
            ComPtr::from_raw(render_target)
        };
        let brush = unsafe {
            let c = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
            let mut brush = null_mut();
            let hr = render_target.CreateSolidColorBrush(&c, null(), &mut brush);
            assert!(hr == S_OK, "0x{:x}", hr);
            ComPtr::from_raw(brush)
        };
        let sel_brush = unsafe {
            let c = D2D1_COLOR_F { r: 0.3, g: 0.3, b: 0.4, a: 1.0 };
            let mut brush = null_mut();
            let hr = render_target.CreateSolidColorBrush(&c, null(), &mut brush);
            assert!(hr == S_OK, "0x{:x}", hr);
            ComPtr::from_raw(brush)
        };
        let text_format = unsafe {
            let mut text_format = null_mut();
            let hr = dwrite_factory.CreateTextFormat(
                win32_string("Arial").as_ptr(),
                null_mut(),
                DWRITE_FONT_WEIGHT_REGULAR,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                14.0,
                win32_string("en-us").as_ptr(),
                &mut text_format,
            );
            assert!(hr == S_OK, "0x{:x}", hr);
            ComPtr::from_raw(text_format)
        };
        Resources {
            render_target,
            brush: brush.up(),
            sel_brush: sel_brush.up(),
            text_format,
        }
    }
}

const PADDING_LEFT: f32 = 5.0;

fn paint() {
    let app_state = unsafe { APP_STATE.as_mut().unwrap() };
    let resources = &app_state.resources;
    let view_state = &mut app_state.view_state;
    let rt = &resources.render_target;
    unsafe {
        rt.BeginDraw();
        let c = D2D1_COLOR_F { r: 0.0, b: 0.2, g: 0.0, a: 1.0 };
        rt.Clear(&c);

        let origin = D2D1_POINT_2F {
            x: PADDING_LEFT,
            y: 0.0,
        };
        view_state.render(origin, rt, &resources.brush, &resources.sel_brush);

        let hr = rt.EndDraw(null_mut(), null_mut());
        assert!(hr == S_OK, "0x{:x}", hr);
        // TODO: if hr == D2DERR_RECREATE_TARGET, recreate resources
    }
}

fn get_clipboard(hwnd: HWND) -> String {
    unsafe {
        let res = OpenClipboard(hwnd);
        assert!(res != 0);
        let h = GetClipboardData(CF_UNICODETEXT);
        let pdata = GlobalLock(h) as *mut u16;
        assert!(!pdata.is_null());
        let mut data = Vec::new();
        let mut pos = 0;
        while *pdata.offset(pos) != 0 {
            data.push(*pdata.offset(pos));
            pos += 1;
        }
        let s = OsString::from_wide(&data);
        let s = s.into_string().unwrap();
        let res = GlobalUnlock(pdata as *mut _);
        if res == 0 {
            assert!(GetLastError() == NO_ERROR);
        }
        let res = CloseClipboard();
        assert!(res != 0);
        s.replace("\r\n", "\n")
    }
}

fn set_clipboard(hwnd: HWND, s: &str) {
    let data = win32_string(s);
    unsafe {
        let res = OpenClipboard(hwnd);
        assert!(res != 0);
        let res = EmptyClipboard();
        assert!(res != 0);

        let h = GlobalAlloc(GMEM_MOVEABLE, data.len() * 2);
        assert!(!h.is_null());

        let pdata = GlobalLock(h) as *mut u16;
        assert!(!pdata.is_null());
        for (i, c) in data.into_iter().enumerate() {
            *pdata.add(i) = c;
        }
        let res = GlobalUnlock(pdata as *mut _);
        if res == 0 {
            assert!(GetLastError() == NO_ERROR);
        }

        let res = SetClipboardData(CF_UNICODETEXT, h);
        assert!(!res.is_null());

        let res = CloseClipboard();
        assert!(res != 0);
    }

}

// https://docs.microsoft.com/en-us/windows/desktop/winmsg/window-procedures
unsafe extern "system"
fn my_window_proc(hWnd: HWND, msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            println!("WM_CREATE");
            let mut app_state = AppState::new(hWnd);
            let (filename, content) = match std::env::args().nth(1) {
                Some(arg) => {
                    let path = std::path::PathBuf::from(arg);
                    match std::fs::read_to_string(&path) {
                        Ok(mut content) => {
                            if content.contains('\r') {
                                MessageBoxW(
                                    hWnd,
                                    win32_string("CRLF line breaks were converted to LF").as_ptr(),
                                    win32_string("an editor").as_ptr(),
                                    MB_OK | MB_ICONINFORMATION);
                                content = content.replace('\r', "");
                                app_state.initially_modified = true;
                            }
                            (Some(path), content)
                        }
                        Err(e) => {
                            let msg = format!("Can't open {}.\n{}", path.to_string_lossy(), e);
                            MessageBoxW(
                                hWnd,
                                win32_string(&msg).as_ptr(),
                                win32_string("an editor - error").as_ptr(),
                                MB_OK | MB_ICONERROR);
                            (None, String::new())
                        }
                    }
                }
                None => (None, String::new())
            };
            app_state.filename = filename;
            app_state.view_state.load(&content);
            let res = SetWindowTextW(
                hWnd,
                win32_string(&app_state.get_title()).as_ptr());
            assert!(res != 0);

            APP_STATE = Some(app_state);
            0
        }
        WM_NCDESTROY => {
            println!("WM_NCDESTROY");
            drop(APP_STATE.take());
            PostQuitMessage(0);
            0
        }
        WM_PAINT => {
            println!("WM_PAINT");
            paint();
            let ret = ValidateRect(hWnd, null());
            assert!(ret != 0);
            0
        }
        WM_SIZE => {
            println!("WM_SIZE");
            let app_state = APP_STATE.as_mut().unwrap();
            let resources = &app_state.resources;
            let view_state = &mut app_state.view_state;

            let render_size = D2D_SIZE_U {
                width: GET_X_LPARAM(lParam) as u32,
                height: GET_Y_LPARAM(lParam) as u32,
            };

            if render_size.width == 0 && render_size.height == 0 {
                println!("minimize");
            } else {
                let hr = resources.render_target.Resize(&render_size);
                assert!(hr == S_OK, "0x{:x}", hr);

                let size = resources.render_target.GetSize();
                view_state.resize(size.width - PADDING_LEFT, size.height);
            }
            0
        }
        WM_LBUTTONDOWN => {
            println!("WM_LBUTTONDOWN");
            let app_state = APP_STATE.as_mut().unwrap();

            let x = GET_X_LPARAM(lParam);
            let y = GET_Y_LPARAM(lParam);
            app_state.view_state.click(x as f32 - PADDING_LEFT, y as f32);
            let shift_pressed = GetKeyState(VK_SHIFT) as u16 & 0x8000 != 0;
            if !shift_pressed {
                app_state.view_state.clear_selection();
            }
            InvalidateRect(hWnd, null(), 1);
            0
        }
        WM_MOUSEMOVE => {
            println!("WM_MOUSEMOVE");
            if wParam & MK_LBUTTON != 0 {
                let x = GET_X_LPARAM(lParam);
                let y = GET_Y_LPARAM(lParam);
                let app_state = APP_STATE.as_mut().unwrap();
                app_state.view_state.click(x as f32 - PADDING_LEFT, y as f32);
                InvalidateRect(hWnd, null(), 1);
            }
            0
        }
        WM_MOUSEWHEEL => {
            let delta = GET_WHEEL_DELTA_WPARAM(wParam);
            println!("WM_MOUSEWHEEL {}", delta);
            let mut scroll_lines: UINT = 0;
            let res = SystemParametersInfoW(
                SPI_GETWHEELSCROLLLINES,
                0,
                &mut scroll_lines as *mut _ as *mut _,
                0);
            assert!(res != 0);
            let delta = f32::from(delta) / 120.0 * scroll_lines as f32;
            let app_state = APP_STATE.as_mut().unwrap();
            app_state.view_state.scroll(delta);
            InvalidateRect(hWnd, null(), 1);
            0
        }
        WM_CHAR => {

            let c: char = std::char::from_u32(wParam as u32).unwrap();
            println!("WM_CHAR {:?}", c);
            if wParam >= 32 {
                let app_state = APP_STATE.as_mut().unwrap();
                app_state.view_state.insert_char(c);
                InvalidateRect(hWnd, null(), 1);
                let res = SetWindowTextW(
                    hWnd,
                    win32_string(&app_state.get_title()).as_ptr());
                assert!(res != 0);
            }
            0
        }
        WM_KEYDOWN => {
            let app_state = APP_STATE.as_mut().unwrap();
            let view_state = &mut app_state.view_state;

            println!("WM_KEYDOWN {}", wParam);
            (||{
                let ctrl_pressed = GetKeyState(VK_CONTROL) as u16 & 0x8000 != 0;
                let shift_pressed = GetKeyState(VK_SHIFT) as u16 & 0x8000 != 0;

                if ctrl_pressed {
                    let scan_code = (lParam >> 16) & 511;
                    match scan_code {
                        0x2d => {  // ctrl-X
                            let s = view_state.cut_selection();
                            set_clipboard(hWnd, &s);
                            InvalidateRect(hWnd, null(), 1);
                            return;
                        }
                        0x2e => {  // ctrl-C
                            let s = view_state.get_selection();
                            set_clipboard(hWnd, &s);
                            return;
                        }
                        0x2f => {  // ctrl-V
                            let s = get_clipboard(hWnd);
                            view_state.paste(&s);
                            InvalidateRect(hWnd, null(), 1);
                            return;
                        }
                        _ => {}
                    }
                }

                let mut need_redraw = true;
                let mut regular_movement_cmd = true;
                match wParam as i32 {
                    VK_BACK => {
                        view_state.backspace();
                        regular_movement_cmd = false;
                    }
                    VK_DELETE => {
                        view_state.del();
                        regular_movement_cmd = false;
                    }
                    VK_LEFT =>
                        if ctrl_pressed {
                            view_state.ctrl_left()
                        } else {
                            view_state.left()
                        }
                    VK_RIGHT =>
                        if ctrl_pressed {
                            view_state.ctrl_right()
                        } else {
                            view_state.right()
                        }
                    VK_HOME =>
                        if ctrl_pressed {
                            view_state.ctrl_home()
                        } else {
                            view_state.home()
                        }
                    VK_END =>
                        if ctrl_pressed {
                            view_state.ctrl_end()
                        } else {
                            view_state.end()
                        }
                    VK_UP =>
                        if ctrl_pressed {
                            view_state.scroll(1.0)
                        } else {
                            view_state.up()
                        }
                    VK_DOWN =>
                        if ctrl_pressed  {
                            view_state.scroll(-1.0)
                        } else {
                            view_state.down()
                        }
                    VK_PRIOR => view_state.pg_up(),
                    VK_NEXT => view_state.pg_down(),
                    VK_RETURN => {
                        view_state.insert_char('\n');
                        regular_movement_cmd = false;
                    }
                    _ => {
                        need_redraw = false;
                        regular_movement_cmd = false;
                    }
                };
                if regular_movement_cmd && !shift_pressed {
                    view_state.clear_selection();
                }
                if need_redraw {
                    InvalidateRect(hWnd, null(), 1);
                }
            })();

            let res = SetWindowTextW(
                hWnd,
                win32_string(&app_state.get_title()).as_ptr());
            assert!(res != 0);

            0
        }
        _ => DefWindowProcW(hWnd, msg, wParam, lParam)
    }
}

static mut APP_STATE: Option<AppState> = None;

fn main() -> Result<(), Error> {
    std::panic::set_hook(Box::new(|pi| {
        let payload =
            if let Some(s) = pi.payload().downcast_ref::<&str>() {
                (*s).to_owned()
            } else if let Some(s) = pi.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                String::new()
            };
        let loc = match pi.location() {
            Some(loc) => format!("{}:{}:{}", loc.file(), loc.line(), loc.column()),
            None => "location unknown".to_owned()
        };

        // (anchor:aIMTMDTQfJDYrJxa)
        let exe = std::env::current_exe().unwrap();
        let exe_dir = exe.parent().unwrap();
        std::env::set_current_dir(exe_dir).unwrap();

        let bt = backtrace::Backtrace::new();
        let message = format!("panic {:?}, {}\n{:?}", payload, loc, bt);
        println!("{}", message);
        std::fs::write("error.txt", message).unwrap();

        unsafe {
            let hwnd = match APP_STATE.as_ref() {
                Some(app_state) => app_state.hwnd,
                None => null_mut(),
            };
            MessageBoxW(
                hwnd,
                win32_string("A programming error has occurred.\nDiagnostic info is in 'error.txt'").as_ptr(),
                win32_string("an editor - error").as_ptr(),
                MB_OK | MB_ICONERROR);
        }

        std::process::exit(1);
    }));

    let _hwnd = create_window("an_editor", "window title")?;
    loop {
        unsafe {
            let mut message: MSG = mem::uninitialized();
            let res = GetMessageW(&mut message, null_mut(), 0, 0);
            if res < 0 {
                Err(Error::last_os_error())?
            }
            if res == 0 {  // WM_QUIT
                break
            }
            TranslateMessage(&message as *const MSG);
            DispatchMessageW(&message as *const MSG);
        }
    }
    Ok(())
}
