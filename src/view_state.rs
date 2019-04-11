use std::ptr::null_mut;

use winapi::um::dwrite::*;
use winapi::um::d2d1::*;

use super::com_ptr::ComPtr;
use super::text_layout::TextLayout;

pub struct ViewFrame {
    pub width: f32,
    pub height: f32,
    pub text_format: ComPtr<IDWriteTextFormat>,
    pub dwrite_factory: ComPtr<IDWriteFactory>,
}

impl ViewFrame {
    fn create_text_layout(&self, text: &[char]) -> TextLayout {
        let text: String = text.iter().collect();
        TextLayout::new(&text, &self.dwrite_factory, &self.text_format, self.width)
    }
}

pub struct ViewState {
    text: Vec<char>,
    cursor_pos: usize,
    text_layout: TextLayout,
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

    pub fn click(&mut self, _view_frame: &mut ViewFrame, x: f32, y: f32) -> bool {
        let pos = self.text_layout.coords_to_pos(x, y);
        assert!(pos <= self.text.len());
        if self.cursor_pos != pos {
            self.cursor_pos = pos;
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
                self.text_layout.raw.as_raw(),
                brush.as_raw(),
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }

        let (x, y) = self.text_layout.cursor_coords(self.cursor_pos);
        let x = x.floor();
        unsafe {
            rt.DrawLine(
                D2D1_POINT_2F {
                    x: origin.x + x,
                    y: origin.y + y },
                D2D1_POINT_2F {
                    x: origin.x + x,
                    y: origin.y + y + self.text_layout.line_height,
                },
                brush.as_raw(),
                2.0,  // strokeWidth
                null_mut(),  // strokeStyle
            );
        }
    }
}
