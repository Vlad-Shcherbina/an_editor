use std::ptr::null_mut;

use winapi::shared::winerror::{S_OK, HRESULT_FROM_WIN32, ERROR_INSUFFICIENT_BUFFER};
use winapi::um::dwrite::*;

use super::com_ptr::ComPtr;

pub struct TextLayout {
    pub raw: ComPtr<IDWriteTextLayout>,
    pub width: f32,
    pub height: f32,
    pub line_height: f32,
    line_metrics: Vec<DWRITE_LINE_METRICS>,
}

impl TextLayout {
    pub fn new(
        text: &str,
        dwrite_factory: &ComPtr<IDWriteFactory>,
        text_format: &ComPtr<IDWriteTextFormat>,
        max_width: f32,
    ) -> TextLayout {
        let text = super::win32_string(text);
        let raw = unsafe {
            let mut text_layout = null_mut();
            let hr = dwrite_factory.CreateTextLayout(
                text.as_ptr(),
                (text.len() - 1) as u32,
                text_format.as_raw(),
                max_width,
                1.0,  // height
                &mut text_layout,
            );
            assert!(hr == S_OK, "0x{:x}", hr);
            ComPtr::from_raw(text_layout)
        };

        let mut text_metrics = unsafe { std::mem::zeroed() };
        let hr = unsafe { raw.GetMetrics(&mut text_metrics) };
        assert!(hr == S_OK, "0x{:x}", hr);

        let mut x = 0.0;
        let mut y = 0.0;
        let mut ht_metrics = unsafe { std::mem::zeroed() };
        unsafe {
            let hr = raw.HitTestTextPosition(
                0,  // cursor pos
                0,  // isTrailingHit
                &mut x, &mut y,
                &mut ht_metrics,
            );
            assert!(hr == S_OK, "0x{:x}", hr);
        }

        let mut line_metrics = vec![unsafe { std::mem::zeroed() }];
        let mut actual_line_count = 0;
        let mut hr = unsafe {
            raw.GetLineMetrics(
                line_metrics.as_mut_ptr(),
                line_metrics.len() as u32,
                &mut actual_line_count,
            )
        };
        if hr == HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER) {
            line_metrics.resize(actual_line_count as usize, unsafe { std::mem::zeroed() });
            hr = unsafe {
                raw.GetLineMetrics(
                    line_metrics.as_mut_ptr(),
                    line_metrics.len() as u32,
                    &mut actual_line_count,
                )
            };
        }
        assert!(hr == S_OK, "0x{:x}", hr);

        TextLayout {
            raw,
            width: text_metrics.widthIncludingTrailingWhitespace,
            height: text_metrics.height,
            line_height: ht_metrics.height,
            line_metrics,
        }
    }

    pub fn cursor_coords(&self, pos: usize) -> (f32, f32) {
        let mut x = 0.0;
        let mut y = 0.0;
        let mut metrics = unsafe { std::mem::zeroed() };
        unsafe {
            let hr = self.raw.HitTestTextPosition(
                pos as u32,
                0,  // isTrailingHit
                &mut x, &mut y,
                &mut metrics,
            );
            assert!(hr == S_OK, "0x{:x}", hr);
        }
        (x, y)
    }

    pub fn cursor_coords_trailing(&self, pos: usize) -> (f32, f32) {
        assert!(pos > 0);
        let mut x = 0.0;
        let mut y = 0.0;
        let mut metrics = unsafe { std::mem::zeroed() };
        unsafe {
            let hr = self.raw.HitTestTextPosition(
                (pos - 1) as u32,
                1,  // isTrailingHit
                &mut x, &mut y,
                &mut metrics,
            );
            assert!(hr == S_OK, "0x{:x}", hr);
        }
        (x, y)
    }

    pub fn coords_to_pos(&self, x: f32, y: f32) -> usize {
        let mut is_trailing_hit = 0;
        let mut is_inside = 0;
        let mut metrics = unsafe { std::mem::zeroed() };
        unsafe {
            let hr = self.raw.HitTestPoint(
                x, y, &mut is_trailing_hit, &mut is_inside, &mut metrics);
            assert!(hr == S_OK, "0x{:x}", hr);
        }
        metrics.textPosition as usize + is_trailing_hit as usize
    }

    pub fn line_boundaries(&self) -> Vec<usize> {
        let mut result = Vec::new();
        result.push(0);
        for lm in &self.line_metrics {
            result.push(result.last().unwrap() + lm.length as usize);
        }
        result
    }
}
