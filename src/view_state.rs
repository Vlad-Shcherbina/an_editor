use std::ptr::null_mut;

use winapi::um::dwrite::*;
use winapi::um::d2d1::*;

use super::com_ptr::ComPtr;
use super::text_layout::TextLayout;
use super::line_gap_buffer::LineGapBuffer;

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
    document: LineGapBuffer<Option<TextLayout>>,
    cursor_pos: usize,
}

impl ViewState {
    pub fn new(_view_frame: &ViewFrame) -> ViewState {
        let mut text = "hello, world".to_owned();
        for _ in 0..5 {
            text.push_str("\nLorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.");
        }
        let text: Vec<char> = text.chars().collect();
        let mut document = LineGapBuffer::new();
        document.replace_slice(0, 0, &text);

        // move gap to the beginning to avoid delay on first edit
        document.replace_slice(0, 0, &[]);

        ViewState {
            document,
            cursor_pos: 0,
        }
    }

    pub fn insert_char(&mut self, _view_frame: &ViewFrame, c: char) -> bool {
        self.document.replace_slice(self.cursor_pos, self.cursor_pos, &[c]);
        self.cursor_pos += 1;
        true
    }

    pub fn backspace(&mut self, _view_frame: &ViewFrame) -> bool {
        if self.cursor_pos > 0 {
            self.cursor_pos -=1;
            self.document.replace_slice(self.cursor_pos, self.cursor_pos + 1, &[]);
            true
        } else {
            false
        }
    }

    pub fn del(&mut self, view_frame: &ViewFrame) -> bool {
        if self.right(view_frame) {
            let changed = self.backspace(view_frame);
            assert!(changed);
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
        if self.cursor_pos < self.document.len() {
            self.cursor_pos += 1;
            true
        } else {
            false
        }
    }

    fn ensure_layout(&mut self, view_frame: &mut ViewFrame, line_no: usize) {
        let line = self.document.get_line(line_no);
        if line.data.is_none() {
            let line_text = self.document.slice_string(line.start, line.end);
            let line_text: Vec<char> = line_text.chars().collect();
            let layout = view_frame.create_text_layout(&line_text);
            let line = self.document.get_line_mut(line_no);
            *line.data = Some(layout);
        }
    }

    pub fn click(&mut self, view_frame: &mut ViewFrame, x: f32, y: f32) -> bool {
        let mut y0 = 0.0;
        for i in 0..self.document.num_lines() {
            self.ensure_layout(view_frame, i);
            let line = self.document.get_line(i);
            let layout = line.data.as_ref().unwrap();
            if (i == 0 || y >= y0) &&
               (i + 1 == self.document.num_lines() || y < y0 + layout.height) {
                let pos = layout.coords_to_pos(x, y - y0);
                assert!(pos <= line.end - line.start);
                self.cursor_pos = line.start + pos;
                return true;
            }
            y0 += layout.height;
        }
        unreachable!()
    }

    pub fn resize(&mut self, view_frame: &mut ViewFrame, width: f32, height: f32) {
        view_frame.width = width;
        view_frame.height = height;
        for i in 0..self.document.num_lines() {
            *self.document.get_line_mut(i).data = None;
        }
    }

    pub fn render(
        &mut self,
        view_frame: &mut ViewFrame,
        origin: D2D1_POINT_2F,
        rt: &ComPtr<ID2D1HwndRenderTarget>,
        brush: &ComPtr<ID2D1Brush>,
    ) {
        let mut y0 = 0.0;
        for i in 0..self.document.num_lines() {
            if y0 > view_frame.height {
                break;
            }
            self.ensure_layout(view_frame, i);
            let line = self.document.get_line(i);
            let layout = line.data.as_ref().unwrap();
            unsafe {
                rt.DrawTextLayout(
                    D2D1_POINT_2F { x: origin.x, y: origin.y + y0},
                    layout.raw.as_raw(),
                    brush.as_raw(),
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
            if line.start <= self.cursor_pos && self.cursor_pos <= line.end {
                let (x, y) = layout.cursor_coords(self.cursor_pos - line.start);
                let x = x.floor();
                unsafe {
                    rt.DrawLine(
                        D2D1_POINT_2F {
                            x: origin.x + x,
                            y: origin.y + y0 + y },
                        D2D1_POINT_2F {
                            x: origin.x + x,
                            y: origin.y + y0 + y + layout.line_height,
                        },
                        brush.as_raw(),
                        2.0,  // strokeWidth
                        null_mut(),  // strokeStyle
                    );
                }
            }
            y0 += layout.height;
        }
    }
}
