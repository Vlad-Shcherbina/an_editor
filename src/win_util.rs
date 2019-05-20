use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::io::Error;
use std::ptr::{null, null_mut};
use std::cell::RefCell;
use std::path::PathBuf;

use winapi::shared::minwindef::*;
use winapi::shared::windef::*;
use winapi::shared::winerror::*;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::winbase::*;
use winapi::um::winuser::*;
use winapi::um::errhandlingapi::*;
use winapi::um::commdlg::*;
use winapi::ctypes::*;

pub trait HasHwnd {
    fn hwnd(&self) -> HWND;
}

// This is a safe-ish abstraction around window proc reentrancy
// and global app state.
// If a WinAPI call is known to send messages, wrap it in a function
// that takes &mut Token. This will statically ensure there are no
// active borrows on the app state.
// And even if one misses some of such WinAPI calls, that's no problem,
// it will be caught by RefCell at runtime.
pub struct Token<AppState: 'static>(&'static RefCell<AppState>);

impl<AppState> Token<AppState> {
    pub fn new(cell: *const RefCell<AppState>) -> Self {
        Self(unsafe { &*cell })
    }

    pub fn borrow_mut(&mut self)
    -> impl std::ops::Deref<Target=AppState> + std::ops::DerefMut<Target=AppState> {
        self.0.borrow_mut()
    }
}

pub fn win32_string(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

pub fn create_window(class_name: &str, title: &str, wnd_proc: WNDPROC) -> Result<HWND, Error> {
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
            lpfnWndProc : wnd_proc,
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

pub fn set_window_title(hwnd: HWND, title: &str) {
    unsafe {
        let res = SetWindowTextW(hwnd, win32_string(title).as_ptr());
        assert!(res != 0, "{}", Error::last_os_error());
    }
}

pub fn invalidate_rect(hwnd: HWND) {
    unsafe {
        let res = InvalidateRect(hwnd, null(), 1);
        assert!(res != 0, "{}", Error::last_os_error());
    }
}

// Why not just write `p as *mut T2`?
// Because then when casting from say *mut void to *mut u16,
// Clippy complains about pointer alignment
// https://rust-lang.github.io/rust-clippy/master/index.html#cast_ptr_alignment
fn cast_ptr<T1, T2>(p: *mut T1) -> *mut T2 {
    assert!(p as usize % std::mem::align_of::<T2>() == 0);
    p as *mut T2
}

pub fn get_clipboard(hwnd: HWND) -> String {
    unsafe {
        let res = OpenClipboard(hwnd);
        assert!(res != 0);
        let h = GetClipboardData(CF_UNICODETEXT);
        let pdata: *mut u16 = cast_ptr(GlobalLock(h));
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

pub fn set_clipboard(hwnd: HWND, s: &str) {
    let data = win32_string(s);
    unsafe {
        let res = OpenClipboard(hwnd);
        assert!(res != 0);
        let res = EmptyClipboard();
        assert!(res != 0);

        let h = GlobalAlloc(GMEM_MOVEABLE, data.len() * 2);
        assert!(!h.is_null());

        let pdata: *mut u16 = cast_ptr(GlobalLock(h));
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

// Unsafe to remind about window proc reentrancy.
pub unsafe fn message_box_raw(hwnd: HWND, title: &str, message: &str, u_type: UINT) -> c_int {
    let res = MessageBoxW(
        hwnd,
        win32_string(message).as_ptr(),
        win32_string(title).as_ptr(),
        u_type);
    assert!(res != 0, "{}", Error::last_os_error());
    res
}

pub fn message_box(
    app_state: &mut Token<impl HasHwnd>,
    title: &str,
    message: &str,
    u_type: UINT,
) -> c_int {
    let hwnd = app_state.borrow_mut().hwnd();
    unsafe {
        message_box_raw(hwnd, title, message, u_type)
    }
}

pub enum FileDialogType {
    Open,
    SaveAs,
}

pub fn file_dialog(app_state: &mut Token<impl HasHwnd>, tp: FileDialogType) -> Option<PathBuf> {
    let hwnd = app_state.borrow_mut().hwnd();
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

pub fn set_menu(app_state: &mut Token<impl HasHwnd>, menu: HMENU) {
    let hwnd = app_state.borrow_mut().hwnd();
    let res = unsafe { SetMenu(hwnd, menu) };
    assert!(res != 0, "{}", Error::last_os_error());
}

pub fn create_menu() -> HMENU {
    let menu = unsafe { CreateMenu() };
    assert!(!menu.is_null(), "{}", Error::last_os_error());
    menu
}

pub fn append_menu_string(menu: HMENU, id: u16, text: &str) {
    let res = unsafe {
        AppendMenuW(menu, MF_STRING, id as usize, win32_string(text).as_ptr())
    };
    assert!(res != 0, "{}", Error::last_os_error());
}

pub fn append_menu_popup(menu: HMENU, submenu: HMENU, text: &str) {
    let res = unsafe {
        AppendMenuW(menu, MF_POPUP, submenu as usize, win32_string(text).as_ptr())
    };
    assert!(res != 0, "{}", Error::last_os_error());
}

pub fn append_menu_separator(menu: HMENU) {
    let res = unsafe {
        AppendMenuW(menu, MF_SEPARATOR, 0, null())
    };
    assert!(res != 0, "{}", Error::last_os_error());
}

pub fn enable_or_disable_menu_item(menu: HMENU, id: u16, enable: bool) {
    let res = unsafe {
        EnableMenuItem(menu, u32::from(id), if enable { MF_ENABLED } else { MF_GRAYED })
    };
    assert!(res != -1);
}

pub fn send_message(
    app_state: &mut Token<impl HasHwnd>,
    msg: UINT, w_param: WPARAM, l_param: LPARAM,
) -> LRESULT {
    let hwnd = app_state.borrow_mut().hwnd();
    unsafe {
        SendMessageW(hwnd, msg, w_param, l_param)
    }
}
