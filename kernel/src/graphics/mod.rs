mod buffer;
mod color;
mod draw;
mod ui;

use bootloader_api::info::FrameBuffer;

pub use color::Color;

pub use draw::{
    clear_screen,
    draw_border,
    draw_char,
    draw_pixel,
    draw_pixel_alpha,
    draw_progress_bar,
    draw_rect,
    draw_status_dot,
    draw_string,
    text_width,
};

pub use buffer::{
    begin_frame,
    present,
};

pub fn init(
    framebuffer: &mut FrameBuffer,
) {
    crate::font::init();

    buffer::init(
        framebuffer,
    );

    //
    // Build the static Rusty background + panel.
    //
    ui::build_base();

    //
    // Show the completed initial frame.
    //
    buffer::present(
        framebuffer,
    );
}

pub fn width() -> usize {
    buffer::width()
}

pub fn height() -> usize {
    buffer::height()
}