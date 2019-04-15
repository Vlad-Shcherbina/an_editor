use std::ptr::null_mut;

use winapi::um::dwrite::*;
use winapi::um::d2d1::*;

use super::com_ptr::ComPtr;
use super::text_layout::TextLayout;
use super::line_gap_buffer::{Line, LineGapBuffer};

pub struct ViewState {
    width: f32,
    height: f32,
    text_format: ComPtr<IDWriteTextFormat>,
    dwrite_factory: ComPtr<IDWriteFactory>,

    document: LineGapBuffer<Option<TextLayout>>,
    cursor_pos: usize,

    anchor_pos: usize,
    anchor_y: f32,
}

impl ViewState {
    pub fn new(
        width: f32,
        height: f32,
        text_format: ComPtr<IDWriteTextFormat>,
        dwrite_factory: ComPtr<IDWriteFactory>,
    ) -> ViewState {
        let text = std::fs::read_to_string("samples/idiot-dostoievskii.txt").unwrap();
        let text = text.replace("\r\n", "\n");
        let text: Vec<char> = text.chars().collect();
        let mut document = LineGapBuffer::new();
        document.replace_slice(0, 0, &text);

        // move gap to the beginning to avoid delay on first edit
        document.replace_slice(0, 0, &[]);

        ViewState {
            width,
            height,
            text_format,
            dwrite_factory,
            document,
            cursor_pos: 0,
            anchor_pos: 0,
            anchor_y: 0.0,
        }
    }

    pub fn insert_char(&mut self, c: char) -> bool {
        self.document.replace_slice(self.cursor_pos, self.cursor_pos, &[c]);
        self.cursor_pos += 1;
        self.ensure_cursor_on_screen();
        true
    }

    pub fn backspace(&mut self) -> bool {
        if self.cursor_pos > 0 {
            self.cursor_pos -=1;
            self.document.replace_slice(self.cursor_pos, self.cursor_pos + 1, &[]);
            self.ensure_cursor_on_screen();
            true
        } else {
            false
        }
    }

    pub fn del(&mut self) -> bool {
        if self.right() {
            let changed = self.backspace();
            assert!(changed);
            self.ensure_cursor_on_screen();
            true
        } else {
            false
        }
    }

