#![allow(non_snake_case)]
// #![windows_subsystem = "windows"]  // prevent console

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::mem;
use std::ptr::{null, null_mut};
use std::io::Error;

use winapi::Interface;
use winapi::ctypes::c_void;
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
use winapi::um::unknwnbase::*;
use winapi::um::d2d1::{
    D2D1_SIZE_U,
    D2D1_RECT_F,
    D2D1_POINT_2F,
};

mod com_ptr;
use com_ptr::ComPtr;

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

        let cursor = LoadCursorW(0 as HINSTANCE, IDC_ARROW);
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
                &mut d2d_factory,
            );
            assert!(hr == S_OK, "0x{:x}", hr);
            ComPtr::from_raw(d2d_factory as * mut _)
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
    brush: ComPtr<ID2D1SolidColorBrush>,
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
            let c = D2D1_COLOR_F { r: 1.0, b: 1.0, g: 1.0, a: 1.0 };
            let mut brush = null_mut();
            let hr = render_target.CreateSolidColorBrush(&c, null(), &mut brush);
            assert!(hr == S_OK, "0x{:x}", hr);
            ComPtr::from_raw(brush)
        };
        let text_format = unsafe {
            let mut text_format = null_mut();
            let hr = app_state.dwrite_factory.CreateTextFormat(
                win32_string("Consolas").as_ptr(),
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
            brush,
            text_format,
        }
    }
}

fn paint() {
    let resources = unsafe { RESOURCES.as_ref().unwrap() };
    let app_state = unsafe { APP_STATE.as_ref().unwrap() };
    let rt = &resources.render_target;
    unsafe {
        rt.BeginDraw();
        let c = D2D1_COLOR_F { r: 0.0, b: 0.2, g: 0.0, a: 1.0 };
        rt.Clear(&c);
        let size = rt.GetSize();
        rt.DrawLine(
            D2D_POINT_2F { x: 0.0, y: 0.0 },
            D2D_POINT_2F {
                x: size.width,
                y: size.height,
            },
            resources.brush.as_up_raw(),
            2.0,
            null_mut(),
        );

        let message = win32_string("Здравствуй, мир!\nZz");

        let origin = D2D1_POINT_2F {
            x: 100.0,
            y: 0.0,
        };
        let layout_width = 100.0;
        let layout_height = 100.0;

        let text_layout = {
            let mut text_layout = null_mut();
            let hr = app_state.dwrite_factory.CreateTextLayout(
                message.as_ptr(),
                (message.len() - 1) as u32,
                resources.text_format.as_raw(),
                layout_width,
                layout_height,
                &mut text_layout,
            );
            assert!(hr == S_OK, "0x{:x}", hr);
            ComPtr::from_raw(text_layout)
        };

        let mut metrics : DWRITE_TEXT_METRICS = std::mem::zeroed();
        let hr = (*text_layout).GetMetrics(&mut metrics);
        assert!(hr == S_OK, "0x{:x}", hr);

        let r = D2D1_RECT_F {
            left: origin.x,
            top: origin.y,
            right: origin.x + metrics.widthIncludingTrailingWhitespace,
            bottom: origin.y + metrics.height,
        };
        rt.DrawRectangle(&r, resources.brush.as_up_raw(), 1.0, null_mut());

        rt.DrawTextLayout(
            origin,
            text_layout.as_raw(),
            resources.brush.as_up_raw(),
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );

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
            APP_STATE = Some(app_state);
            RESOURCES = Some(resources);
            0
        }
        WM_SIZE => {
            println!("WM_SIZE");
            let render_size = D2D_SIZE_U {
                width: GET_X_LPARAM(lParam) as u32,
                height: GET_Y_LPARAM(lParam) as u32,
            };

            let resources = RESOURCES.as_ref().unwrap();
            let hr = (*resources.render_target).Resize(&render_size);
            assert!(hr == S_OK, "0x{:x}", hr);
            0
        }
        _ => DefWindowProcW(hWnd, msg, wParam, lParam)
    }
}

static mut APP_STATE: Option<AppState> = None;
static mut RESOURCES: Option<Resources> = None;

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
