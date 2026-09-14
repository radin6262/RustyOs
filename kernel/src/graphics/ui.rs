use super::{
    buffer,
    draw,
    Color,
};

pub fn build_base() {
    draw_background();
    draw_panel();
}

// ============================================================
// Background
// ============================================================

fn draw_background() {
    let width =
        buffer::width();

    let height =
        buffer::height();

    unsafe {
        let base =
            buffer::base_mut();

        let bpp =
            buffer::bytes_per_pixel();

        let stride =
            buffer::stride();

        let format =
            buffer::pixel_format();

        for y in 0..height {
            let t =
                if height > 1 {
                    ((y * 255)
                        / (height - 1))
                        as u8
                } else {
                    0
                };

            let r =
                7u8.saturating_add(
                    t / 14,
                );

            let g =
                9u8.saturating_add(
                    t / 12,
                );

            let b =
                18u8.saturating_add(
                    t / 4,
                );

            let row =
                y * stride * bpp;

            for x in 0..width {
                let offset =
                    row + x * bpp;

                if offset + bpp >
                    base.len()
                {
                    continue;
                }

                match format {
                    bootloader_api::info::PixelFormat::Rgb => {
                        base[offset] = r;
                        base[offset + 1] = g;
                        base[offset + 2] = b;
                    }

                    bootloader_api::info::PixelFormat::Bgr => {
                        base[offset] = b;
                        base[offset + 1] = g;
                        base[offset + 2] = r;
                    }

                    bootloader_api::info::PixelFormat::U8 => {
                        base[offset] = r;
                    }

                    _ => {}
                }
            }
        }
    }

    //
    // Scanlines.
    //
    //
    // These are only generated ONCE.
    //

    unsafe {
        draw_base_rect(
            0,
            0,
            0,
            0,
        );

        let base =
            buffer::base_mut();

        let bpp =
            buffer::bytes_per_pixel();

        let stride =
            buffer::stride();

        let format =
            buffer::pixel_format();

        let color =
            Color {
                r: 20,
                g: 25,
                b: 40,
                a: 255,
            };

        for y in
            (0..height).step_by(4)
        {
            for x in 0..width {
                let offset =
                    y * stride * bpp
                        + x * bpp;

                if offset + bpp >
                    base.len()
                {
                    continue;
                }

                match format {
                    bootloader_api::info::PixelFormat::Rgb => {
                        base[offset] = color.r;
                        base[offset + 1] = color.g;
                        base[offset + 2] = color.b;
                    }

                    bootloader_api::info::PixelFormat::Bgr => {
                        base[offset] = color.b;
                        base[offset + 1] = color.g;
                        base[offset + 2] = color.r;
                    }

                    bootloader_api::info::PixelFormat::U8 => {
                        base[offset] = color.r;
                    }

                    _ => {}
                }
            }
        }
    }
}

// ============================================================
// Panel
// ============================================================

fn draw_panel() {
    //
    // Temporarily render into the working frame.
    //
    //
    // Then copy the resulting frame into the base.
    //

    buffer::begin_frame();

    let width =
        buffer::width();

    let height =
        buffer::height();

    let panel_width =
        if width > 100 {
            (width * 3) / 4
        } else {
            width.saturating_sub(10)
        };

    let panel_height =
        if height > 100 {
            (height * 3) / 4
        } else {
            height.saturating_sub(10)
        };

    let panel_x =
        width
            .saturating_sub(
                panel_width,
            )
            / 2;

    let panel_y =
        height
            .saturating_sub(
                panel_height,
            )
            / 2;

    draw::draw_rect(
        panel_x + 4,
        panel_y + 4,
        panel_width,
        panel_height,
        Color::SHADOW,
    );

    draw::draw_rect(
        panel_x,
        panel_y,
        panel_width,
        panel_height,
        Color::PANEL,
    );

    draw::draw_rect(
        panel_x,
        panel_y,
        panel_width,
        2,
        Color::PANEL_LIGHT,
    );

    draw::draw_border(
        panel_x,
        panel_y,
        panel_width,
        panel_height,
        Color::BORDER,
    );

    let accent_width =
        panel_width.min(180);

    draw::draw_rect(
        panel_x,
        panel_y,
        accent_width,
        3,
        Color::CYAN,
    );

    draw::draw_rect(
        panel_x + accent_width,
        panel_y,
        panel_width
            .saturating_sub(
                accent_width,
            ),
        3,
        Color::PURPLE,
    );

    let led_y =
        panel_y + 14;

    draw::draw_rect(
        panel_x + 12,
        led_y,
        4,
        4,
        Color::GREEN,
    );

    draw::draw_rect(
        panel_x + 20,
        led_y,
        4,
        4,
        Color::CYAN,
    );

    draw::draw_rect(
        panel_x + 28,
        led_y,
        4,
        4,
        Color::PURPLE,
    );

    //
    // Copy complete panel frame into base.
    //
    unsafe {
        let frame =
            core::slice::from_raw_parts(
                buffer::frame_ptr(),
                buffer::frame_len(),
            );

        let base =
            buffer::base_mut();

        base.copy_from_slice(frame);
    }
}

// ============================================================
// Tiny helper
// ============================================================

unsafe fn draw_base_rect(
    _x: usize,
    _y: usize,
    _width: usize,
    _height: usize,
) {
    //
    // Intentionally empty.
    //
    // Keeps base rendering isolated from the public
    // framebuffer drawing API.
    //
}