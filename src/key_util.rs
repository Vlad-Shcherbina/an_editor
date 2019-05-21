use std::fmt::{Debug, Formatter, Result};

use winapi::shared::minwindef::*;
use winapi::um::winuser::*;

pub struct KeyEvent {
    pub ctrl_pressed: bool,
    pub shift_pressed: bool,
    pub alt_pressed: bool,
    pub key_code: i32,
    pub scan_code: i32,
}

impl Debug for KeyEvent {
    fn fmt(&self, f: &mut Formatter) -> Result {
        f.debug_struct("KeyEvent")
            .field("ctrl_pressed", &self.ctrl_pressed)
            .field("shift_pressed", &self.shift_pressed)
            .field("alt_pressed", &self.alt_pressed)
            .field("key_code", &format_args!("0x{:02X}", self.key_code))
            .field("scan_code", &format_args!("0x{:02X}", self.scan_code))
            .finish()
    }
}

impl KeyEvent {
    pub fn new(w_param: WPARAM, l_param: LPARAM) -> Self {
        let ctrl_pressed = unsafe { GetKeyState(VK_CONTROL) } as u16 & 0x8000 != 0;
        let shift_pressed = unsafe { GetKeyState(VK_SHIFT) } as u16 & 0x8000 != 0;
        let alt_pressed = unsafe { GetKeyState(VK_MENU) } as u16 & 0x8000 != 0;
        let key_code = w_param as i32;
        let scan_code = ((l_param >> 16) & 511) as i32;
        Self {
            ctrl_pressed,
            shift_pressed,
            alt_pressed,
            key_code,
            scan_code,
        }
    }
}

pub struct KeyMatcher {
    ctrl: bool,
    shift: bool,
    alt: bool,
    key_code: Option<i32>,
    scan_code: Option<i32>,
}

impl KeyMatcher {
    pub fn from_key_code(x: i32) -> Self {
        Self {
            ctrl: false,
            shift: false,
            alt: false,
            key_code: Some(x),
            scan_code: None,
        }
    }
    pub fn from_scan_code(x: i32) -> Self {
        Self {
            ctrl: false,
            shift: false,
            alt: false,
            key_code: None,
            scan_code: Some(x),
        }
    }
    pub fn from_char_to_scan_code(c: char) -> Self {
        let res = unsafe { MapVirtualKeyW(c as u32, MAPVK_VK_TO_VSC) };
        assert!(res != 0, "{:?}", c);
        Self::from_scan_code(res as i32)
    }

    pub fn matches(&self, ke: &KeyEvent) -> bool {
        if self.ctrl != ke.ctrl_pressed {
            return false;
        }
        if self.shift != ke.shift_pressed {
            return false;
        }
        if let Some(x) = self.key_code {
            if x != ke.key_code {
                return false;
            }
        }
        if let Some(x) = self.scan_code {
            if x != ke.scan_code {
                return false;
            }
        }
        true
    }
}

pub struct Modifier {
    ctrl: bool,
    shift: bool,
    alt: bool,
}

impl std::ops::Add<KeyMatcher> for Modifier {
    type Output = KeyMatcher;
    fn add(self, km: KeyMatcher) -> KeyMatcher {
        assert!(!(self.ctrl && km.ctrl));
        assert!(!(self.shift && km.shift));
        assert!(!(self.alt && km.alt));
        KeyMatcher {
            ctrl: self.ctrl || km.ctrl,
            shift: self.shift || km.shift,
            alt: self.alt || km.alt,
            key_code: km.key_code,
            scan_code: km.scan_code,
        }
    }
}

pub const CTRL: Modifier = Modifier { ctrl: true, shift: false, alt: false };
pub const SHIFT: Modifier = Modifier { ctrl: false, shift: true, alt: false };
pub const ALT: Modifier = Modifier { ctrl: false, shift: false, alt: true };
