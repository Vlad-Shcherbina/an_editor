use std::ptr::null_mut;

use winapi::shared::winerror::S_OK;
use winapi::um::dwrite::*;
use winapi::um::d2d1::*;

use super::com_ptr::ComPtr;

pub struct ViewFrame {
    pub width: f32,
    pub height: f32,
    pub text_format: ComPtr<IDWriteTextFormat>,
    pub dwrite_factory: ComPtr<IDWriteFactory>,
}

impl ViewFrame {
    fn create_text_layout(&self, text: &[char]) -> ComPtr<IDWriteTextLayout> {
        let text: String = text.iter().collect();
        let text = super::win32_string(&text);
        unsafe {
            let mut text_layout = null_mut();
            let hr = self.dwrite_factory.CreateTextLayout(
                text.as_ptr(),
                (text.len() - 1) as u32,
                self.text_format.as_raw(),
                self.width,
                self.height,
                &mut text_layout,
            );
            assert!(hr == S_OK, "0x{:x}", hr);
            ComPtr::from_raw(text_layout)
        }
    }
}

pub struct ViewState {
    text: Vec<char>,
    cursor_pos: usize,
    text_layout: ComPtr<IDWriteTextLayout>,
}

impl ViewState {
    pub fn new(view_frame: &ViewFrame) -> ViewState {
        let text: Vec<char> = "hello".chars().collect();
        let text_layout = view_frame.create_text_layout(&text);
        ViewState {
            text,
            cursor_pos: 0,
            text_layout,
        }
    }

    pub fn insert_char(&mut self, view_frame: &ViewFrame, c: char) -> bool {
        self.text.insert(self.cursor_pos, c);
        self.cursor_pos += 1;
        self.text_layout = view_frame.create_text_layout(&self.text);
        true
    }

    pub fn backspace(&mut self, view_frame: &ViewFrame) -> bool {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.text.remove(self.cursor_pos);
            self.text_layout = view_frame.create_text_layout(&self.text);
            true
        } else {
            false
        }
    }

    pub fn del(&mut self, view_frame: &ViewFrame) -> bool {
        if self.cursor_pos < self.text.len() {
            self.text.remove(self.cursor_pos);
            self.text_layout = view_frame.create_text_layout(&self.text);
            true
        } else {
            false
        }
    }

    pub fn left(&mut self, _view_frame: &ViewFrame) -> bool {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            true
        } else {
            false
        }
    }

    pub fn right(&mut self, _view_frame: &ViewFrame) -> bool {
        if self.cursor_pos < self.text.len() {
            self.cursor_pos += 1;
            true
        } else {
            false
        }
    }

    pub fn resize(&mut self, view_frame: &mut ViewFrame, width: f32, height: f32) {
        view_frame.width = width;
        view_frame.height = height;
        self.text_layout = view_frame.create_text_layout(&self.text);
    }

    pub fn render(
        &self,
        origin: D2D1_POINT_2F,
        rt: &ComPtr<ID2D1HwndRenderTarget>,
        brush: &ComPtr<ID2D1Brush>,
    ) {
        unsafe {
            rt.DrawTextLayout(
                origin,
                self.text_layout.as_raw(),
                brush.as_raw(),
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }

        let mut x = 0.0;
        let mut y = 0.0;
        let mut metrics = unsafe { std::mem::zeroed() };
        unsafe {
            let hr = self.text_layout.HitTestTextPosition(
                self.cursor_pos as u32,
                0,  // isTrailingHit
                &mut x, &mut y,
                &mut metrics,
            );
            assert!(hr == S_OK, "0x{:x}", hr);
        }
        x = x.floor();
        unsafe {
            rt.DrawLine(
                D2D1_POINT_2F {
                    x: origin.x + x,
                    y: origin.y + y },
                D2D1_POINT_2F {
                    x: origin.x + x,
                    y: origin.y + y + metrics.height,
                },
                brush.as_raw(),
                2.0,  // strokeWidth
                null_mut(),  // strokeStyle
            );
        }
    }
}
