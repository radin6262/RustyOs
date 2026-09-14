use bootloader_api::info::{
    FrameBuffer,
    PixelFormat,
};

use super::{
    buffer,
    Color,
};

// ============================================================
// Pixel writing
// ============================================================

#[inline(always)]
unsafe fn write_pixel(
    ptr: *mut u8,
    format: PixelFormat,
    r: u8,
    g: u8,
    b: u8,
) {
    unsafe {
        match format {
            PixelFormat::Rgb => {
                *ptr.add(0) = r;
                *ptr.add(1) = g;
                *ptr.add(2) = b;
            }

            PixelFormat::Bgr => {
                *ptr.add(0) = b;
                *ptr.add(1) = g;
                *ptr.add(2) = r;
            }

            PixelFormat::U8 => {
                *ptr = r;
            }

            _ => {}
        }
    }
}

// ============================================================
// Pixel
// ============================================================

pub fn draw_pixel(
    x: usize,
    y: usize,
    color: Color,
) {
    if x >= buffer::width()
        || y >= buffer::height()
    {
        return;
    }

    unsafe {
        let ptr =
            buffer::frame_ptr();

        let bpp =
            buffer::bytes_per_pixel();

        let offset =
            (y * buffer::stride()
                + x)
                * bpp;

        if offset + bpp
            > buffer::frame_len()
        {
            return;
        }

        write_pixel(
            ptr.add(offset),
            buffer::pixel_format(),
            color.r,
            color.g,
            color.b,
        );
    }
}

// ============================================================
// Rectangle
// ============================================================

pub fn draw_rect(
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    color: Color,
) {
    let screen_width =
        buffer::width();

    let screen_height =
        buffer::height();

    if x >= screen_width
        || y >= screen_height
        || width == 0
        || height == 0
    {
        return;
    }

    let x_end =
        x.saturating_add(width)
            .min(screen_width);

    let y_end =
        y.saturating_add(height)
            .min(screen_height);

    let bpp =
        buffer::bytes_per_pixel();

    let stride =
        buffer::stride();

    let format =
        buffer::pixel_format();

    unsafe {
        let base =
            buffer::frame_ptr();

        for py in y..y_end {
            let row =
                base.add(
                    py * stride * bpp,
                );

            for px in x..x_end {
                write_pixel(
                    row.add(px * bpp),
                    format,
                    color.r,
                    color.g,
                    color.b,
                );
            }
        }
    }
}

// ============================================================
// Alpha pixel
// ============================================================

pub fn draw_pixel_alpha(
    x: usize,
    y: usize,
    color: Color,
) {
    if color.a == 0 {
        return;
    }

    if color.a == 255 {
        draw_pixel(
            x,
            y,
            color,
        );

        return;
    }

    if x >= buffer::width()
        || y >= buffer::height()
    {
        return;
    }

    let bpp =
        buffer::bytes_per_pixel();

    let offset =
        (y * buffer::stride()
            + x)
            * bpp;

    unsafe {
        let base =
            buffer::frame_ptr();

        if offset + bpp
            > buffer::frame_len()
        {
            return;
        }

        let ptr =
            base.add(offset);

        let (
            dst_r,
            dst_g,
            dst_b,
        ) =
            match buffer::pixel_format()
            {
                PixelFormat::Rgb
                if bpp >= 3 =>
                    {
                        (
                            *ptr.add(0),
                            *ptr.add(1),
                            *ptr.add(2),
                        )
                    }

                PixelFormat::Bgr
                if bpp >= 3 =>
                    {
                        (
                            *ptr.add(2),
                            *ptr.add(1),
                            *ptr.add(0),
                        )
                    }

                PixelFormat::U8 => {
                    let value =
                        *ptr;

                    (
                        value,
                        value,
                        value,
                    )
                }

                _ => return,
            };

        let alpha =
            color.a as u16;

        let inverse =
            255u16 - alpha;

        let r =
            ((color.r as u16 * alpha
                + dst_r as u16
                * inverse)
                / 255) as u8;

        let g =
            ((color.g as u16 * alpha
                + dst_g as u16
                * inverse)
                / 255) as u8;

        let b =
            ((color.b as u16 * alpha
                + dst_b as u16
                * inverse)
                / 255) as u8;

        write_pixel(
            ptr,
            buffer::pixel_format(),
            r,
            g,
            b,
        );
    }
}

// ============================================================
// Clear
// ============================================================

