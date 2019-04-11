use std::ptr::null_mut;

use winapi::shared::winerror::S_OK;
use winapi::um::dwrite::*;

use super::com_ptr::ComPtr;

pub struct TextLayout {
    pub raw: ComPtr<IDWriteTextLayout>,
    pub width: f32,
    pub height: f32,
    pub line_height: f32,
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

        TextLayout {
            raw,
            width: text_metrics.widthIncludingTrailingWhitespace,
            height: text_metrics.height,
            line_height: ht_metrics.height,
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
}
