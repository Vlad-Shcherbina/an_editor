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

struct Line {
    text: Vec<char>,
    layout: TextLayout,
}

pub struct ViewState {
    lines: Vec<Line>,  // never empty
    cursor_row: usize,
    cursor_col: usize,
}

impl ViewState {
    pub fn new(view_frame: &ViewFrame) -> ViewState {
        let mut text = "hello, world".to_owned();
        for _ in 0..5 {
            text.push_str("\nLorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.");
        }
        let lines = text.split('\n').map(|line| {
            let text: Vec<char> = line.chars().collect();
            let layout = view_frame.create_text_layout(&text);
            Line { text, layout }
        }).collect();
        ViewState {
            lines,
            cursor_row: 0,
            cursor_col: 0,
        }
    }

    pub fn insert_char(&mut self, view_frame: &ViewFrame, c: char) -> bool {
        if c == '\n' {
            let next_text: Vec<char> =
                self.lines[self.cursor_row].text[self.cursor_col..].to_vec();
            let next_layout = view_frame.create_text_layout(&next_text);

            let line = &mut self.lines[self.cursor_row];
            line.text.truncate(self.cursor_col);
            line.layout = view_frame.create_text_layout(&line.text);
            self.cursor_row += 1;
            self.lines.insert(self.cursor_row, Line {
                text: next_text,
                layout: next_layout,
            });
            self.cursor_col = 0;
        } else {
            let line = &mut self.lines[self.cursor_row];
            line.text.insert(self.cursor_col, c);
            self.cursor_col += 1;
            line.layout = view_frame.create_text_layout(&line.text);
        }
        true
    }

    pub fn backspace(&mut self, view_frame: &ViewFrame) -> bool {
        if self.cursor_col > 0 {
            let line = &mut self.lines[self.cursor_row];
            self.cursor_col -= 1;
            line.text.remove(self.cursor_col);
            line.layout = view_frame.create_text_layout(&line.text);
            true
        } else if self.cursor_row > 0 {
            let tail = self.lines.remove(self.cursor_row).text;
            self.cursor_row -= 1;
            let line = &mut self.lines[self.cursor_row];
            self.cursor_col = line.text.len();
            line.text.extend_from_slice(&tail);
            line.layout = view_frame.create_text_layout(&line.text);
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
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
            true
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.lines[self.cursor_row].text.len();
            true
        } else {
            false
        }
    }

    pub fn right(&mut self, _view_frame: &ViewFrame) -> bool {
        if self.cursor_col < self.lines[self.cursor_row].text.len() {
            self.cursor_col += 1;
            true
        } else if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
            true
        } else {
            false
        }
    }

    pub fn click(&mut self, _view_frame: &mut ViewFrame, x: f32, y: f32) -> bool {
        let mut y0 = 0.0;
        for (i, line) in self.lines.iter().enumerate() {
            if (i == 0 || y >= y0) &&
               (i + 1 == self.lines.len() || y < y0 + line.layout.height) {
                let pos = line.layout.coords_to_pos(x, y - y0);
                assert!(pos <= line.text.len());
                self.cursor_row = i;
                self.cursor_col = pos;
                return true;
            }
            y0 += line.layout.height;
        }
        unreachable!()
    }

    pub fn resize(&mut self, view_frame: &mut ViewFrame, width: f32, height: f32) {
        view_frame.width = width;
        view_frame.height = height;
        for line in self.lines.iter_mut() {
            line.layout = view_frame.create_text_layout(&line.text);
        }
    }

    pub fn render(
        &self,
        origin: D2D1_POINT_2F,
        rt: &ComPtr<ID2D1HwndRenderTarget>,
        brush: &ComPtr<ID2D1Brush>,
    ) {
        let mut y0 = origin.y;
        for (i, line) in self.lines.iter().enumerate() {
            unsafe {
                rt.DrawTextLayout(
                    D2D1_POINT_2F { x: origin.x, y: y0},
                    line.layout.raw.as_raw(),
                    brush.as_raw(),
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
            if i == self.cursor_row {
                let (x, y) = line.layout.cursor_coords(self.cursor_col);
                let x = x.floor();
                unsafe {
                    rt.DrawLine(
                        D2D1_POINT_2F {
                            x: origin.x + x,
                            y: y0 + y },
                        D2D1_POINT_2F {
                            x: origin.x + x,
                            y: y0 + y + line.layout.line_height,
                        },
                        brush.as_raw(),
                        2.0,  // strokeWidth
                        null_mut(),  // strokeStyle
                    );
                }
            }
            y0 += line.layout.height;
        }
    }
}
