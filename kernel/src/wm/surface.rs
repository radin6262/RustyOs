#![no_std]

// ============================================================
// Surface primitives
// ============================================================
//
// Small, allocation-free drawing helpers shared by the desktop
// chrome and the window pixel buffers.
//
// A `Surface` is a plain ARGB pixel slice plus its dimensions.
// Every operation is bounds checked, so callers may draw
// partially or completely off-screen.
//
// ============================================================

use crate::font;
use crate::graphics::Color;

// ============================================================
// Color helpers
// ============================================================

pub const fn argb(
    red: u32,
    green: u32,
    blue: u32,
) -> u32 {
    0xFF00_0000
        | (red << 16)
        | (green << 8)
        | blue
}

/// Blend `color` over `background`.
///
/// `alpha` is the source alpha:
///
///     0   -> keep the background
///     255 -> fully replace it
pub fn blend(
    background: u32,
    color: u32,
    alpha: u8,
) -> u32 {
    if alpha == 0 {
        return background;
    }

    let source =
        alpha as u32;

    if source >= 255 {
        return 0xFF00_0000
            | (color & 0x00FF_FFFF);
    }

    let inverse =
        255 - source;

    let background_red =
        (background >> 16) & 0xFF;

    let background_green =
        (background >> 8) & 0xFF;

    let background_blue =
        background & 0xFF;

    let source_red =
        (color >> 16) & 0xFF;

    let source_green =
        (color >> 8) & 0xFF;

    let source_blue =
        color & 0xFF;

    let red =
        (source_red * source
            + background_red * inverse)
            / 255;

    let green =
        (source_green * source
            + background_green * inverse)
            / 255;

    let blue =
        (source_blue * source
            + background_blue * inverse)
            / 255;

    argb(red, green, blue)
}

/// Blend using the color's own alpha channel.
pub fn blend_alpha(
    background: u32,
    color: u32,
) -> u32 {
    blend(
        background,
        color,
        ((color >> 24) & 0xFF) as u8,
    )
}

// ============================================================
// Integer square root
// ============================================================

pub fn isqrt(
    value: u32,
) -> u32 {
    let mut remainder =
        value;

    let mut root =
        0u32;

    let mut bit =
        1u32 << 30;

    while bit > remainder {
        bit >>= 2;
    }

    while bit != 0 {
        if remainder >= root + bit {
            remainder -=
                root + bit;

            root =
                (root >> 1) + bit;
        } else {
            root >>=
                1;
        }

        bit >>=
            2;
    }

    root
}

// ============================================================
// Rounded rectangle insets
// ============================================================
//
// Returns how far one row of a rounded rectangle is inset from
// both sides, which is what turns a plain rectangle into a
// rounded one.
//
// `row` counts from the top of the rectangle.
// ============================================================

pub fn round_row_inset(
    row: usize,
    total: usize,
    radius: usize,
) -> usize {
    if radius == 0
        || total == 0
    {
        return 0;
    }

    let row =
        row.min(
            total - 1,
        );

    let distance =
        if row < radius {
            radius - row
        } else if row + radius >= total {
            row + radius - total + 1
        } else {
            0
        };

    if distance == 0 {
        return 0;
    }

    let radius_u32 =
        radius as u32;

    let distance_u32 =
        distance as u32;

    if distance_u32 >= radius_u32 {
        return radius;
    }

    radius
        - isqrt(
            radius_u32 * radius_u32
                - distance_u32 * distance_u32,
        ) as usize
}

// ============================================================
// Surface
// ============================================================

pub struct Surface<'a> {
    pixels:
        &'a mut [u32],

    width:
        usize,

    height:
        usize,
}

impl<'a> Surface<'a> {
    pub fn new(
        pixels: &'a mut [u32],
        width: usize,
        height: usize,
    ) -> Self {
        Self {
            pixels,
            width,
            height,
        }
    }

    pub fn width(
        &self,
    ) -> usize {
        self.width
    }

    pub fn height(
        &self,
    ) -> usize {
        self.height
    }

    fn index(
        &self,
        x: i64,
        y: i64,
    ) -> Option<usize> {
        if x < 0
            || y < 0
            || x >= self.width as i64
            || y >= self.height as i64
        {
            return None;
        }

        Some(
            y as usize
                * self.width
                + x as usize,
        )
    }