pub fn clear_screen(
    framebuffer: &mut FrameBuffer,
    color: Color,
) {
    let _ = framebuffer;

    draw_rect(
        0,
        0,
        buffer::width(),
        buffer::height(),
        color,
    );
}

// ============================================================
// Border
// ============================================================

pub fn draw_border(
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    color: Color,
) {
    if width < 2
        || height < 2
    {
        return;
    }

    draw_rect(
        x,
        y,
        width,
        1,
        color,
    );

    draw_rect(
        x,
        y + height - 1,
        width,
        1,
        color,
    );

    draw_rect(
        x,
        y,
        1,
        height,
        color,
    );

    draw_rect(
        x + width - 1,
        y,
        1,
        height,
        color,
    );
}

// ============================================================
// TTF Character
// ============================================================

pub fn draw_char(
    x: usize,
    y: usize,
    c: char,
    color: Color,
    scale: usize,
) {
    if scale == 0 {
        return;
    }

    let pixel_size =
        (16 * scale) as f32;

    let Some((
                 metrics,
                 bitmap,
             )) =
        crate::font::rasterize(
            c,
            pixel_size,
        )
    else {
        return;
    };

    //
    // The public y position represents the
    // top of the line box.
    //
    let baseline =
        y as i32
            + crate::font::ascent(
            pixel_size,
        ) as i32;

    let origin_x =
        x as i32
            + metrics.offset_x;

    let origin_y =
        baseline
            + metrics.offset_y;

    for gy in 0..metrics.height {
        for gx in 0..metrics.width {
            let index =
                gy * metrics.width
                    + gx;

            let coverage =
                bitmap[index];

            if coverage == 0 {
                continue;
            }

            let px =
                origin_x
                    + gx as i32;

            let py =
                origin_y
                    + gy as i32;

            if px < 0
                || py < 0
            {
                continue;
            }

            let alpha =
                (
                    coverage as u16
                        * color.a as u16
                        / 255
                ) as u8;

            draw_pixel_alpha(
                px as usize,
                py as usize,
                Color {
                    r: color.r,
                    g: color.g,
                    b: color.b,
                    a: alpha,
                },
            );
        }
    }
}

// ============================================================
// TTF String
// ============================================================

pub fn draw_string(
    x: usize,
    y: usize,
    text: &str,
    color: Color,
    scale: usize,
) {
    if scale == 0 {
        return;
    }

    let pixel_size =
        (16 * scale) as f32;

    let line_height =
        crate::font::line_height(
            pixel_size,
        );

    let mut cursor_x =
        x as f32;

    let mut cursor_y =
        y;

    for c in text.chars() {
        match c {
            '\n' => {
                cursor_x =
                    x as f32;

                cursor_y +=
                    line_height;

                continue;
            }

            '\r' => {
                continue;
            }

            '\t' => {
                cursor_x +=
                    crate::font::advance(
                        ' ',
                        pixel_size,
                    ) * 4.0;

                continue;
            }

            _ => {}
        }

        draw_char(
            cursor_x.max(0.0)
                as usize,
            cursor_y,
            c,
            color,
            scale,
        );

        cursor_x +=
            crate::font::advance(
                c,
                pixel_size,
            );
    }
}

// ============================================================
// Progress bar
// ============================================================

pub fn draw_progress_bar(
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    progress: usize,
    color: Color,
) {
    let progress =
        progress.min(100);

    draw_rect(
        x,
        y,
        width,
        height,
        Color::BLACK,
    );

    draw_border(
        x,
        y,
        width,
        height,
        Color::BORDER,
    );

    if width <= 2
        || height <= 2
    {
        return;
    }

    let fill =
        ((width - 2) * progress)
            / 100;

    if fill > 0 {
        draw_rect(
            x + 1,
            y + 1,
            fill,
            height - 2,
            color,
        );
    }
}

// ============================================================
// Status dot
// ============================================================

pub fn draw_status_dot(
    x: usize,
    y: usize,
    color: Color,
) {
    draw_rect(
        x,
        y,
        5,
        5,
        color,
    );
}


pub fn text_width(
    text: &str,
    scale: usize,
) -> usize {
    if scale == 0 {
        return 0;
    }

    let font_size =
        (16 * scale) as f32;

    let mut width = 0.0f32;

    for c in text.chars() {
        match c {
            '\n' | '\r' => break,

            '\t' => {
                width +=
                    crate::font::advance(
                        ' ',
                        font_size,
                    ) * 4.0;
            }

            _ => {
                width +=
                    crate::font::advance(
                        c,
                        font_size,
                    );
            }
        }
    }

    width as usize
}