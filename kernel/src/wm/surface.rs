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

// ============================================================
// Box blur
// ============================================================
//
// Runs over packed 0xAARRGGBB pixels. The alpha byte is dropped
// before averaging and restored afterwards, because summing full
// pixels would overflow into it.
// ============================================================

#[inline]
fn clamp_index(
    value: i64,
    limit: usize,
) -> usize {
    value.clamp(0, limit as i64 - 1) as usize
}

/// Per-channel sums. Packed RGB cannot be averaged as one integer:
/// the blue total overflows into green and produces colour noise.
#[inline]
fn channel_sum(pixel: u32) -> (u32, u32, u32) {
    (
        (pixel >> 16) & 0xFF,
        (pixel >> 8) & 0xFF,
        pixel & 0xFF,
    )
}

fn box_blur_horizontal(
    source: &[u32],
    destination: &mut [u32],
    width: usize,
    height: usize,
    radius: usize,
) {
    let window =
        (2 * radius + 1) as u32;

    for row in 0..height {
        let base =
            row * width;

        let mut red = 0u32;
        let mut green = 0u32;
        let mut blue = 0u32;

        for offset in -(radius as i64)..=(radius as i64) {
            let (r, g, b) =
                channel_sum(
                    source[base
                        + clamp_index(
                            offset,
                            width,
                        )],
                );

            red += r;
            green += g;
            blue += b;
        }

        for column in 0..width {
            destination[base + column] =
                argb(
                    red / window,
                    green / window,
                    blue / window,
                );

            let add =
                clamp_index(
                    column as i64
                        + radius as i64
                        + 1,
                    width,
                );

            let remove =
                clamp_index(
                    column as i64 - radius as i64,
                    width,
                );

            let (add_r, add_g, add_b) =
                channel_sum(
                    source[base + add],
                );

            let (sub_r, sub_g, sub_b) =
                channel_sum(
                    source[base + remove],
                );

            red = red + add_r - sub_r;
            green = green + add_g - sub_g;
            blue = blue + add_b - sub_b;
        }
    }
}