    /// Blend one pixel, leaving the destination untouched when the
    /// point is off-screen.
    pub fn put(
        &mut self,
        x: i64,
        y: i64,
        color: u32,
        alpha: u8,
    ) {
        if let Some(index) =
            self.index(
                x,
                y,
            )
        {
            self.pixels[index] =
                blend(
                    self.pixels[index],
                    color,
                    alpha,
                );
        }
    }

    /// Write one pixel opaquely.
    pub fn set(
        &mut self,
        x: i64,
        y: i64,
        color: u32,
    ) {
        if let Some(index) =
            self.index(
                x,
                y,
            )
        {
            self.pixels[index] =
                color;
        }
    }

    /// Blend a horizontal run of pixels.
    pub fn row(
        &mut self,
        x: i64,
        y: i64,
        length: usize,
        color: u32,
        alpha: u8,
    ) {
        if length == 0
            || y < 0
            || y >= self.height as i64
        {
            return;
        }

        let start =
            x.max(0);

        let end =
            x.saturating_add(
                length as i64,
            )
                .min(
                    self.width as i64,
                );

        if end <= start {
            return;
        }

        let row_start =
            y as usize
                * self.width;

        for px in start..end {
            let index =
                row_start
                    + px as usize;

            self.pixels[index] =
                blend(
                    self.pixels[index],
                    color,
                    alpha,
                );
        }
    }

    /// Blend a filled rectangle.
    pub fn rect(
        &mut self,
        x: i64,
        y: i64,
        width: usize,
        height: usize,
        color: u32,
        alpha: u8,
    ) {
        for row in 0..height {
            self.row(
                x,
                y + row as i64,
                width,
                color,
                alpha,
            );
        }
    }

    /// Blend a filled rounded rectangle.
    pub fn round_rect(
        &mut self,
        x: i64,
        y: i64,
        width: usize,
        height: usize,
        radius: usize,
        color: u32,
        alpha: u8,
    ) {
        self.rounded(
            x,
            y,
            width,
            height,
            radius,
            color,
            alpha,
            true,
            true,
            true,
        );
    }

    /// Blend a rounded rectangle whose bottom corners stay square,
    /// which is how a title bar sits on top of a window.
    pub fn top_round_rect(
        &mut self,
        x: i64,
        y: i64,
        width: usize,
        height: usize,
        radius: usize,
        color: u32,
        alpha: u8,
    ) {
        self.rounded(
            x,
            y,
            width,
            height,
            radius,
            color,
            alpha,
            true,
            false,
            true,
        );
    }

    /// Blend a one pixel rounded rectangle outline.
    pub fn round_stroke(
        &mut self,
        x: i64,
        y: i64,
        width: usize,
        height: usize,
        radius: usize,
        color: u32,
        alpha: u8,
    ) {
        self.rounded(
            x,
            y,
            width,
            height,
            radius,
            color,
            alpha,
            true,
            true,
            false,
        );
    }

    /// Shared implementation for every rounded rectangle shape.
    ///
    /// `round_top` / `round_bottom` select which corners are
    /// rounded, and `filled` switches between a solid shape and a
    /// one pixel outline.
    #[allow(clippy::too_many_arguments)]
    fn rounded(
        &mut self,
        x: i64,
        y: i64,
        width: usize,
        height: usize,
        radius: usize,
        color: u32,
        alpha: u8,
        round_top: bool,
        round_bottom: bool,
        filled: bool,
    ) {
        if width == 0
            || height == 0
        {
            return;
        }

        let radius =
            radius.min(
                width.min(
                    height,
                ) / 2,
            );

        for row in 0..height {
            let top_inset =
                if round_top
                    && row < radius
                {
                    round_row_inset(
                        row,
                        height,
                        radius,
                    )
                } else {
                    0
                };

            let bottom_inset =
                if round_bottom
                    && row + radius >= height
                {
                    round_row_inset(
                        row,
                        height,
                        radius,
                    )
                } else {
                    0
                };

            let inset =
                top_inset.max(
                    bottom_inset,
                );

            let start =
                x + inset as i64;

            let end =
                x + width as i64
                    - inset as i64;

            if end <= start {
                continue;
            }

            let y =
                y + row as i64;

            if filled {
                self.row(
                    start,
                    y,
                    (end - start) as usize,
                    color,
                    alpha,
                );

                continue;
            }

            // Outline: complete top and bottom rows, the left and
            // right edge pixels in between.
            if row == 0
                || row + 1 == height
            {
                self.row(
                    start,
                    y,
                    (end - start) as usize,
                    color,
                    alpha,
                );
            } else {
                self.put(
                    start,
                    y,
                    color,
                    alpha,
                );

                self.put(
                    end - 1,
                    y,
                    color,
                    alpha,
                );
            }
        }
    }

