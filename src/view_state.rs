use std::ptr::null_mut;

use winapi::um::dwrite::*;
use winapi::um::d2d1::*;

use super::com_ptr::ComPtr;
use super::text_layout::TextLayout;
use super::line_gap_buffer::{Line, LineGapBuffer};

#[derive(Debug)]
struct SliceEdit {
    old_text: String,
    start: usize,
    end: usize,
}

#[derive(Debug)]
struct UndoSnapshot {
    slice_edit_count: usize,
    cursor_pos: usize,
}

pub struct ViewState {
    width: f32,
    height: f32,
    text_format: ComPtr<IDWriteTextFormat>,
    dwrite_factory: ComPtr<IDWriteFactory>,

    document: LineGapBuffer<Option<TextLayout>>,
    cursor_pos: usize,
    selection_pos: usize,

    // for screen positioning relative to the document
    anchor_pos: usize,
    anchor_y: f32,

    // for vertical navigation using up, down, pgup, pgdown
    anchor_x: f32,

    undo_slice_edits: Vec<SliceEdit>,
    undo_snapshots: Vec<UndoSnapshot>,
    redo_slice_edits: Vec<SliceEdit>,
    redo_snapshots: Vec<UndoSnapshot>,

    unmodified_snapshot: Option<usize>,
}

impl ViewState {
    pub fn new(
        width: f32,
        height: f32,
        text_format: ComPtr<IDWriteTextFormat>,
        dwrite_factory: ComPtr<IDWriteFactory>,
    ) -> ViewState {
        ViewState {
            width,
            height,
            text_format,
            dwrite_factory,
            document: LineGapBuffer::new(),
            cursor_pos: 0,
            selection_pos: 0,
            anchor_pos: 0,
            anchor_y: 0.0,
            anchor_x: 0.0,
            undo_slice_edits: Vec::new(),
            undo_snapshots: Vec::new(),
            redo_slice_edits: Vec::new(),
            redo_snapshots: Vec::new(),
            unmodified_snapshot: Some(0),
        }
    }

    pub fn change_text_format(&mut self, text_format: ComPtr<IDWriteTextFormat>) {
        self.text_format = text_format;
        for i in 0..self.document.num_lines() {
            *self.document.get_line_mut(i).data = None;
        }
    }

    fn replace_slice_and_get_edit(&mut self, start: usize, end: usize, text: &[char]) -> Option<SliceEdit> {
        if self.document.slice_string(start, end).chars().eq(text.iter().cloned()) {
            return None;
        }
        let result = SliceEdit {
            start,
            end: start + text.len(),
            old_text: self.document.slice_string(start, end),
        };
        self.document.replace_slice(start, end, text);
        Some(result)
    }

    fn replace_slice(&mut self, start: usize, end: usize, text: &[char]) {
        let u = self.replace_slice_and_get_edit(start, end, text);
        assert!(!self.undo_snapshots.is_empty());
        self.undo_slice_edits.extend(u.into_iter());
    }

    pub fn modified(&self) -> bool {
        self.unmodified_snapshot != Some(self.undo_snapshots.len())
    }

