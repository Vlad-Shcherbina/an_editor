#![allow(non_snake_case)]
// #![windows_subsystem = "windows"]  // prevent console

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::mem;
use std::ptr::{null, null_mut};
use std::io::Error;

use winapi::Interface;
use winapi::shared::minwindef::*;
use winapi::shared::windef::*;
use winapi::shared::winerror::*;
use winapi::shared::dxgiformat::*;
use winapi::shared::windowsx::*;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::winuser::*;
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
    d2d_factory: ComPtr<ID2D1Factory>,
    dwrite_factory: ComPtr<IDWriteFactory>,
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
        AppState {
            hwnd,
            d2d_factory,
            dwrite_factory,
        }
    }
}

struct Resources {
    render_target: ComPtr<ID2D1HwndRenderTarget>,
    brush: ComPtr<ID2D1Brush>,
    sel_brush: ComPtr<ID2D1Brush>,
    text_format: ComPtr<IDWriteTextFormat>,
}

impl Resources {
    fn new(app_state: &AppState) -> Self {
        println!("Resources::new()");
        let render_target = unsafe {
            let mut rc: RECT = mem::uninitialized();
            GetClientRect(app_state.hwnd, &mut rc);
            println!("client rect {:?}", rc);

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
            let hwnd_render_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: app_state.hwnd,
                pixelSize: D2D1_SIZE_U {
                    width: (rc.right - rc.left) as u32,
                    height: (rc.bottom - rc.top) as u32,
                },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };
            let mut render_target = null_mut();
            let hr = app_state.d2d_factory.CreateHwndRenderTarget(
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
            let hr = app_state.dwrite_factory.CreateTextFormat(
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
    let resources = unsafe { RESOURCES.as_ref().unwrap() };
    let view_state = unsafe { VIEW_STATE.as_mut().unwrap() };
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

// https://docs.microsoft.com/en-us/windows/desktop/winmsg/window-procedures
unsafe extern "system"
fn my_window_proc(hWnd: HWND, msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT {
    match msg {
        WM_DESTROY => {
            println!("WM_DESTROY");
            drop(APP_STATE.take());
            drop(RESOURCES.take());
            drop(VIEW_STATE.take());
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
        WM_CREATE => {
            println!("WM_CREATE");
            let app_state = AppState::new(hWnd);
            let resources = Resources::new(&app_state);
            let size = resources.render_target.GetSize();
            let view_state = ViewState::new(
                size.width - PADDING_LEFT,
                size.height,
                resources.text_format.clone(),
                app_state.dwrite_factory.clone(),
            );
            APP_STATE = Some(app_state);
            RESOURCES = Some(resources);
            VIEW_STATE = Some(view_state);
            0
        }
        WM_SIZE => {
            println!("WM_SIZE");
            let render_size = D2D_SIZE_U {
                width: GET_X_LPARAM(lParam) as u32,
                height: GET_Y_LPARAM(lParam) as u32,
            };

            if render_size.width == 0 && render_size.height == 0 {
                println!("minimize");
            } else {
                let resources = RESOURCES.as_ref().unwrap();
                let hr = resources.render_target.Resize(&render_size);
                assert!(hr == S_OK, "0x{:x}", hr);

                let size = resources.render_target.GetSize();
                let view_state = VIEW_STATE.as_mut().unwrap();
                view_state.resize(size.width - PADDING_LEFT, size.height);
            }
            0
        }
        WM_LBUTTONDOWN => {
            println!("WM_LBUTTONDOWN");
            let x = GET_X_LPARAM(lParam);
            let y = GET_Y_LPARAM(lParam);
            let view_state = VIEW_STATE.as_mut().unwrap();
            view_state.click(x as f32 - PADDING_LEFT, y as f32);
            let shift_pressed = GetKeyState(VK_SHIFT) as u16 & 0x8000 != 0;
            if !shift_pressed {
                view_state.clear_selection();
            }
            InvalidateRect(hWnd, null(), 1);
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
            let delta = delta as f32 / 120.0 * scroll_lines as f32;
            let view_state = VIEW_STATE.as_mut().unwrap();
            view_state.scroll(delta);
            InvalidateRect(hWnd, null(), 1);
            0
        }
        WM_CHAR => {
            let c: char = std::char::from_u32(wParam as u32).unwrap();
            println!("WM_CHAR {:?}", c);
            if wParam >= 32 {
                let view_state = VIEW_STATE.as_mut().unwrap();
                view_state.insert_char(c);
                InvalidateRect(hWnd, null(), 1);
            }
            0
        }
        WM_KEYDOWN => {
            println!("WM_KEYDOWN {}", wParam);
            let view_state = VIEW_STATE.as_mut().unwrap();
            let ctrl_pressed = GetKeyState(VK_CONTROL) as u16 & 0x8000 != 0;
            let shift_pressed = GetKeyState(VK_SHIFT) as u16 & 0x8000 != 0;
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
            if regular_movement_cmd {
                if !shift_pressed {
                    view_state.clear_selection();
                }
            }
            if need_redraw {
                InvalidateRect(hWnd, null(), 1);
            }
            0
        }
        _ => DefWindowProcW(hWnd, msg, wParam, lParam)
    }
}

static mut APP_STATE: Option<AppState> = None;
static mut RESOURCES: Option<Resources> = None;
static mut VIEW_STATE: Option<ViewState> = None;

fn main() -> Result<(), Error> {
    let _hwnd = create_window("an_editor", "тест")?;
    println!("yo");
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