    /// Blend a filled circle.
    pub fn circle(
        &mut self,
        center_x: i64,
        center_y: i64,
        radius: i64,
        color: u32,
        alpha: u8,
    ) {
        if radius <= 0 {
            return;
        }

        let squared =
            (radius * radius) as u32;

        for offset_y in -radius..=radius {
            let offset_y_squared =
                (offset_y * offset_y) as u32;

            if offset_y_squared > squared {
                continue;
            }

            let offset_x =
                isqrt(
                    squared - offset_y_squared,
                ) as i64;

            self.row(
                center_x - offset_x,
                center_y + offset_y,
                (offset_x * 2 + 1) as usize,
                color,
                alpha,
            );
        }
    }

    /// Blend text using the kernel font.
    ///
    /// Byte channel order is 0xAARRGGBB, matching the rest of the
    /// compositor. Lines break on `\n`, and every glyph is clipped
    /// to the surface.
    pub fn text(
        &mut self,
        x: usize,
        y: usize,
        text: &str,
        color: Color,
        scale: usize,
    ) {
        if text.is_empty()
            || scale == 0
        {
            return;
        }

        let pixel_size =
            (16 * scale) as f32;

        let ascent =
            font::ascent(pixel_size) as i32;

        let line_height =
            font::line_height(pixel_size).max(1) as i32;

        let mut cursor_x =
            x as i32;

        let mut cursor_y =
            y as i32;

        let argb_color =
            argb(
                color.r as u32,
                color.g as u32,
                color.b as u32,
            );

        for character in text.chars() {
            if character == '\n' {
                cursor_x =
                    x as i32;

                cursor_y =
                    cursor_y.saturating_add(
                        line_height,
                    );

                if cursor_y >= self.height as i32 {
                    break;
                }

                continue;
            }

            if character == '\r' {
                cursor_x =
                    x as i32;

                continue;
            }

            let Some((metrics, bitmap)) =
                font::rasterize(
                    character,
                    pixel_size,
                )
            else {
                let advance =
                    font::advance(
                        character,
                        pixel_size,
                    )
                        .max(
                            pixel_size / 2.0,
                        );

                cursor_x =
                    cursor_x.saturating_add(
                        advance as i32,
                    );

                continue;
            };

            let baseline =
                cursor_y.saturating_add(
                    ascent,
                );

            for glyph_y in 0..metrics.height {
                let destination_y =
                    baseline
                        .saturating_add(
                            metrics.offset_y,
                        )
                        .saturating_add(
                            glyph_y as i32,
                        );

                if destination_y < 0
                    || destination_y >= self.height as i32
                {
                    continue;
                }

                for glyph_x in 0..metrics.width {
                    let coverage =
                        bitmap[glyph_y * metrics.width + glyph_x];

                    if coverage == 0 {
                        continue;
                    }

                    let destination_x =
                        cursor_x
                            .saturating_add(
                                metrics.offset_x,
                            )
                            .saturating_add(
                                glyph_x as i32,
                            );

                    if destination_x < 0
                        || destination_x >= self.width as i32
                    {
                        continue;
                    }

                    let index =
                        destination_y as usize
                            * self.width
                            + destination_x as usize;

                    self.pixels[index] =
                        blend(
                            self.pixels[index],
                            argb_color,
                            coverage,
                        );
                }
            }

            cursor_x =
                cursor_x.saturating_add(
                    metrics.advance_width.max(0.0) as i32,
                );

            if cursor_x >= self.width as i32 {
                break;
            }
        }
    }
}