fn box_blur_vertical(
    source: &[u32],
    destination: &mut [u32],
    width: usize,
    height: usize,
    radius: usize,
) {
    let window =
        (2 * radius + 1) as u32;

    for column in 0..width {
        let mut red = 0u32;
        let mut green = 0u32;
        let mut blue = 0u32;

        for offset in -(radius as i64)..=(radius as i64) {
            let (r, g, b) =
                channel_sum(
                    source[clamp_index(
                        offset,
                        height,
                    ) * width
                        + column],
                );

            red += r;
            green += g;
            blue += b;
        }

        for row in 0..height {
            destination[row * width + column] =
                argb(
                    red / window,
                    green / window,
                    blue / window,
                );

            let add =
                clamp_index(
                    row as i64
                        + radius as i64
                        + 1,
                    height,
                );

            let remove =
                clamp_index(
                    row as i64 - radius as i64,
                    height,
                );

            let (add_r, add_g, add_b) =
                channel_sum(
                    source[add * width + column],
                );

            let (sub_r, sub_g, sub_b) =
                channel_sum(
                    source[remove * width + column],
                );

            red = red + add_r - sub_r;
            green = green + add_g - sub_g;
            blue = blue + add_b - sub_b;
        }
    }
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
// Anti-aliasing coverage
// ============================================================
//
// `inside_half` is twice the distance from a pixel center to the
// shape edge, in pixels. Positive values sit inside the shape.
// ============================================================

#[inline]
fn coverage_from_inside_half(
    inside_half: i32,
) -> u8 {
    (128 + inside_half * 127).clamp(0, 255) as u8
}

/// Coverage of a filled circle sampled at the pixel center.
#[inline]
fn circle_coverage(
    offset_x_half: i32,
    offset_y_half: i32,
    radius: usize,
) -> u8 {
    let squared =
        (offset_x_half as i64
            * offset_x_half as i64
            + offset_y_half as i64 * offset_y_half as i64)
            as u32;

    let distance_half =
        isqrt(squared) as i32;

    coverage_from_inside_half(
        2 * radius as i32 - distance_half,
    )
}

/// Coverage of a one pixel circle outline whose centerline sits at
/// `radius - 0.5` from the center.
#[inline]
fn arc_coverage(
    offset_x_half: i32,
    offset_y_half: i32,
    radius: usize,
) -> u8 {
    let squared =
        (offset_x_half as i64
            * offset_x_half as i64
            + offset_y_half as i64 * offset_y_half as i64)
            as u32;

    let distance_half =
        isqrt(squared) as i32;

    let difference =
        (distance_half
            - (2 * radius as i32 - 1))
            .abs();

    (255 - difference * 127).clamp(0, 255) as u8
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

    /// Read one pixel, returning opaque black when off-screen.
    pub fn get(
        &self,
        x: i64,
        y: i64,
    ) -> u32 {
        match self.index(x, y) {
            Some(index) => self.pixels[index],

            None => 0xFF00_0000,
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
            || alpha == 0
        {
            return;
        }

        let radius =
            radius.min(
                width.min(
                    height,
                ) / 2,
            );

        if radius == 0 {
            if filled {
                self.rect(
                    x,
                    y,
                    width,
                    height,
                    color,
                    alpha,
                );
            } else {
                self.border(
                    x,
                    y,
                    width,
                    height,
                    color,
                    alpha,
                );
            }

            return;
        }

        if filled {
            self.filled_rounded(
                x,
                y,
                width,
                height,
                radius,
                color,
                alpha,
                round_top,
                round_bottom,
            );
        } else {
            self.stroke_rounded(
                x,
                y,
                width,
                height,
                radius,
                color,
                alpha,
                round_top,
                round_bottom,
            );
        }
    }

    /// One pixel rectangle outline, used when no rounding is asked for.
    fn border(
        &mut self,
        x: i64,
        y: i64,
        width: usize,
        height: usize,
        color: u32,
        alpha: u8,
    ) {
        self.row(x, y, width, color, alpha);

        if height > 1 {
            self.row(
                x,
                y + height as i64 - 1,
                width,
                color,
                alpha,
            );
        }

        if height > 2 {
            for row in 1..height - 1 {
                self.put(
                    x,
                    y + row as i64,
                    color,
                    alpha,
                );

                self.put(
                    x + width as i64 - 1,
                    y + row as i64,
                    color,
                    alpha,
                );
            }
        }
    }

    /// Blend one quarter of a circle, clipped to the given pixel box.
    ///
    /// The caller restricts the box to a single rounded corner, so a
    /// pixel is never blended twice by the four corners of a shape.
    #[allow(clippy::too_many_arguments)]
    fn fill_corner(
        &mut self,
        center_x: i64,
        center_y: i64,
        radius: usize,
        x0: i64,
        y0: i64,
        x1: i64,
        y1: i64,
        color: u32,
        alpha: u8,
    ) {
        for py in y0..y1 {
            for px in x0..x1 {
                let coverage =
                    circle_coverage(
                        (2 * (px - center_x) + 1) as i32,
                        (2 * (py - center_y) + 1) as i32,
                        radius,
                    );

                if coverage == 0 {
                    continue;
                }

                self.put(
                    px,
                    py,
                    color,
                    (coverage as u32 * alpha as u32 / 255) as u8,
                );
            }
        }
    }

    /// Blend one quarter of a one pixel circle outline.
    #[allow(clippy::too_many_arguments)]
    fn stroke_corner(
        &mut self,
        center_x: i64,
        center_y: i64,
        radius: usize,
        x0: i64,
        y0: i64,
        x1: i64,
        y1: i64,
        color: u32,
        alpha: u8,
    ) {
        for py in y0..y1 {
            for px in x0..x1 {
                let coverage =
                    arc_coverage(
                        (2 * (px - center_x) + 1) as i32,
                        (2 * (py - center_y) + 1) as i32,
                        radius,
                    );

                if coverage == 0 {
                    continue;
                }

                self.put(
                    px,
                    py,
                    color,
                    (coverage as u32 * alpha as u32 / 255) as u8,
                );
            }
        }
    }

    /// Anti-aliased filled rounded rectangle.
    #[allow(clippy::too_many_arguments)]
    fn filled_rounded(
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
    ) {
        for row in 0..height {
            let top_band =
                round_top
                    && row < radius;

            let bottom_band =
                round_bottom
                    && row + radius >= height;

            let row_y =
                y + row as i64;

            if !top_band
                && !bottom_band
            {
                self.row(
                    x,
                    row_y,
                    width,
                    color,
                    alpha,
                );

                continue;
            }

            // Straight top / bottom edge between the two corners.
            if width > 2 * radius {
                self.row(
                    x + radius as i64,
                    row_y,
                    width - 2 * radius,
                    color,
                    alpha,
                );
            }

            let left_x1 =
                x + radius as i64;

            let right_x0 =
                x + width as i64
                    - radius as i64;

            let right_x1 =
                x + width as i64;

            if top_band {
                let center_y =
                    y + radius as i64;

                self.fill_corner(
                    x + radius as i64,
                    center_y,
                    radius,
                    x,
                    row_y,
                    left_x1,
                    row_y + 1,
                    color,
                    alpha,
                );

                self.fill_corner(
                    right_x0,
                    center_y,
                    radius,
                    right_x0,
                    row_y,
                    right_x1,
                    row_y + 1,
                    color,
                    alpha,
                );
            }

            if bottom_band {
                let center_y =
                    y + height as i64
                        - radius as i64;

                self.fill_corner(
                    x + radius as i64,
                    center_y,
                    radius,
                    x,
                    row_y,
                    left_x1,
                    row_y + 1,
                    color,
                    alpha,
                );

                self.fill_corner(
                    right_x0,
                    center_y,
                    radius,
                    right_x0,
                    row_y,
                    right_x1,
                    row_y + 1,
                    color,
                    alpha,
                );
            }
        }
    }

    /// Anti-aliased one pixel rounded rectangle outline.
    #[allow(clippy::too_many_arguments)]
    fn stroke_rounded(
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
    ) {
        let middle =
            width - 2 * radius;

        if middle > 0 {
            if round_top {
                self.row(
                    x + radius as i64,
                    y,
                    middle,
                    color,
                    alpha,
                );
            } else {
                self.row(x, y, width, color, alpha);
            }

            if height > 1 {
                if round_bottom {
                    self.row(
                        x + radius as i64,
                        y + height as i64 - 1,
                        middle,
                        color,
                        alpha,
                    );
                } else {
                    self.row(
                        x,
                        y + height as i64 - 1,
                        width,
                        color,
                        alpha,
                    );
                }
            }
        }

        let top_rows =
            if round_top {
                radius
            } else {
                0
            };

        let bottom_rows =
            if round_bottom {
                radius
            } else {
                0
            };

        let vertical_len =
            height.saturating_sub(
                top_rows + bottom_rows,
            );

        for row in 0..vertical_len {
            let row_y =
                y + top_rows as i64
                    + row as i64;

            self.put(x, row_y, color, alpha);

            if width > 1 {
                self.put(
                    x + width as i64 - 1,
                    row_y,
                    color,
                    alpha,
                );
            }
        }

        let left_x1 =
            x + radius as i64;

        let right_x0 =
            x + width as i64
                - radius as i64;

        let right_x1 =
            x + width as i64;

        if round_top {
            let center_y =
                y + radius as i64;

            self.stroke_corner(
                x + radius as i64,
                center_y,
                radius,
                x,
                y,
                left_x1,
                y + radius as i64,
                color,
                alpha,
            );

            self.stroke_corner(
                right_x0,
                center_y,
                radius,
                right_x0,
                y,
                right_x1,
                y + radius as i64,
                color,
                alpha,
            );
        }

        if round_bottom && height > 1 {
            let center_y =
                y + height as i64
                    - radius as i64;

            let bottom_y =
                y + height as i64;

            self.stroke_corner(
                x + radius as i64,
                center_y,
                radius,
                x,
                bottom_y - radius as i64,
                left_x1,
                bottom_y,
                color,
                alpha,
            );

            self.stroke_corner(
                right_x0,
                center_y,
                radius,
                right_x0,
                bottom_y - radius as i64,
                right_x1,
                bottom_y,
                color,
                alpha,
            );
        }
    }

    /// Blend a filled, anti-aliased circle.
    pub fn circle(
        &mut self,
        center_x: i64,
        center_y: i64,
        radius: i64,
        color: u32,
        alpha: u8,
    ) {
        if radius <= 0
            || alpha == 0
        {
            return;
        }

        let radius =
            radius as usize;

        for offset_y in -(radius as i64)..=(radius as i64) {
            for offset_x in -(radius as i64)..=(radius as i64) {
                let coverage =
                    circle_coverage(
                        (2 * offset_x + 1) as i32,
                        (2 * offset_y + 1) as i32,
                        radius,
                    );

                if coverage == 0 {
                    continue;
                }

                self.put(
                    center_x + offset_x,
                    center_y + offset_y,
                    color,
                    (coverage as u32 * alpha as u32 / 255) as u8,
                );
            }
        }
    }

    /// Blend an anti-aliased line segment.
    ///
    /// Handy for the small glyphs inside the window controls and for
    /// future vector chrome: every pixel within the segment's bounding
    /// box is shaded by its distance to the centerline, so diagonals
    /// do not stair-step.
    pub fn line(
        &mut self,
        start_x: i64,
        start_y: i64,
        end_x: i64,
        end_y: i64,
        color: u32,
        alpha: u8,
    ) {
        if alpha == 0 {
            return;
        }

        // Positions are kept in 1/16 pixel units so the distance test
        // stays in integers.
        const SCALE: i64 = 16;

        /// Half the line width, in 1/16 pixel units: 1 px.
        const RADIUS: i64 = 8;

        let ax = start_x * SCALE + SCALE / 2;
        let ay = start_y * SCALE + SCALE / 2;
        let bx = end_x * SCALE + SCALE / 2;
        let by = end_y * SCALE + SCALE / 2;

        let dx = bx - ax;
        let dy = by - ay;

        let length_squared =
            dx * dx + dy * dy;

        let left =
            start_x.min(end_x) - 2;

        let top =
            start_y.min(end_y) - 2;

        let right =
            start_x.max(end_x) + 2;

        let bottom =
            start_y.max(end_y) + 2;

        for y in top..=bottom {
            for x in left..=right {
                let px = x * SCALE + SCALE / 2;
                let py = y * SCALE + SCALE / 2;

                let vx = px - ax;
                let vy = py - ay;

                // Project the pixel onto the segment, clamped to the
                // endpoints so both caps are round.
                let projection =
                    if length_squared == 0 {
                        0
                    } else {
                        ((vx * dx + vy * dy) * 256
                            / length_squared)
                            .clamp(0, 256)
                    };

                let closest_x =
                    ax + dx * projection / 256;

                let closest_y =
                    ay + dy * projection / 256;

                let offset_x =
                    px - closest_x;

                let offset_y =
                    py - closest_y;

                let distance =
                    isqrt(
                        (offset_x * offset_x
                            + offset_y * offset_y)
                            as u32,
                    ) as i64;

                let coverage =
                    255 - (distance - RADIUS).max(0) * 16;

                if coverage <= 0 {
                    continue;
                }

                self.put(
                    x,
                    y,
                    color,
                    (coverage.min(255) as u32
                        * alpha as u32
                        / 255) as u8,
                );
            }
        }
    }

    /// Blur the backdrop inside a rounded rectangle and write it back.
    ///
    /// This is the "frosted glass" behind the menu bar and the dock:
    /// the wallpaper (and anything already composited under the panel)
    /// is box blurred in place, then the caller tints it.
    ///
    /// `scratch_a` and `scratch_b` are caller owned so this never
    /// allocates in the compositor's hot path. Both must be at least
    /// `width * height` elements long, otherwise the call does nothing.
    pub fn blur_rounded(
        &mut self,
        x: i64,
        y: i64,
        width: usize,
        height: usize,
        radius: usize,
        blur_radius: usize,
        scratch_a: &mut [u32],
        scratch_b: &mut [u32],
    ) {
        if width == 0
            || height == 0
        {
            return;
        }

        let x0 =
            x.max(0);

        let y0 =
            y.max(0);

        let x1 =
            x.saturating_add(
                width as i64,
            )
                .min(
                    self.width as i64,
                );

        let y1 =
            y.saturating_add(
                height as i64,
            )
                .min(
                    self.height as i64,
                );

        if x1 <= x0
            || y1 <= y0
        {
            return;
        }

        let w =
            (x1 - x0) as usize;

        let h =
            (y1 - y0) as usize;

        let count =
            w * h;

        if scratch_a.len() < count
            || scratch_b.len() < count
        {
            return;
        }

        // Copy the region into scratch space.
        for row in 0..h {
            let source =
                (y0 as usize + row)
                    * self.width
                    + x0 as usize;

            let target =
                row * w;

            scratch_a[target..target + w]
                .copy_from_slice(
                    &self.pixels[source..source + w],
                );
        }

        let blur_radius =
            blur_radius.min(24);

        if blur_radius > 0 {
            // Two box passes in each direction approximate a
            // Gaussian closely enough for frosted glass.
            for _ in 0..2 {
                box_blur_horizontal(
                    &scratch_a[..count],
                    &mut scratch_b[..count],
                    w,
                    h,
                    blur_radius,
                );

                box_blur_vertical(
                    &scratch_b[..count],
                    &mut scratch_a[..count],
                    w,
                    h,
                    blur_radius,
                );
            }
        }

        // Write the blurred pixels back only inside the rounded shape.
        for row in 0..h {
            let shape_row =
                (y0 - y + row as i64)
                    .max(0) as usize;

            let inset =
                if radius > 0 {
                    round_row_inset(
                        shape_row,
                        height,
                        radius,
                    )
                } else {
                    0
                };

            let left =
                (x + inset as i64).max(x0);

            let right =
                (x + width as i64 - inset as i64)
                    .min(x1);

            if right <= left {
                continue;
            }

            let start =
                (left - x0) as usize;

            let end =
                (right - x0) as usize;

            let destination =
                (y0 as usize + row)
                    * self.width
                    + x0 as usize;

            let source =
                row * w;

            self.pixels[destination + start
                ..destination + end]
                .copy_from_slice(
                    &scratch_a[source + start
                        ..source + end],
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
