#![no_std]

extern crate alloc;

use alloc::vec::Vec;

use crate::font;
use crate::graphics::Color;

// ============================================================
// Window limits
// ============================================================

/// Maximum width of a single window.
const MAX_WINDOW_WIDTH: usize = 4096;

/// Maximum height of a single window.
const MAX_WINDOW_HEIGHT: usize = 4096;

/// Maximum number of pixels a single window may contain.
///
/// 16 million pixels × 4 bytes = 64 MiB.
const MAX_WINDOW_PIXELS: usize = 16 * 1024 * 1024;

// ============================================================
// Window
// ============================================================

pub struct Window {
    pub id: u64,

    pub x: i32,
    pub y: i32,

    pub width: usize,
    pub height: usize,

    /// Window-owned ARGB/XRGB pixel buffer.
    pub pixels: Vec<u32>,

    pub z_index: usize,

    pub title: Vec<u8>,
}

impl Window {
    // ========================================================
    // Construction
    // ========================================================

    pub fn new(
        id: u64,
        x: i32,
        y: i32,
        width: usize,
        height: usize,
        z_index: usize,
    ) -> Self {
        assert!(width > 0, "Rusty: window width cannot be zero");
        assert!(height > 0, "Rusty: window height cannot be zero");
        assert!(width <= MAX_WINDOW_WIDTH, "Rusty: window width is too large");
        assert!(height <= MAX_WINDOW_HEIGHT, "Rusty: window height is too large");

        let pixel_count = width
            .checked_mul(height)
            .expect("Rusty: window pixel count overflow");

        assert!(pixel_count <= MAX_WINDOW_PIXELS, "Rusty: window is too large");

        let mut pixels = Vec::with_capacity(pixel_count);
        pixels.resize(pixel_count, 0xFF181825);

        Self {
            id,
            x,
            y,
            width,
            height,
            pixels,
            z_index,
            title: Vec::new(),
        }
    }

    // ========================================================
    // Clear
    // ========================================================

    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }

    // ========================================================
    // Pixel
    // ========================================================

    pub fn set_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }

        let index = y * self.width + x;
        self.pixels[index] = color;
    }

    // ========================================================
    // Rectangle
    // ========================================================

    pub fn fill_rect(
        &mut self,
        rx: usize,
        ry: usize,
        rw: usize,
        rh: usize,
        color: u32,
    ) {
        if rx >= self.width || ry >= self.height {
            return;
        }

        let end_x = rx.saturating_add(rw).min(self.width);
        let end_y = ry.saturating_add(rh).min(self.height);

        for y in ry..end_y {
            let row_start = y * self.width;
            for x in rx..end_x {
                self.pixels[row_start + x] = color;
            }
        }
    }

    // ========================================================
    // Text
    // ========================================================

    pub fn draw_string(
        &mut self,
        x: usize,
        y: usize,
        text: &str,
        color: Color,
        scale: usize,
    ) {
        if text.is_empty() || scale == 0 {
            return;
        }

        let pixel_size = (16 * scale) as f32;
        let ascent = font::ascent(pixel_size) as i32;
        let line_height = font::line_height(pixel_size).max(1) as i32;

        let mut cursor_x = x as i32;
        let mut cursor_y = y as i32;

        for character in text.chars() {
            if character == '\n' {
                cursor_x = x as i32;
                cursor_y = cursor_y.saturating_add(line_height);

                if cursor_y >= self.height as i32 {
                    break;
                }
                continue;
            }

            if character == '\r' {
                cursor_x = x as i32;
                continue;
            }

            let Some((metrics, bitmap)) = font::rasterize(character, pixel_size) else {
                let adv = font::advance(character, pixel_size).max(pixel_size / 2.0);
                cursor_x = cursor_x.saturating_add(adv as i32);
                continue;
            };

            let baseline = cursor_y.saturating_add(ascent);

            for glyph_y in 0..metrics.height {
                let dst_y = baseline
                    .saturating_add(metrics.offset_y)
                    .saturating_add(glyph_y as i32);

                if dst_y < 0 || dst_y >= self.height as i32 {
                    continue;
                }

                let dst_y_usize = dst_y as usize;

                for glyph_x in 0..metrics.width {
                    let alpha = bitmap[glyph_y * metrics.width + glyph_x];

                    if alpha == 0 {
                        continue;
                    }

                    let dst_x = cursor_x
                        .saturating_add(metrics.offset_x)
                        .saturating_add(glyph_x as i32);

                    if dst_x < 0 || dst_x >= self.width as i32 {
                        continue;
                    }

                    let dst_x_usize = dst_x as usize;
                    let index = dst_y_usize * self.width + dst_x_usize;

                    self.pixels[index] = blend_pixel(self.pixels[index], color, alpha);
                }
            }

            let advance = metrics.advance_width.max(0.0);
            cursor_x = cursor_x.saturating_add(advance as i32);

            if cursor_x >= self.width as i32 {
                break;
            }
        }
    }

    // ========================================================
    // Title
    // ========================================================

    pub fn set_title(&mut self, title: &str) {
        self.title.clear();
        self.title.extend_from_slice(title.as_bytes());
    }

    pub fn title(&self) -> &str {
        core::str::from_utf8(&self.title).unwrap_or("")
    }
}

// ============================================================
// Pixel blending
// ============================================================

fn blend_pixel(background: u32, color: Color, alpha: u8) -> u32 {
    if alpha == 0 {
        return background;
    }

    if alpha == 255 {
        return color_to_argb(color);
    }

    let src_alpha = alpha as u32;
    let inverse_alpha = 255u32.saturating_sub(src_alpha);

    let background_r = (background >> 16) & 0xFF;
    let background_g = (background >> 8) & 0xFF;
    let background_b = background & 0xFF;

    let source_r = color.r as u32;
    let source_g = color.g as u32;
    let source_b = color.b as u32;

    let r = (source_r * src_alpha + background_r * inverse_alpha) / 255;
    let g = (source_g * src_alpha + background_g * inverse_alpha) / 255;
    let b = (source_b * src_alpha + background_b * inverse_alpha) / 255;

    0xFF000000 | (r << 16) | (g << 8) | b
}

fn color_to_argb(color: Color) -> u32 {
    0xFF000000
        | ((color.r as u32) << 16)
        | ((color.g as u32) << 8)
        | (color.b as u32)
}