    pub fn make_undo_snapshot(&mut self) {
        if let Some(&UndoSnapshot { slice_edit_count, cursor_pos }) = self.undo_snapshots.last() {
            if cursor_pos == self.cursor_pos && slice_edit_count == self.undo_slice_edits.len() {
                return;
            }
        }
        self.undo_snapshots.push(UndoSnapshot {
            slice_edit_count: self.undo_slice_edits.len(),
            cursor_pos: self.cursor_pos,
        });
        self.redo_snapshots.clear();
        self.redo_slice_edits.clear();
        if let Some(n) = self.unmodified_snapshot {
            if n > self.redo_snapshots.len() {
                self.unmodified_snapshot = None;
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_snapshots.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(UndoSnapshot { slice_edit_count, cursor_pos }) = self.undo_snapshots.pop() {
            self.redo_snapshots.push(UndoSnapshot {
                slice_edit_count: self.redo_slice_edits.len(),
                cursor_pos: self.cursor_pos,
            });
            while self.undo_slice_edits.len() > slice_edit_count {
                let SliceEdit { start, end, old_text} = self.undo_slice_edits.pop().unwrap();
                let old_text: Vec<char> = old_text.chars().collect();
                let re = self.replace_slice_and_get_edit(start, end, &old_text);
                self.redo_slice_edits.extend(re.into_iter());
            }
            self.cursor_pos = cursor_pos;
            self.clear_selection();
        }
        self.ensure_cursor_on_screen();
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_snapshots.is_empty()
    }

    pub fn redo(&mut self) {
        std::mem::swap(&mut self.undo_snapshots, &mut self.redo_snapshots);
        std::mem::swap(&mut self.undo_slice_edits, &mut self.redo_slice_edits);
        self.undo();
        std::mem::swap(&mut self.undo_snapshots, &mut self.redo_snapshots);
        std::mem::swap(&mut self.undo_slice_edits, &mut self.redo_slice_edits);
    }

    pub fn load(&mut self, text: &str, initially_modified: bool) {
        let text: Vec<char> = text.chars().collect();
        self.document.replace_slice(0, self.document.len(), &text);
        self.undo_snapshots.clear();
        self.undo_slice_edits.clear();
        self.redo_snapshots.clear();
        self.redo_slice_edits.clear();
        // move gap to the beginning to avoid delay on first edit
        self.document.replace_slice(0, 0, &[]);
        self.cursor_pos = 0;
        self.selection_pos = 0;
        self.anchor_pos = 0;
        self.anchor_y = 0.0;
        self.anchor_x = 0.0;
        self.unmodified_snapshot = if initially_modified { None } else { Some(0) };
    }

    pub fn set_unmodified_snapshot(&mut self) {
        self.unmodified_snapshot = Some(self.undo_snapshots.len());
    }

    pub fn content(&self) -> String {
        self.document.slice_string(0, self.document.len())
    }

    pub fn clear_selection(&mut self) {
        self.selection_pos = self.cursor_pos;
    }

    pub fn select_all(&mut self) {
        self.selection_pos = 0;
        self.cursor_pos = self.document.len();
    }

    pub fn paste(&mut self, s: &str) {
        let s: Vec<char> = s.chars().collect();
        if self.selection_pos != self.cursor_pos {
            let a = self.cursor_pos.min(self.selection_pos);
            let b = self.cursor_pos.max(self.selection_pos);
            self.replace_slice(a, b, &s);
            self.cursor_pos = a + s.len();
            self.clear_selection();
            self.ensure_cursor_on_screen();
            return;
        }
        self.replace_slice(self.cursor_pos, self.cursor_pos, &s);
        self.cursor_pos += s.len();
        self.clear_selection();
        self.ensure_cursor_on_screen();
    }

    pub fn has_selection(&self) -> bool {
        self.cursor_pos != self.selection_pos
    }

    pub fn get_selection(&self) -> String {
        let a = self.cursor_pos.min(self.selection_pos);
        let b = self.cursor_pos.max(self.selection_pos);
        self.document.slice_string(a, b)
    }

    pub fn cut_selection(&mut self) -> String {
        let a = self.cursor_pos.min(self.selection_pos);
        let b = self.cursor_pos.max(self.selection_pos);
        let result = self.document.slice_string(a, b);
        self.replace_slice(a, b, &[]);
        self.cursor_pos = a;
        self.clear_selection();
        self.ensure_cursor_on_screen();
        self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
        result
    }

    pub fn insert_char(&mut self, c: char) {
        if self.selection_pos != self.cursor_pos {
            let a = self.cursor_pos.min(self.selection_pos);
            let b = self.cursor_pos.max(self.selection_pos);
            self.replace_slice(a, b, &[c]);
            self.cursor_pos = a + 1;
            self.clear_selection();
            self.ensure_cursor_on_screen();
            self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
            return;
        }
        self.replace_slice(self.cursor_pos, self.cursor_pos, &[c]);
        self.cursor_pos += 1;
        self.clear_selection();
        self.ensure_cursor_on_screen();
        self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
    }

    pub fn backspace(&mut self) {
        if self.selection_pos != self.cursor_pos {
            let a = self.cursor_pos.min(self.selection_pos);
            let b = self.cursor_pos.max(self.selection_pos);
            self.replace_slice(a, b, &[]);
            self.cursor_pos = a;
            self.clear_selection();
            self.ensure_cursor_on_screen();
            self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
            return;
        }
        if self.cursor_pos > 0 {
            self.cursor_pos -=1;
            self.replace_slice(self.cursor_pos, self.cursor_pos + 1, &[]);
            self.clear_selection();
            self.ensure_cursor_on_screen();
            self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
        }
    }

    pub fn del(&mut self) {
        if self.selection_pos != self.cursor_pos {
            let a = self.cursor_pos.min(self.selection_pos);
            let b = self.cursor_pos.max(self.selection_pos);
            self.replace_slice(a, b, &[]);
            self.cursor_pos = a;
            self.clear_selection();
            self.ensure_cursor_on_screen();
            self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
            return;
        }
        if self.cursor_pos < self.document.len() {
            self.replace_slice(self.cursor_pos, self.cursor_pos + 1, &[]);
            self.clear_selection();
            self.ensure_cursor_on_screen();
            self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
        }
    }

    pub fn left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.ensure_cursor_on_screen();
            self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
        }
    }

