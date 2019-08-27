#![allow(non_snake_case)]
// #![windows_subsystem = "windows"]  // prevent console

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
use winapi::um::winuser::*;
use winapi::um::dcommon::*;
use winapi::um::d2d1::*;
use winapi::um::dwrite::*;
use winapi::um::d2d1::{
    D2D1_SIZE_U,
    D2D1_POINT_2F,
};

use log::info;

mod com_ptr;
mod text_layout;
mod line_gap_buffer;
mod view_state;
mod win_util;
mod key_util;

use com_ptr::ComPtr;
use view_state::ViewState;

use win_util::*;
use key_util::{KeyEvent, KeyMatcher};

#[derive(PartialEq, Eq)]
enum ActionType {
    InsertChar,
    Backspace,
    Del,
    Other,
}

struct AppState {
    hwnd: HWND,

    dwrite_factory: ComPtr<IDWriteFactory>,
    resources: Resources,
    view_state: ViewState,
    font_size: f32,

    filename: Option<PathBuf>,

    flash: Option<String>,

    left_button_pressed: bool,
    last_action: ActionType,

    menu: HMENU,
    key_bindings: Vec<(KeyMatcher, Idm)>,
}

impl HasHwnd for AppState {
    fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl AppState {
    fn new(hwnd: HWND) -> Self {
        let d2d_factory = unsafe {
            let factory_options = D2D1_FACTORY_OPTIONS {
                debugLevel: D2D1_DEBUG_LEVEL_NONE,
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
            dwrite_factory,
            resources,
            view_state,
            font_size: DEFAULT_FONT_SIZE,

            filename: None,

            flash: None,

            left_button_pressed: false,
            last_action: ActionType::Other,

            menu: create_app_menu(),
            key_bindings: init_key_bindings(),
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
        set_window_title(self.hwnd, &self.get_title());
    }

    fn match_key_event(&self, k: &KeyEvent) -> Option<Idm> {
        let mut matches = Vec::new();
        for (km, cmd) in &self.key_bindings {
            if km.matches(k) {
                matches.push(*cmd);
            }
        }
        assert!(matches.len() < 2);
        matches.first().cloned()
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
            let mut rc: RECT = mem::zeroed();
            let res = GetClientRect(hwnd, &mut rc);
            assert!(res != 0, "{}", Error::last_os_error());
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
        Resources {
            render_target,
            brush: brush.up(),
            sel_brush: sel_brush.up(),
            text_format: create_text_format(dwrite_factory, DEFAULT_FONT_SIZE),
        }
    }
}

fn create_text_format(dwrite_factory: &ComPtr<IDWriteFactory>, size: f32) -> ComPtr<IDWriteTextFormat> {
    unsafe {
        let mut text_format = null_mut();
        let hr = dwrite_factory.CreateTextFormat(
            win32_string("Arial").as_ptr(),
            null_mut(),
            DWRITE_FONT_WEIGHT_REGULAR,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            win32_string("en-us").as_ptr(),
            &mut text_format,
        );
        assert!(hr == S_OK, "0x{:x}", hr);
        ComPtr::from_raw(text_format)
    }
}

const DEFAULT_FONT_SIZE: f32 = 14.0;
const MIN_FONT_SIZE: f32 = 4.0;
const MAX_FONT_SIZE: f32 = 32.0;

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

fn load_document(app_state: &mut Token<AppState>, path: PathBuf) {
    match std::fs::read(&path) {
        Ok(data) => {
            let mut content = String::from_utf8_lossy(&data);
            let utf8_loss = match content {
                std::borrow::Cow::Borrowed(_) => false,
                std::borrow::Cow::Owned(_) => true,
            };
            let crlf_fix = if content.contains('\r') {
                content = content.replace('\r', "").into();
                true
            } else {
                false
            };
            let mut app_state = app_state.borrow_mut();
            app_state.filename = Some(path);
            app_state.view_state.load(&content, utf8_loss || crlf_fix);
            app_state.update_title();

            if utf8_loss || crlf_fix {
                let mut messages = Vec::new();
                if utf8_loss {
                    messages.push("File is not valid UTF-8, problematic parts were replaced with '�'.");
                }
                if crlf_fix {
                    messages.push("CRLF line breaks were converted to LF.");
                }
                assert!(app_state.flash.is_none());
                app_state.flash = Some(messages.join("\n"));
            }
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

fn save_document(app_state: &mut Token<AppState>, path: PathBuf) -> bool {
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
fn prompt_about_unsaved_changes(app_state: &mut Token<AppState>) -> bool {
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

fn init_key_bindings() -> Vec<(KeyMatcher, Idm)> {
    use key_util::{CTRL, SHIFT, ALT};
    let vk = |key_code| KeyMatcher::from_key_code(key_code);
    let ch_scan = |c| KeyMatcher::from_char_to_scan_code(c);
    vec![
        (SHIFT + vk(VK_DELETE), Idm::Cut),
        (CTRL + vk(VK_INSERT), Idm::Copy),
        (SHIFT + vk(VK_INSERT), Idm::Paste),
        (CTRL + ch_scan('X'), Idm::Cut),
        (CTRL + ch_scan('C'), Idm::Copy),
        (CTRL + ch_scan('V'), Idm::Paste),

        (CTRL + vk(VK_OEM_MINUS), Idm::SmallerFont),
        (CTRL + vk(VK_OEM_PLUS), Idm::LargerFont),
        (CTRL + vk(VK_SUBTRACT), Idm::SmallerFont),
        (CTRL + vk(VK_ADD), Idm::LargerFont),

        (CTRL + ch_scan('Z'), Idm::Undo),
        (CTRL + ch_scan('Y'), Idm::Redo),

        (CTRL + ch_scan('A'), Idm::SelectAll),
        (CTRL + ch_scan('N'), Idm::New),
        (CTRL + ch_scan('O'), Idm::Open),
        (CTRL + ch_scan('S'), Idm::Save),
        (CTRL + (SHIFT + ch_scan('S')), Idm::SaveAs),

        (ALT + ch_scan('Q'), Idm::Exit),
    ]
}

fn handle_keydown(app_state: &mut Token<AppState>, k: KeyEvent) {
    let mut g = app_state.borrow_mut();
    let a = &mut *g;

    if let Some(cmd) = a.match_key_event(&k) {
        drop(g);
        send_message(app_state, WM_COMMAND, cmd as usize, 0);
        return;
    }

    let view_state = &mut a.view_state;

    let ctrl_pressed = k.ctrl_pressed;
    let shift_pressed = k.shift_pressed;

    let mut regular_movement_cmd = true;
    match k.key_code {
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

fn get_app_state(hwnd: HWND) -> Token<AppState> {
    let user_data = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    assert!(user_data != 0, "{}", Error::last_os_error());
    let cell = user_data as *const std::cell::RefCell<AppState>;
    Token::new(cell)
}

#[derive(Clone, Copy)]
enum Idm {
    New = 1,
    Open,
    Save,
    SaveAs,
    Exit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    SmallerFont,
    LargerFont,
}

fn create_app_menu() -> HMENU {
    let file_menu = create_menu();
    append_menu_string(file_menu, Idm::New as u16, "&New\tCtrl-N");
    append_menu_string(file_menu, Idm::Open as u16, "&Open...\tCtrl-O");
    append_menu_string(file_menu, Idm::Save as u16, "&Save\tCtrl-S");
    append_menu_string(file_menu, Idm::SaveAs as u16, "&Save As...\tCtrl-Shift-S");
    append_menu_separator(file_menu);
    append_menu_string(file_menu, Idm::Exit as u16, "&Exit\tAlt-Q");
    let edit_menu = create_menu();
    append_menu_string(edit_menu, Idm::Undo as u16, "&Undo\tCtrl-Z");
    append_menu_string(edit_menu, Idm::Redo as u16, "&Redo\tCtrl-Y");
    append_menu_separator(edit_menu);

    // anchor:nlfrlxqmswoujkiu
    append_menu_string(edit_menu, Idm::Cut as u16, "&Cut\tCtrl-X or Shift-Del");
    append_menu_string(edit_menu, Idm::Copy as u16, "&Copy\tCtrl-C or Ctrl-Ins");
    append_menu_string(edit_menu, Idm::Paste as u16, "&Paste\tCtrl-V or Shift-Ins");

    append_menu_separator(edit_menu);
    append_menu_string(edit_menu, Idm::SelectAll as u16, "&Select all\tCtrl-A");
    let view_menu = create_menu();
    append_menu_string(view_menu, Idm::SmallerFont as u16, "&Smaller font\tCtrl-- or Ctrl-Wheel Up");
    append_menu_string(view_menu, Idm::LargerFont as u16, "&Larger font\tCtrl-+ or Ctrl-Wheel Down");
    let menu = create_menu();
    append_menu_popup(menu, file_menu, "File");
    append_menu_popup(menu, edit_menu, "Edit");
    append_menu_popup(menu, view_menu, "View");
    menu
}

fn enable_available_menu_items(app_state: &mut AppState) {
    enable_or_disable_menu_item(
        app_state.menu,
        Idm::New as u16,
        app_state.filename.is_some() || app_state.view_state.modified());
    enable_or_disable_menu_item(
        app_state.menu,
        Idm::Save as u16,
        app_state.filename.is_none() || app_state.view_state.modified());
    enable_or_disable_menu_item(
        app_state.menu,
        Idm::Undo as u16,
        app_state.view_state.can_undo());
    enable_or_disable_menu_item(
        app_state.menu,
        Idm::Redo as u16,
        app_state.view_state.can_redo());
    enable_or_disable_menu_item(
        app_state.menu,
        Idm::Cut as u16,
        app_state.view_state.has_selection());
    enable_or_disable_menu_item(
        app_state.menu,
        Idm::Copy as u16,
        app_state.view_state.has_selection());
    enable_or_disable_menu_item(
        app_state.menu,
        Idm::SmallerFont as u16,
        app_state.font_size > MIN_FONT_SIZE);
    enable_or_disable_menu_item(
        app_state.menu,
        Idm::LargerFont as u16,
        app_state.font_size < MAX_FONT_SIZE);
}

fn handle_menu_command(app_state: &mut Token<AppState>, id: u16) {
    let cmd = if id == Idm::New as u16 { Idm::New }
        else if id == Idm::Open as u16 { Idm::Open }
        else if id == Idm::Save as u16 { Idm::Save }
        else if id == Idm::SaveAs as u16 { Idm::SaveAs }
        else if id == Idm::Exit as u16 { Idm::Exit }
        else if id == Idm::Undo as u16 { Idm::Undo }
        else if id == Idm::Redo as u16 { Idm::Redo }
        else if id == Idm::Cut as u16 { Idm::Cut }
        else if id == Idm::Copy as u16 { Idm::Copy }
        else if id == Idm::Paste as u16 { Idm::Paste }
        else if id == Idm::SelectAll as u16 { Idm::SelectAll }
        else if id == Idm::SmallerFont as u16 { Idm::SmallerFont }
        else if id == Idm::LargerFont as u16 { Idm::LargerFont }
        else { panic!("{}", id) };

    match cmd {
        Idm::Exit => {
            let hwnd = app_state.borrow_mut().hwnd;
            let res = unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
            assert!(res != 0, "{}", Error::last_os_error());
        }
        Idm::New => {
            let modified = app_state.borrow_mut().view_state.modified();
            if !modified ||
                prompt_about_unsaved_changes(app_state) {
                let mut app_state = app_state.borrow_mut();
                app_state.last_action = ActionType::Other;
                app_state.filename = None;
                app_state.view_state.load("", false);
                invalidate_rect(app_state.hwnd);
                app_state.update_title();
            }
        }
        Idm::Open => {
            let modified = app_state.borrow_mut().view_state.modified();
            if !modified ||
                prompt_about_unsaved_changes(app_state) {
                if let Some(path) = file_dialog(app_state, FileDialogType::Open) {
                    load_document(app_state, path);
                    let mut app_state = app_state.borrow_mut();
                    app_state.last_action = ActionType::Other;
                    invalidate_rect(app_state.hwnd);
                    app_state.update_title();
                }
            }
        }
        Idm::Save => {
            let mut g = app_state.borrow_mut();
            let a = &mut *g;
            match &a.filename {
                Some(path) => {
                    if a.view_state.modified() {
                        let path = path.clone();
                        drop(g);
                        save_document(app_state, path);
                        app_state.borrow_mut().update_title();
                        app_state.borrow_mut().last_action = ActionType::Other;
                    }
                }
                None => {
                    drop(g);
                    if let Some(path) = file_dialog(app_state, FileDialogType::SaveAs) {
                        save_document(app_state, path);
                        let mut g = app_state.borrow_mut();
                        g.update_title();
                        g.last_action = ActionType::Other;
                    }
                }
            }
        }
        Idm::SaveAs => {
            if let Some(path) = file_dialog(app_state, FileDialogType::SaveAs) {
                save_document(app_state, path);
                let mut g = app_state.borrow_mut();
                g.update_title();
                g.last_action = ActionType::Other;
            }
        }
        Idm::Undo => {
            let mut g = app_state.borrow_mut();
            let a = &mut *g;
            a.last_action = ActionType::Other;
            a.view_state.undo();
            invalidate_rect(a.hwnd);
            a.update_title();
        }
        Idm::Redo => {
            let mut g = app_state.borrow_mut();
            let a = &mut *g;
            a.last_action = ActionType::Other;
            a.view_state.redo();
            invalidate_rect(a.hwnd);
            a.update_title();
        }
        Idm::Cut => {
            let mut g = app_state.borrow_mut();
            let a = &mut *g;
            a.last_action = ActionType::Other;
            a.view_state.make_undo_snapshot();
            let s = a.view_state.cut_selection();
            set_clipboard(a.hwnd, &s);
            invalidate_rect(a.hwnd);
            a.update_title();
        }
        Idm::Copy => {
            let mut g = app_state.borrow_mut();
            let a = &mut *g;
            a.last_action = ActionType::Other;
            let s = a.view_state.get_selection();
            set_clipboard(a.hwnd, &s);
        }
        Idm::Paste => {
            let mut g = app_state.borrow_mut();
            let a = &mut *g;
            a.last_action = ActionType::Other;
            let s = get_clipboard(a.hwnd);
            if let Some(s) = s {
                a.view_state.make_undo_snapshot();
                a.view_state.paste(&s);
                invalidate_rect(a.hwnd);
                a.update_title();
            }
        }
        Idm::SelectAll => {
            let mut g = app_state.borrow_mut();
            let a = &mut *g;
            a.last_action = ActionType::Other;
            a.view_state.select_all();
            invalidate_rect(a.hwnd);
        }
        Idm::SmallerFont => {
            let mut g = app_state.borrow_mut();
            let a = &mut *g;
            a.font_size -= 1.0;
            a.font_size = a.font_size.max(MIN_FONT_SIZE);
            a.resources.text_format = create_text_format(&a.dwrite_factory, a.font_size);
            a.view_state.change_text_format(a.resources.text_format.clone());
            invalidate_rect(a.hwnd);
        }
        Idm::LargerFont => {
            let mut g = app_state.borrow_mut();
            let a = &mut *g;
            a.font_size += 1.0;
            a.font_size = a.font_size.min(MAX_FONT_SIZE);
            a.resources.text_format = create_text_format(&a.dwrite_factory, a.font_size);
            a.view_state.change_text_format(a.resources.text_format.clone());
            invalidate_rect(a.hwnd);
        }
    }
}

#[allow(clippy::cognitive_complexity)]
// https://docs.microsoft.com/en-us/windows/desktop/winmsg/window-procedures
extern "system"
fn my_window_proc(hWnd: HWND, msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            info!("WM_CREATE");

            let app_state = AppState::new(hWnd);

            let user_data = Box::into_raw(Box::new(std::cell::RefCell::new(app_state)));
            let user_data = user_data as isize;

            let old_user_data = unsafe { SetWindowLongPtrW(hWnd, GWLP_USERDATA, user_data) };
            assert!(old_user_data == 0);
            let e = Error::last_os_error();
            assert!(e.raw_os_error() == Some(0), "{}", e);

            let app_state = &mut get_app_state(hWnd);
            let menu = app_state.borrow_mut().menu;
            set_menu(app_state, menu);
            app_state.borrow_mut().update_title();
            if let Some(path) = std::env::args().nth(1) {
                load_document(app_state, PathBuf::from(path));
            }

            0
        }
        WM_NCDESTROY => {
            info!("WM_NCDESTROY");

            // just to ensure nobody is borrowing it at the moment
            get_app_state(hWnd).borrow_mut();

            let user_data = unsafe { GetWindowLongPtrW(hWnd, GWLP_USERDATA) };
            assert!(user_data != 0, "{}", Error::last_os_error());
            let app_state = unsafe {
                Box::from_raw(user_data as *mut std::cell::RefCell<AppState>)
            };
            drop(app_state);

            unsafe { PostQuitMessage(0); }
            0
        }
        WM_CLOSE => {
            info!("WM_CLOSE");
            let app_state = &mut get_app_state(hWnd);
            let modified = app_state.borrow_mut().view_state.modified();
            if !modified ||
               prompt_about_unsaved_changes(app_state) {
                unsafe { DestroyWindow(hWnd); }
            }
            0
        }
        WM_PAINT => {
            info!("WM_PAINT");
            let app_state = &mut get_app_state(hWnd);
            let flash = {
                let mut app_state = app_state.borrow_mut();
                paint(&mut *app_state);
                let ret = unsafe { ValidateRect(hWnd, null()) };
                assert!(ret != 0);
                app_state.flash.take()
            };
            if let Some(s) = flash {
                info!("flash");
                message_box(app_state, "an editor", &s, MB_OK | MB_ICONINFORMATION);
            }

            0
        }
        WM_SIZE => {
            info!("WM_SIZE");
            let app_state = &mut get_app_state(hWnd);
            let mut g = app_state.borrow_mut();
            let a = &mut *g;
            let resources = &a.resources;
            let view_state = &mut a.view_state;

            let render_size = D2D_SIZE_U {
                width: GET_X_LPARAM(lParam) as u32,
                height: GET_Y_LPARAM(lParam) as u32,
            };

            if render_size.width == 0 && render_size.height == 0 {
                info!("minimize");
            } else {
                let hr = unsafe { resources.render_target.Resize(&render_size) };
                assert!(hr == S_OK, "0x{:x}", hr);

                let size = unsafe { resources.render_target.GetSize() };
                view_state.resize(size.width - PADDING_LEFT, size.height);
            }
            0
        }
        WM_ENTERMENULOOP => {
            info!("WM_ENTERMENULOOP");
            let app_state = &mut get_app_state(hWnd);
            enable_available_menu_items(&mut app_state.borrow_mut());
            0
        }
        WM_COMMAND => {
            info!("WM_COMMAND");
            if HIWORD(wParam as u32) == 0 {
                let app_state = &mut get_app_state(hWnd);
                let id = LOWORD(wParam as u32);
                handle_menu_command(app_state, id);
            }
            0
        }
        WM_CONTEXTMENU => {
            info!("WM_CONTEXTMENU");
            let rc = unsafe {
                let mut rc: RECT = mem::zeroed();
                let res = GetClientRect(hWnd, &mut rc);
                assert!(res != 0, "{}", Error::last_os_error());
                rc
            };
            let pt_screen = POINT {
                x: GET_X_LPARAM(lParam),
                y: GET_Y_LPARAM(lParam),
            };
            info!("x={}, y={}", pt_screen.x, pt_screen.y);
            let mut pt_client = pt_screen;
            let res = unsafe { ScreenToClient(hWnd, &mut pt_client) };
            assert!(res != 0);
            if unsafe { PtInRect(&rc, pt_client)} != 0 ||
               pt_screen.x == -1 && pt_screen.y == -1 {
                let has_selection = get_app_state(hWnd).borrow_mut().view_state.has_selection();
                let context_menu = create_menu();
                // anchor:nlfrlxqmswoujkiu
                if has_selection {
                    append_menu_string(context_menu, Idm::Cut as u16, "&Cut\tCtrl-X or Shift-Del");
                    append_menu_string(context_menu, Idm::Copy as u16, "&Copy\tCtrl-C or Ctrl-Ins");
                }
                append_menu_string(context_menu, Idm::Paste as u16, "&Paste\tCtrl-V or Shift-Ins");

                // Popup menu has to be a submeny of some other menu,
                // otherwise its size is not calculated correctly :(
                let menu = create_menu();
                append_menu_popup(menu, context_menu, "zzz");
                let res = unsafe {
                    TrackPopupMenuEx(
                        context_menu,
                        TPM_RIGHTBUTTON,
                        pt_screen.x, pt_screen.y,
                        hWnd,
                        null_mut())
                };
                assert!(res != 0, "{}", Error::last_os_error());
                destroy_menu(menu);
            }
            0
        }
        WM_LBUTTONDOWN => {
            info!("WM_LBUTTONDOWN");
            let app_state = &mut get_app_state(hWnd);
            let mut app_state = app_state.borrow_mut();

            app_state.left_button_pressed = true;
            let x = GET_X_LPARAM(lParam);
            let y = GET_Y_LPARAM(lParam);
            app_state.last_action = ActionType::Other;
            app_state.view_state.click(x as f32 - PADDING_LEFT, y as f32);
            let shift_pressed = unsafe { GetKeyState(VK_SHIFT) } as u16 & 0x8000 != 0;
            if !shift_pressed {
                app_state.view_state.clear_selection();
            }
            invalidate_rect(app_state.hwnd);
            unsafe { SetCapture(hWnd); }
            0
        }
        WM_LBUTTONUP => {
            info!("WM_LBUTTONUP");
            let app_state = &mut get_app_state(hWnd);
            let mut app_state = app_state.borrow_mut();
            app_state.left_button_pressed = false;
            let res = unsafe { ReleaseCapture() };
            assert!(res != 0, "{}", Error::last_os_error());
            0
        }
        WM_LBUTTONDBLCLK => {
            info!("WM_LBUTTONDBLCLK");
            let app_state = &mut get_app_state(hWnd);
            let mut app_state = app_state.borrow_mut();
            let x = GET_X_LPARAM(lParam);
            let y = GET_Y_LPARAM(lParam);
            app_state.view_state.double_click(x as f32 - PADDING_LEFT, y as f32);
            invalidate_rect(app_state.hwnd);
            0
        }
        WM_MOUSEMOVE => {
            // info!("WM_MOUSEMOVE");
            let app_state = &mut get_app_state(hWnd);
            let mut app_state = app_state.borrow_mut();
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
            info!("WM_MOUSEWHEEL {}", delta);

            let app_state = &mut get_app_state(hWnd);
            let mut app_state = app_state.borrow_mut();

            let ctrl_pressed = unsafe { GetKeyState(VK_CONTROL) } as u16 & 0x8000 != 0;
            if ctrl_pressed {
                let delta = f32::from(delta) / 120.0;
                app_state.font_size += delta;
                app_state.font_size = app_state.font_size.max(MIN_FONT_SIZE);
                app_state.font_size = app_state.font_size.min(MAX_FONT_SIZE);
                let tf = create_text_format(&app_state.dwrite_factory, app_state.font_size);
                app_state.resources.text_format = tf.clone();
                app_state.view_state.change_text_format(tf.clone());
                invalidate_rect(app_state.hwnd);
            } else {
                let mut scroll_lines: UINT = 0;
                let res = unsafe {
                    SystemParametersInfoW(
                        SPI_GETWHEELSCROLLLINES,
                        0,
                        &mut scroll_lines as *mut _ as *mut _,
                        0)};
                assert!(res != 0, "{}", Error::last_os_error());
                let delta = f32::from(delta) / 120.0 * scroll_lines as f32;
                app_state.view_state.scroll(delta);
            }
            invalidate_rect(app_state.hwnd);
            0
        }
        WM_CHAR => {
            let c: char = std::char::from_u32(wParam as u32).unwrap();
            info!("WM_CHAR {:?}", c);
            if wParam >= 32 || wParam == 9 /* tab */ {
                let app_state = &mut get_app_state(hWnd);
                let mut app_state = app_state.borrow_mut();
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
            let ke = key_util::KeyEvent::new(wParam, lParam);
            info!("WM_KEYDOWN {:?}", ke);
            let app_state = &mut get_app_state(hWnd);
            handle_keydown(app_state, ke);
            0
        }
        WM_SYSKEYDOWN => {
            let ke = key_util::KeyEvent::new(wParam, lParam);
            info!("WM_SYSKEYDOWN {:?}", ke);
            let app_state = &mut get_app_state(hWnd);
            let cmd = app_state.borrow_mut().match_key_event(&ke);

            if let Some(cmd) = cmd {
                send_message(app_state, WM_COMMAND, cmd as usize, 0);
                0
            } else {
                unsafe { DefWindowProcW(hWnd, msg, wParam, lParam) }
            }
        }
        WM_SYSCHAR => {
            info!("WM_SYSCHAR");
            // Default window proc for this event is utterly useless and even
            // harmful.
            // Alt-F supposed to open "File" menu?
            // Yes, but only in English layout.
            // In addition, unrecognized keys make annoying bell sound.
            // So it's better to just sacrifice Alt-F functionality that's
            // broken anyway.
            0
        }
        _ => unsafe { DefWindowProcW(hWnd, msg, wParam, lParam) }
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
    log::error!("{}", message);
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
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, DefWindowProcW as usize as isize)
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
    env_logger::init();

    std::panic::set_hook(Box::new(panic_hook));
    let hwnd = create_window("an_editor", "window title", Some(my_window_proc))?;
    unsafe {
        STATIC_HWND = Some(hwnd);
    }
    loop {
        unsafe {
            let mut message: MSG = mem::zeroed();
            let res = GetMessageW(&mut message, null_mut(), 0, 0);
            if res < 0 {
                return Err(Error::last_os_error());
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