    pub fn left(&mut self) -> bool {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.ensure_cursor_on_screen();
            true
        } else {
            false
        }
    }

    pub fn right(&mut self) -> bool {
        if self.cursor_pos < self.document.len() {
            self.cursor_pos += 1;
            self.ensure_cursor_on_screen();
            true
        } else {
            false
        }
    }

    pub fn home(&mut self) -> bool {
        let line_no = self.document.find_line(self.cursor_pos);
        self.ensure_layout(line_no);
        let line = self.document.get_line(line_no);
        let layout = line.data.as_ref().unwrap();
        let bounds = layout.line_boundaries();
        self.cursor_pos = line.start + bounds.into_iter()
            .filter(|&x| x < self.cursor_pos - line.start)
            .last()
            .unwrap_or(0);
        self.ensure_cursor_on_screen();
        true
    }

    pub fn end(&mut self) -> bool {
        let line_no = self.document.find_line(self.cursor_pos);
        self.ensure_layout(line_no);
        let line = self.document.get_line(line_no);
        let layout = line.data.as_ref().unwrap();
        let bounds = layout.line_boundaries();
        let &end = bounds.last().unwrap();
        self.cursor_pos = line.start + bounds.into_iter()
            .find(|&x| x > self.cursor_pos - line.start)
            .unwrap_or(end);
        self.ensure_cursor_on_screen();
        true
    }

    pub fn ctrl_home(&mut self) -> bool {
        self.cursor_pos = 0;
        self.ensure_cursor_on_screen();
        true
    }

    pub fn ctrl_end(&mut self) -> bool {
        self.cursor_pos = self.document.len();
        self.ensure_cursor_on_screen();
        true
    }

    pub fn up(&mut self) -> bool {
        let (x, y) = self.pos_to_coord(self.cursor_pos);

        let line_no = self.document.find_line(self.cursor_pos);
        self.ensure_layout(line_no);
        let line = self.document.get_line(line_no);
        let layout = line.data.as_ref().unwrap();
        // TODO: what if line above has different height?
        self.click(x, y - layout.line_height * 0.5);
        self.ensure_cursor_on_screen();
        true
    }

    pub fn down(&mut self) -> bool {
        let (x, y) = self.pos_to_coord(self.cursor_pos);

        let line_no = self.document.find_line(self.cursor_pos);
        self.ensure_layout(line_no);
        let line = self.document.get_line(line_no);
        let layout = line.data.as_ref().unwrap();
        // TODO: what if line below has different height?
        self.click(x, y + layout.line_height * 1.5);
        self.ensure_cursor_on_screen();
        true
    }

    pub fn pg_up(&mut self) -> bool {
        let (x, y) = self.pos_to_coord(self.cursor_pos);

        let line_no = self.document.find_line(self.cursor_pos);
        self.ensure_layout(line_no);
        let line = self.document.get_line(line_no);
        let layout = line.data.as_ref().unwrap();
        // TODO: what if lines has different heights?
        self.click(x, y + layout.line_height * 1.5 - self.height);
        self.ensure_cursor_on_screen();
        true
    }

    pub fn pg_down(&mut self) -> bool {
        let (x, y) = self.pos_to_coord(self.cursor_pos);

        let line_no = self.document.find_line(self.cursor_pos);
        self.ensure_layout(line_no);
        let line = self.document.get_line(line_no);
        let layout = line.data.as_ref().unwrap();
        // TODO: what if lines has different heights?
        self.click(x, y - layout.line_height * 0.5 + self.height);
        self.ensure_cursor_on_screen();
        true
    }

    fn ensure_cursor_on_screen(&mut self) {
        // TODO: when jumping large distances it will force layout
        // on all lines in between, it's slow
        let (_x, y) = self.pos_to_coord(self.cursor_pos);
        let i = self.document.find_line(self.cursor_pos);
        self.ensure_layout(i);
        let line = self.document.get_line(i);
        let layout = line.data.as_ref().unwrap();
        if y < 0.0 {
            self.anchor_pos = self.cursor_pos;
            self.anchor_y = 0.0;
        }
        // TODO: what if lines has different heights
        if y + layout.line_height > self.height {
            self.anchor_pos = self.cursor_pos;
            self.anchor_y = self.height - layout.line_height;
        }
        self.anchor_to_top();
    }

    fn anchor_to_top(&mut self) {
        let (anchor_line, anchor_line_y) = self.anchor_line_and_y();
        let (_y0, line_no1, line_no2) =
            self.lines_on_screen(anchor_line, anchor_line_y);
        assert!(line_no1 < self.document.num_lines());

        for line_no in line_no1..line_no2 {
            self.ensure_layout(line_no);
            let line = self.document.get_line(line_no);
            let line_start = line.start;
            let layout = line.data.as_ref().unwrap();
            let bounds = layout.line_boundaries();
            for &b in &bounds[..bounds.len() - 1] {
                let (_x, y) = self.pos_to_coord(line_start + b);
                if y >= 0.0 {
                    self.anchor_pos = line_start + b;
                    self.anchor_y = y;
                    return;
                }
            }
        }
    }

    fn ensure_layout(&mut self, line_no: usize) {
        let line = self.document.get_line(line_no);
        if line.data.is_none() {
            let line_text = self.document.slice_string(line.start, line.end);
            let layout = TextLayout::new(
                &line_text, &self.dwrite_factory, &self.text_format, self.width);
            let line = self.document.get_line_mut(line_no);
            *line.data = Some(layout);
        }
    }

    pub fn click(&mut self, x: f32, y: f32) -> bool {
        let (mut i, mut y0) = self.anchor_line_and_y();
        while i > 0 && y0 > y {
            self.ensure_layout(i - 1);
            let line = self.document.get_line(i - 1);
            let layout = line.data.as_ref().unwrap();
            i -= 1;
            y0 -= layout.height;
        }
        loop {
            self.ensure_layout(i);
            let line = self.document.get_line(i);
            let layout = line.data.as_ref().unwrap();
            if y < y0 + layout.height || i + 1 == self.document.num_lines() {
                let pos = layout.coords_to_pos(x, y - y0);
                assert!(pos <= line.end - line.start);
                self.cursor_pos = line.start + pos;
                self.ensure_cursor_on_screen();
                return true;
            }
            i += 1;
            y0 += layout.height;
        }
    }

    fn pos_to_coord(&mut self, pos: usize) -> (f32, f32) {
        let (anchor_line, anchor_line_y) = self.anchor_line_and_y();
        let line_no = self.document.find_line(pos);
        self.ensure_layout(line_no);
        let line = self.document.get_line(line_no);
        let layout = line.data.as_ref().unwrap();
        let (x, y) = layout.cursor_coords(pos - line.start);
        (x, anchor_line_y + self.vertical_offset(anchor_line, line_no) + y)
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
        for i in 0..self.document.num_lines() {
            *self.document.get_line_mut(i).data = None;
        }
    }

    fn draw_cursor(
        &self,
        x0: f32, y0: f32,
        line: Line<&Option<TextLayout>>,
        rt: &ComPtr<ID2D1HwndRenderTarget>,
        brush: &ComPtr<ID2D1Brush>,
    ) {
        assert!(line.start <= self.cursor_pos && self.cursor_pos <= line.end);
        let layout = line.data.as_ref().unwrap();
        let (x, y) = layout.cursor_coords(self.cursor_pos - line.start);
        let x = x.floor();
        unsafe {
            rt.DrawLine(
                D2D1_POINT_2F {
                    x: x0 + x,
                    y: y0 + y },
                D2D1_POINT_2F {
                    x: x0 + x,
                    y: y0 + y + layout.line_height,
                },
                brush.as_raw(),
                2.0,  // strokeWidth
                null_mut(),  // strokeStyle
            );
        }

        let bounds = layout.line_boundaries();
        assert!(bounds.len() >= 2);
        let bounds = &bounds[1..bounds.len() - 1];
        if bounds.contains(&(self.cursor_pos - line.start)) {
            let (x, y) = layout.cursor_coords_trailing(self.cursor_pos - line.start);
            let x = x.floor();
            unsafe {
                rt.DrawLine(
                    D2D1_POINT_2F {
                        x: x0 + x,
                        y: y0 + y,
                    },
                    D2D1_POINT_2F {
                        x: x0 + x,
                        y: y0 + y + layout.line_height,
                    },
                    brush.as_raw(),
                    2.0,  // strokeWidth
                    null_mut(),  // strokeStyle
                );
            }
        }
    }

    pub fn render(
        &mut self,
        origin: D2D1_POINT_2F,
        rt: &ComPtr<ID2D1HwndRenderTarget>,
        brush: &ComPtr<ID2D1Brush>,
    ) {
        let (anchor_line, anchor_line_y) = self.anchor_line_and_y();
        let (mut y0, line_no1, line_no2) =
            self.lines_on_screen(anchor_line, anchor_line_y);
        for i in line_no1..line_no2 {
            self.ensure_layout(i);
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
                self.draw_cursor(origin.x, origin.y + y0, line, rt, brush);
            }
            y0 += layout.height;
        }
        // TODO: remove, it's only for debugging
        let (x, y) = self.pos_to_coord(self.anchor_pos);
        unsafe {
            rt.DrawLine(
                D2D1_POINT_2F {
                    x: origin.x + x - 2.0,
                    y: origin.y + y + 2.0,
                },
                D2D1_POINT_2F {
                    x: origin.x + x + 2.0,
                    y: origin.y + y + 2.0,
                },
                brush.as_raw(),
                3.0,  // strokeWidth
                null_mut(),  // strokeStyle
            );
        }
    }

    fn vertical_offset(&mut self, mut line_no1: usize, mut line_no2: usize) -> f32 {
        let sign = if line_no1 > line_no2 {
            std::mem::swap(&mut line_no1, &mut line_no2);
            -1.0
        } else {
            1.0
        };
        let mut result = 0.0;
        for i in line_no1..line_no2 {
            self.ensure_layout(i);
            let line = self.document.get_line(i);
            let layout = line.data.as_ref().unwrap();
            result += layout.height;
        }
        result * sign
    }

    fn anchor_line_and_y(&mut self) -> (usize, f32) {
        let anchor_line = self.document.find_line(self.anchor_pos);
        self.ensure_layout(anchor_line);
        let line = self.document.get_line(anchor_line);
        let layout = line.data.as_ref().unwrap();
        let (_x, y) = layout.cursor_coords(self.anchor_pos - line.start);
        let anchor_line_y = self.anchor_y - y;
        (anchor_line, anchor_line_y)
    }

    fn lines_on_screen(&mut self, line_no: usize, line_y: f32) -> (f32, usize, usize) {
        let mut i = line_no;
        let mut y = line_y;
        while i > 0 && y > 0.0 {
            self.ensure_layout(i - 1);
            let line = self.document.get_line(i - 1);
            let layout = line.data.as_ref().unwrap();
            i -= 1;
            y -= layout.height;
        }
        while i < self.document.num_lines() {
            self.ensure_layout(i);
            let line = self.document.get_line(i);
            let layout = line.data.as_ref().unwrap();
            if y + layout.height > 0.0 {
                break;
            }
            i += 1;
            y += layout.height;
        }
        let start_y = y;
        let start_line = i;
        while i < self.document.num_lines() && y < self.height {
            self.ensure_layout(i);
            let line = self.document.get_line(i);
            let layout = line.data.as_ref().unwrap();
            i += 1;
            y += layout.height;
        }
        (start_y, start_line, i)
    }
}