    pub fn right(&mut self) {
        if self.cursor_pos < self.document.len() {
            self.cursor_pos += 1;
            self.ensure_cursor_on_screen();
            self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
        }
    }

    pub fn ctrl_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
        }
        while self.cursor_pos > 0 {
            if self.document.get_char(self.cursor_pos - 1).is_whitespace() &&
                !self.document.get_char(self.cursor_pos).is_whitespace() {
                break;
            }
            self.cursor_pos -= 1;
        }
        self.ensure_cursor_on_screen();
        self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
    }

    pub fn ctrl_right(&mut self) {
        while self.cursor_pos < self.document.len() {
            self.cursor_pos += 1;
            if self.cursor_pos == self.document.len() {
                break;
            }
            if !self.document.get_char(self.cursor_pos - 1).is_whitespace() &&
                self.document.get_char(self.cursor_pos).is_whitespace() {
                break;
            }
        }
        self.ensure_cursor_on_screen();
        self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
    }

    pub fn home(&mut self) {
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
        self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
    }

    pub fn end(&mut self) {
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
        self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
    }

    pub fn ctrl_home(&mut self) {
        self.cursor_pos = 0;
        self.ensure_cursor_on_screen();
        self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
    }

    pub fn ctrl_end(&mut self) {
        self.cursor_pos = self.document.len();
        self.ensure_cursor_on_screen();
        self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
    }

    pub fn up(&mut self) {
        let (_x, y) = self.pos_to_coord(self.cursor_pos);

        let line_no = self.document.find_line(self.cursor_pos);
        self.ensure_layout(line_no);
        let line = self.document.get_line(line_no);
        let layout = line.data.as_ref().unwrap();
        // TODO: what if line above has different height?
        self.cursor_pos = self.coord_to_pos(self.anchor_x, y - layout.line_height * 0.5);
        self.ensure_cursor_on_screen();
    }

    pub fn down(&mut self) {
        let (_x, y) = self.pos_to_coord(self.cursor_pos);

        let line_no = self.document.find_line(self.cursor_pos);
        self.ensure_layout(line_no);
        let line = self.document.get_line(line_no);
        let layout = line.data.as_ref().unwrap();
        // TODO: what if line below has different height?
        self.cursor_pos = self.coord_to_pos(self.anchor_x, y + layout.line_height * 1.5);
        self.ensure_cursor_on_screen();
    }

    pub fn scroll(&mut self, delta: f32) {
        let line_no = self.document.find_line(self.cursor_pos);
        self.ensure_layout(line_no);
        let line = self.document.get_line(line_no);
        let layout = line.data.as_ref().unwrap();
        // TODO: what if lines have different heights
        self.anchor_y += delta * layout.line_height;
        self.clip_scroll_position_to_document();
    }

    pub fn pg_up(&mut self) {
        let (_x, y) = self.pos_to_coord(self.cursor_pos);

        let line_no = self.document.find_line(self.cursor_pos);
        self.ensure_layout(line_no);
        let line = self.document.get_line(line_no);
        let layout = line.data.as_ref().unwrap();
        // TODO: what if lines has different heights?
        self.cursor_pos = self.coord_to_pos(
            self.anchor_x, y + layout.line_height * 1.5 - self.height);
        self.ensure_cursor_on_screen();
    }

    pub fn pg_down(&mut self) {
        let (_x, y) = self.pos_to_coord(self.cursor_pos);

        let line_no = self.document.find_line(self.cursor_pos);
        self.ensure_layout(line_no);
        let line = self.document.get_line(line_no);
        let layout = line.data.as_ref().unwrap();
        // TODO: what if lines has different heights?
        self.cursor_pos = self.coord_to_pos(
            self.anchor_x, y - layout.line_height * 0.5 + self.height);
        self.ensure_cursor_on_screen();
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
        self.clip_scroll_position_to_document();
    }

    fn clip_scroll_position_to_document(&mut self) {
        let (anchor_line, anchor_line_y) = self.anchor_line_and_y();
        let (y1, line_no1, line_no2) =
            self.lines_on_screen(anchor_line, anchor_line_y);
        if y1 > 0.0 {
            assert!(line_no1 == 0);
            self.anchor_y -= y1;
        } else if line_no2 == self.document.num_lines() {
            let (_x, y2) = self.pos_to_coord(self.document.len());
            if y2 < 0.0 {
                self.anchor_y -= y2;
            }
        }
        self.anchor_to_top();
    }

    fn anchor_to_top(&mut self) {
        let (anchor_line, anchor_line_y) = self.anchor_line_and_y();
        let (_y0, line_no1, line_no2) =
            self.lines_on_screen(anchor_line, anchor_line_y);

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

    pub fn coord_to_pos(&mut self, x: f32, y: f32) -> usize {
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
                return line.start + pos;
            }
            i += 1;
            y0 += layout.height;
        }
    }

    pub fn click(&mut self, x: f32, y: f32) {
        self.cursor_pos = self.coord_to_pos(x, y);
        self.ensure_cursor_on_screen();
        self.anchor_x = self.pos_to_coord(self.cursor_pos).0;
    }

    pub fn double_click(&mut self, x: f32, y: f32) {
        let pos = self.coord_to_pos(x, y);
        let mut start = pos;
        while start > 0 {
            if !self.document.get_char(start - 1).is_alphanumeric() {
                break;
            }
            start -= 1;
        }
        let mut end = pos;
        while end < self.document.len() {
            if !self.document.get_char(end).is_alphanumeric() {
                break;
            }
            end += 1;
        }
        self.selection_pos = start;
        self.cursor_pos = end;
        self.ensure_cursor_on_screen();
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
        selection_brush: &ComPtr<ID2D1Brush>,
    ) {
        let (anchor_line, anchor_line_y) = self.anchor_line_and_y();
        let (mut y0, line_no1, line_no2) =
            self.lines_on_screen(anchor_line, anchor_line_y);
        let selection_start = self.cursor_pos.min(self.selection_pos);
        let selection_end = self.cursor_pos.max(self.selection_pos);
        for i in line_no1..line_no2 {
            self.ensure_layout(i);
            let line = self.document.get_line(i);
            let layout = line.data.as_ref().unwrap();

            let sel_start = selection_start.max(line.start);
            let sel_end = selection_end.min(line.end + 1);
            if sel_start < sel_end {
                let rs = layout.get_selection_rects(sel_start - line.start, sel_end - line.start);
                for (left, top, w, h) in rs {
                    let rect = D2D1_RECT_F {
                        left: left + origin.x,
                        top: top + y0 + origin.y,
                        right: left + w + origin.x,
                        bottom: top + h + y0 + origin.y,
                    };
                    unsafe {
                        rt.FillRectangle(&rect, selection_brush.as_raw());
                    }
                }
            }

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
        // it could be out of range after an edit
        self.anchor_pos = self.anchor_pos.min(self.document.len());

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
