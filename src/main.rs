#![allow(non_snake_case)]
// #![windows_subsystem = "windows"]  // prevent console

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::mem;
use std::ptr::{null, null_mut};
use std::io::Error;
use std::path::PathBuf;
use std::cell::RefCell;

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
use winapi::um::commdlg::*;
use winapi::ctypes::*;

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
            style : CS_OWNDC | CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
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

#[derive(PartialEq, Eq)]
enum ActionType {
    InsertChar,
    Backspace,
    Del,
    Other,
}

struct AppState {
    hwnd: HWND,

    resources: Resources,
    view_state: ViewState,

    filename: Option<PathBuf>,

    flash: Option<String>,

    left_button_pressed: bool,
    last_action: ActionType,
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

            flash: None,

            left_button_pressed: false,
            last_action: ActionType::Other,
        }
    }

    fn get_title(&self) -> String {
        let mut s = String::new();
        if self.view_state.modified() {
            s.push_str("* ");
        }
        match &self.filename {
            Some(p) => s.push_str(&p.file_name().unwrap().to_string_lossy()),
            None => s.push_str("untitled"),
        };
        s
    }

    fn update_title(&self) {
        unsafe {
            let res = SetWindowTextW(
                self.hwnd,
                win32_string(&self.get_title()).as_ptr());
            assert!(res != 0);
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

fn invalidate_rect(hwnd: HWND) {
    unsafe {
        let res = InvalidateRect(hwnd, null(), 1);
        assert!(res != 0);
    }
}

const PADDING_LEFT: f32 = 5.0;

fn paint(app_state: &mut AppState) {
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

enum FileDialogType {
    Open,
    SaveAs,
}

// mut app_state is passed to ensure that it's not borrowed at the time,
// for window proc reentrancy
fn file_dialog(app_state: &mut RefCell<AppState>, tp: FileDialogType) -> Option<PathBuf> {
    let hwnd = app_state.borrow_mut().hwnd;
    let mut buf: Vec<u16> = vec![0 as u16; 1024];
    let mut d = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        hInstance: null_mut(),
        lpstrFilter: null(),
        lpstrCustomFilter: null_mut(),
        nMaxCustFilter: 0,
        nFilterIndex: 0,
        lpstrFile: buf.as_mut_ptr(),
        nMaxFile: buf.len() as u32,
        lpstrFileTitle: null_mut(),
        nMaxFileTitle: 0,
        lpstrInitialDir: null(),
        lpstrTitle: null(),
        Flags: 0,
        nFileOffset: 0,
        nFileExtension: 0,
        lpstrDefExt: null(),
        lCustData: 0,
        lpfnHook: None,
        lpTemplateName: null(),
        pvReserved: null_mut(),
        dwReserved: 0,
        FlagsEx: 0,
    };
    let opened = unsafe {
        let res = match tp {
            FileDialogType::Open => GetOpenFileNameW(&mut d),
            FileDialogType::SaveAs => GetSaveFileNameW(&mut d),
        };
        if res != 0 {
            true
        } else {
            let e = CommDlgExtendedError();
            assert!(e == 0, "{}", e);
            false
        }
    };
    if opened {
        let mut pos = 0;
        while buf[pos] != 0 {
            pos += 1;
        }
        Some(OsString::from_wide(&buf[..pos]).into())
    } else {
        None
    }
}

// Unsafe because one must remember that it's blocking
// and has its own message loop, so when using from window proc
// one must ensure reentrancy.
unsafe fn message_box_raw(hwnd: HWND, title: &str, message: &str, u_type: UINT) -> c_int {
    let res = MessageBoxW(
        hwnd,
        win32_string(message).as_ptr(),
        win32_string(title).as_ptr(),
        u_type);
    assert!(res != 0, "{}", Error::last_os_error());
    res
}

// mut app_state is passed to ensure that it's not borrowed at the time,
// for window proc reentrancy
fn message_box(
    app_state: &mut RefCell<AppState>,
    title: &str,
    message: &str,
    u_type: UINT,
) -> c_int {
    let hwnd = app_state.borrow_mut().hwnd;
    unsafe {
        message_box_raw(hwnd, title, message, u_type)
    }
}

fn load_document(app_state: &mut RefCell<AppState>, path: PathBuf) {
    match std::fs::read_to_string(&path) {
        Ok(mut content) => {
            let mut app_state = app_state.borrow_mut();
            let initially_modified = if content.contains('\r') {
                content = content.replace('\r', "");
                assert!(app_state.flash.is_none());
                app_state.flash = Some(
                    "CRLF line breaks were converted to LF".to_owned());
                true
            } else {
                false
            };
            app_state.filename = Some(path);
            app_state.view_state.load(&content, initially_modified);
            app_state.update_title();
        }
        Err(e) => {
            let msg = format!("Can't open {}.\n{}", path.to_string_lossy(), e);
            message_box(
                app_state,
                "an editor - error",
                &msg,
                MB_OK | MB_ICONERROR);
        }
    }
}

fn save_document(app_state: &mut RefCell<AppState>, path: PathBuf) -> bool {
    let mut g = app_state.borrow_mut();
    let content: String = g.view_state.content();
    match std::fs::write(&path, content) {
        Ok(()) => {
            g.filename = Some(path);
            g.view_state.set_unmodified_snapshot();
            g.update_title();
            true
        },
        Err(e) => {
            let msg = format!("Can't write to {}.\n{}", path.to_string_lossy(), e);
            drop(g);
            message_box(app_state, "an editor - error", &msg, MB_OK | MB_ICONERROR);
            false
        }
    }
}

// Returns true if it's ok to proceed
// (that is, the changes were saved or the user chose to abandon them).
fn prompt_about_unsaved_changes(app_state: &mut RefCell<AppState>) -> bool {
    let res = message_box(
        app_state,
        "an editor - unsaved changes",
        "Do you want to save changes to the current document?",
        MB_YESNOCANCEL | MB_ICONWARNING);
    match res {
        IDYES => {
            let path = app_state.borrow_mut().filename.clone();
            match path {
                Some(path) => {
                    if save_document(app_state, path) {
                        return true;
                    }
                }
                None => {
                    if let Some(path) = file_dialog(app_state, FileDialogType::SaveAs) {
                        save_document(app_state, path);
                        // Intentionally not returning true after
                        // "saving as" untitled document,
                        // to avoid chaining modals.
                    }
                }
            }
        }
        IDNO => {
            return true;
        }
        IDCANCEL => {}
        _ => unreachable!("{}", res),
    }
    false
}

fn handle_keydown(app_state: &mut RefCell<AppState>, key_code: i32, scan_code: i32) {
    let mut g = app_state.borrow_mut();
    let a = &mut *g;
    let view_state = &mut a.view_state;

    let ctrl_pressed = unsafe { GetKeyState(VK_CONTROL) } as u16 & 0x8000 != 0;
    let shift_pressed = unsafe { GetKeyState(VK_SHIFT) } as u16 & 0x8000 != 0;

    match key_code {
        VK_DELETE if shift_pressed => {  // shift-del (cut)
            a.last_action = ActionType::Other;
            view_state.make_undo_snapshot();
            let s = view_state.cut_selection();
            set_clipboard(a.hwnd, &s);
            invalidate_rect(a.hwnd);
            a.update_title();
            return;
        }
        VK_INSERT if ctrl_pressed && !shift_pressed => {  // ctrl-ins (copy)
            a.last_action = ActionType::Other;
            let s = view_state.get_selection();
            set_clipboard(a.hwnd, &s);
            return;
        }
        VK_INSERT if shift_pressed && !ctrl_pressed => {  // shift-ins (paste)
            a.last_action = ActionType::Other;
            view_state.make_undo_snapshot();
            let s = get_clipboard(a.hwnd);
            view_state.paste(&s);
            invalidate_rect(a.hwnd);
            a.update_title();
            return;
        }
        _ => {}
    }

    if ctrl_pressed {
        match scan_code {
            0x2d => {  // ctrl-X
                a.last_action = ActionType::Other;
                view_state.make_undo_snapshot();
                let s = view_state.cut_selection();
                set_clipboard(a.hwnd, &s);
                invalidate_rect(a.hwnd);
                a.update_title();
                return;
            }
            0x2e => {  // ctrl-C
                a.last_action = ActionType::Other;
                let s = view_state.get_selection();
                set_clipboard(a.hwnd, &s);
                return;
            }
            0x2f => {  // ctrl-V
                a.last_action = ActionType::Other;
                view_state.make_undo_snapshot();
                let s = get_clipboard(a.hwnd);
                view_state.paste(&s);
                invalidate_rect(a.hwnd);
                a.update_title();
                return;
            }
            0x2c => {  // ctrl-Z
                a.last_action = ActionType::Other;
                view_state.undo();
                invalidate_rect(a.hwnd);
                a.update_title();
                return;
            }
            _ => {}
        }
        match key_code {
            89 => {  // ord('Y')
                a.last_action = ActionType::Other;
                view_state.redo();
                invalidate_rect(a.hwnd);
                a.update_title();
                return;
            }
            65 => {  // ord('A')
                a.last_action = ActionType::Other;
                view_state.select_all();
                invalidate_rect(a.hwnd);
                return;
            }
            83 => {  // ord('S')
                match &a.filename {
                    Some(path) => {
                        if a.view_state.modified() {
                            let path = path.clone();
                            drop(g);
                            save_document(app_state, path);
                            app_state.borrow_mut().update_title();
                        }
                    }
                    None => {
                        drop(g);
                        if let Some(path) = file_dialog(app_state, FileDialogType::SaveAs) {
                            save_document(app_state, path);
                            let mut g = app_state.borrow_mut();
                            g.view_state.set_unmodified_snapshot();
                            g.update_title();
                        }
                    }
                }
                return;
            }
            79 => {  // ord('O')
                let modified = g.view_state.modified();
                drop(g);
                if !modified ||
                    prompt_about_unsaved_changes(app_state) {

                    if let Some(path) = file_dialog(app_state, FileDialogType::Open) {
                        load_document(app_state, path);
                        let app_state = app_state.borrow_mut();
                        invalidate_rect(app_state.hwnd);
                        app_state.update_title();
                    }
                }
                return;
            }
            _ => {}
        }
    }

    let mut regular_movement_cmd = true;
    match key_code {
        VK_BACK => {
            // TODO: also make shapshot before deleting newline
            if a.last_action != ActionType::Backspace {
                view_state.make_undo_snapshot();
                a.last_action = ActionType::Backspace;
            }
            view_state.backspace();
            regular_movement_cmd = false;
        }
        VK_DELETE => {
            // TODO: also make shapshot before deleting newline
            if a.last_action != ActionType::Del {
                view_state.make_undo_snapshot();
                a.last_action = ActionType::Del;
            }
            view_state.del();
            regular_movement_cmd = false;
        }
        VK_LEFT => {
            a.last_action = ActionType::Other;
            if ctrl_pressed {
                view_state.ctrl_left()
            } else {
                view_state.left()
            }
        }
        VK_RIGHT => {
            a.last_action = ActionType::Other;
            if ctrl_pressed {
                view_state.ctrl_right()
            } else {
                view_state.right()
            }
        }
        VK_HOME => {
            a.last_action = ActionType::Other;
            if ctrl_pressed {
                view_state.ctrl_home()
            } else {
                view_state.home()
            }
        }
        VK_END => {
            a.last_action = ActionType::Other;
            if ctrl_pressed {
                view_state.ctrl_end()
            } else {
                view_state.end()
            }
        }
        VK_UP => {
            a.last_action = ActionType::Other;
            if ctrl_pressed {
                regular_movement_cmd = false;
                view_state.scroll(1.0)
            } else {
                view_state.up()
            }
        }
        VK_DOWN => {
            a.last_action = ActionType::Other;
            if ctrl_pressed  {
                regular_movement_cmd = false;
                view_state.scroll(-1.0)
            } else {
                view_state.down()
            }
        }
        VK_PRIOR => {
            a.last_action = ActionType::Other;
            view_state.pg_up();
        }
        VK_NEXT => {
            a.last_action = ActionType::Other;
            view_state.pg_down();
        }
        VK_RETURN => {
            a.last_action = ActionType::InsertChar;
            view_state.make_undo_snapshot();
            view_state.insert_char('\n');
            regular_movement_cmd = false;
        }
        _ => return,
    };
    if regular_movement_cmd && !shift_pressed {
        view_state.clear_selection();
    }
    invalidate_rect(a.hwnd);
    a.update_title();
}

// Don't use .get_mut() on it, use .borrow_mut().
// We still use runtime check because not all ways window proc
// can be reentered are annotated like message_box().
fn get_app_state<'a>(hwnd: HWND) -> &'a mut RefCell<AppState> {
    let user_data = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    assert!(user_data != 0, "{}", Error::last_os_error());
    let cell = user_data as *mut std::cell::RefCell<AppState>;
    unsafe { &mut *cell }
}

// https://docs.microsoft.com/en-us/windows/desktop/winmsg/window-procedures
unsafe extern "system"
fn my_window_proc(hWnd: HWND, msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            println!("WM_CREATE");

            let app_state = AppState::new(hWnd);

            let user_data = Box::into_raw(Box::new(std::cell::RefCell::new(app_state)));
            let user_data = user_data as isize;

            let old_user_data = SetWindowLongPtrW(hWnd, GWLP_USERDATA, user_data);
            assert!(old_user_data == 0);
            let e = Error::last_os_error();
            assert!(e.raw_os_error() == Some(0), "{}", e);

            let app_state = get_app_state(hWnd);
            app_state.borrow_mut().update_title();
            if let Some(path) = std::env::args().nth(1) {
                load_document(app_state, PathBuf::from(path));
            }

            0
        }
        WM_NCDESTROY => {
            println!("WM_NCDESTROY");

            // just to ensure nobody is borrowing it at the moment
            get_app_state(hWnd).borrow_mut();

            let user_data = GetWindowLongPtrW(hWnd, GWLP_USERDATA);
            assert!(user_data != 0, "{}", Error::last_os_error());
            let app_state = Box::from_raw(user_data as *mut std::cell::RefCell<AppState>);
            drop(app_state);

            PostQuitMessage(0);
            0
        }
        WM_CLOSE => {
            println!("WM_CLOSE");
            let app_state = get_app_state(hWnd);
            let modified = app_state.borrow_mut().view_state.modified();
            if !modified ||
               prompt_about_unsaved_changes(app_state) {
                DestroyWindow(hWnd);
            }
            0
        }
        WM_PAINT => {
            println!("WM_PAINT");
            let app_state = get_app_state(hWnd);
            let flash = {
                let mut app_state = app_state.borrow_mut();
                paint(&mut *app_state);
                let ret = ValidateRect(hWnd, null());
                assert!(ret != 0);
                app_state.flash.take()
            };
            if let Some(s) = flash {
                println!("flash");
                message_box(app_state, "an editor", &s, MB_OK | MB_ICONINFORMATION);
            }

            0
        }
        WM_SIZE => {
            println!("WM_SIZE");
            let mut g = get_app_state(hWnd).borrow_mut();
            let a = &mut *g;
            let resources = &a.resources;
            let view_state = &mut a.view_state;

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
            let mut app_state = get_app_state(hWnd).borrow_mut();

            app_state.left_button_pressed = true;
            let x = GET_X_LPARAM(lParam);
            let y = GET_Y_LPARAM(lParam);
            app_state.last_action = ActionType::Other;
            app_state.view_state.click(x as f32 - PADDING_LEFT, y as f32);
            let shift_pressed = GetKeyState(VK_SHIFT) as u16 & 0x8000 != 0;
            if !shift_pressed {
                app_state.view_state.clear_selection();
            }
            invalidate_rect(app_state.hwnd);
            SetCapture(hWnd);
            0
        }
        WM_LBUTTONUP => {
            println!("WM_LBUTTONUP");
            let mut app_state = get_app_state(hWnd).borrow_mut();
            app_state.left_button_pressed = false;
            let res = ReleaseCapture();
            assert!(res != 0);
            0
        }
        WM_LBUTTONDBLCLK => {
            println!("WM_LBUTTONDBLCLK");
            let mut app_state =  get_app_state(hWnd).borrow_mut();
            let x = GET_X_LPARAM(lParam);
            let y = GET_Y_LPARAM(lParam);
            app_state.view_state.double_click(x as f32 - PADDING_LEFT, y as f32);
            invalidate_rect(app_state.hwnd);
            0
        }
        WM_MOUSEMOVE => {
            // println!("WM_MOUSEMOVE");
            let mut app_state =  get_app_state(hWnd).borrow_mut();
            if app_state.left_button_pressed {
                let x = GET_X_LPARAM(lParam);
                let y = GET_Y_LPARAM(lParam);
                app_state.view_state.click(x as f32 - PADDING_LEFT, y as f32);
                invalidate_rect(app_state.hwnd);
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
            let mut app_state = get_app_state(hWnd).borrow_mut();
            app_state.view_state.scroll(delta);
            invalidate_rect(app_state.hwnd);
            0
        }
        WM_CHAR => {
            let c: char = std::char::from_u32(wParam as u32).unwrap();
            println!("WM_CHAR {:?}", c);
            if wParam >= 32 || wParam == 9 /* tab */ {
                let mut app_state = get_app_state(hWnd).borrow_mut();
                if app_state.last_action != ActionType::InsertChar {
                    app_state.view_state.make_undo_snapshot();
                    app_state.last_action = ActionType::InsertChar;
                }
                app_state.view_state.insert_char(c);
                invalidate_rect(app_state.hwnd);
                app_state.update_title();
            }
            0
        }
        WM_KEYDOWN => {
            println!("WM_KEYDOWN {}", wParam);
            let key_code = wParam as i32;
            let scan_code = ((lParam >> 16) & 511) as i32;
            let app_state =  get_app_state(hWnd);
            handle_keydown(app_state, key_code, scan_code);
            0
        }
        _ => DefWindowProcW(hWnd, msg, wParam, lParam)
    }
}

fn panic_hook(pi: &std::panic::PanicInfo) {
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

    let hwnd = unsafe { STATIC_HWND };
    if let Some(hwnd) = hwnd {
        // The panic was likely thrown from inside window procedure.
        // The stack was not unwound yet, so we are likely holding app_state.
        // We can't simply call MessageBox here, because while it's open,
        // it will dispatch messages such as WM_MOUSEMOVE and window procedure
        // will be reentered and fail attempting to grab app_state.
        // To prevent this, we replace our window proc with the default one.
        let res = unsafe {
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, DefWindowProcW as isize)
        };
        assert!(res != 0, "{}", Error::last_os_error());
    }

    unsafe {
        message_box_raw(
            hwnd.unwrap_or(null_mut()),
            "an editor - error",
            "A programming error has occurred.\nDiagnostic info is in 'error.txt'",
            MB_OK | MB_ICONERROR);
    }

    if let Some(hwnd) = hwnd {
        let res = unsafe { DestroyWindow(hwnd) };
        assert!(res != 0, "{}", Error::last_os_error());
    }

    std::process::exit(1);
}

static mut STATIC_HWND: Option<HWND> = None;

fn main() -> Result<(), Error> {
    std::panic::set_hook(Box::new(panic_hook));
    let hwnd = create_window("an_editor", "window title")?;
    unsafe {
        STATIC_HWND = Some(hwnd);
    }
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
