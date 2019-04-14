#[derive(Debug)]
pub struct Line<T> {
    pub start: usize,
    pub end: usize,
    pub data: T,
}

pub struct LineGapBuffer<T> {
    chars_left: Vec<char>,
    chars_right: Vec<char>,  // reversed

    lines_left: Vec<Line<T>>,
    lines_right: Vec<Line<T>>,  // reversed, start and end flipped (see get_line())
}

impl<T: Default> LineGapBuffer<T> {
    pub fn new() -> Self {
        Self {
            chars_left: Vec::new(),
            chars_right: Vec::new(),
            lines_left: vec![Line { start: 0, end: 0, data: T::default() }],
            lines_right: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.chars_left.len() + self.chars_right.len()
    }

    pub fn num_lines(&self) -> usize {
        self.lines_left.len() + self.lines_right.len()
    }

    fn get_char(&self, pos: usize) -> char {
        if pos < self.chars_left.len() {
            self.chars_left[pos]
        } else {
            self.chars_right[self.len() - 1 - pos]
        }
    }

    pub fn slice_string(&self, start: usize, end: usize) -> String {
        assert!(start <= end && end <= self.len());
        (start..end).map(|i| self.get_char(i)).collect()
    }

    pub fn get_line(&self, line_no: usize) -> Line<&T> {
        let n = self.len();
        let num_lines = self.num_lines();
        assert!(line_no < num_lines);
        if line_no < self.lines_left.len() {
            let line = &self.lines_left[line_no];
            Line {
                start: line.start,
                end: line.end,
                data: &line.data,
            }
        } else {
            let line = &self.lines_right[num_lines - 1 - line_no];
            Line {
                start: n - line.start,
                end: n - line.end,
                data: &line.data,
            }
        }
    }

    pub fn get_line_mut(&mut self, line_no: usize) -> Line<&mut T> {
        let n = self.len();
        let num_lines = self.num_lines();
        assert!(line_no < num_lines);
        if line_no < self.lines_left.len() {
            let line = &mut self.lines_left[line_no];
            Line {
                start: line.start,
                end: line.end,
                data: &mut line.data,
            }
        } else {
            let line = &mut self.lines_right[num_lines - 1 - line_no];
            Line {
                start: n - line.start,
                end: n - line.end,
                data: &mut line.data,
            }
        }
    }

    pub fn find_line(&self, pos: usize) -> usize {
        assert!(pos <= self.len());
        let mut left = 0;
        let mut right = self.num_lines();
        while right - left > 1 {
            let mid = left + (right - left) / 2;
            if pos < self.get_line(mid).start {
                right = mid;
            } else {
                left = mid;
            }
        }
        assert!(left + 1 == right);
        assert!(self.get_line(left).start <= pos && pos <= self.get_line(left).end);
        left
    }

    fn move_char_gap(&mut self, pos: usize) {
        while self.chars_left.len() > pos {
            self.chars_right.push(self.chars_left.pop().unwrap());
        }
        while self.chars_left.len() < pos {
            self.chars_left.push(self.chars_right.pop().unwrap());
        }
    }

    fn move_line_gap(&mut self, pos: usize) {
        let n = self.len();
        while self.lines_left.len() > pos {
            let mut line = self.lines_left.pop().unwrap();
            line.start = n - line.start;
            line.end = n - line.end;
            self.lines_right.push(line);
        }
        while self.lines_left.len() < pos {
            let mut line = self.lines_right.pop().unwrap();
            line.start = n - line.start;
            line.end = n - line.end;
            self.lines_left.push(line);
        }
    }

    pub fn replace_slice(&mut self, start: usize, end: usize, new_slice: &[char]) {
        assert!(start <= end && end <= self.len());

        let line_left = self.find_line(start);
        let line_right = self.find_line(end) + 1;

        let recompute_left = self.get_line(line_left).start;
        let recompute_right = self.get_line(line_right - 1).end
            - (end - start)
            + new_slice.len();

        self.move_line_gap(line_left);
        self.lines_right.truncate(self.lines_right.len() - (line_right - line_left));

        self.move_char_gap(start);
        self.chars_right.truncate(self.chars_right.len() - (end - start));
        for &c in new_slice {
            self.chars_left.push(c);
        }

        let mut t = recompute_left;
        for i in recompute_left .. recompute_right {
            if self.get_char(i) == '\n' {
                self.lines_left.push(Line {
                    start: t,
                    end: i,
                    data: T::default(),
                });
                t = i + 1;
            }
        }
        self.lines_left.push(Line {
            start: t,
            end: recompute_right,
            data: T::default(),
        });
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn line_ranges(b: &LineGapBuffer<()>) -> Vec<(usize, usize)> {
        let mut result = Vec::new();
        for i in 0 .. b.num_lines() {
            let Line { start, end, .. } = b.get_line(i);
            result.push((start, end));
        }
        result
    }

    #[test]
    fn stuff() {
        let mut b = LineGapBuffer::<()>::new();

        b.replace_slice(0, 0, &chars("hello"));
        assert_eq!(b.slice_string(0, b.len()), "hello");
        assert_eq!(line_ranges(&b), [(0, 5)]);

        b.replace_slice(2, 3, &chars("--"));
        assert_eq!(b.slice_string(0, b.len()), "he--lo");
        assert_eq!(line_ranges(&b), [(0, 6)]);

        b.replace_slice(2, 3, &chars("z\n\nz"));
        assert_eq!(b.slice_string(0, b.len()), "hez\n\nz-lo");
        assert_eq!(line_ranges(&b), [(0, 3), (4, 4), (5, 9)]);

        b.replace_slice(0, 4, &chars("q"));
        assert_eq!(b.slice_string(0, b.len()), "q\nz-lo");
        assert_eq!(line_ranges(&b), [(0, 1), (2, 6)]);

        b.replace_slice(0, 6, &chars(""));
        assert_eq!(b.slice_string(0, b.len()), "");
        assert_eq!(line_ranges(&b), [(0, 0)]);
    }
}
