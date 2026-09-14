use bootloader_api::info::FrameBuffer;

use crate::{
    graphics,
    input::{self, Key},
};

const MAX_INPUT: usize = 40;
const MAX_MESSAGE: usize = 64;


pub fn run(
    framebuffer: &mut FrameBuffer,
) {
    let mut input_buffer =
        [0u8; MAX_INPUT];

    let mut length =
        0usize;

    let mut message =
        [0u8; MAX_MESSAGE];

    let mut message_length =
        0usize;

    redraw(
        framebuffer,
        &input_buffer,
        length,
        &message,
        message_length,
    );

    loop {
        if let Some(key) =
            input::read_key()
        {
            match key {
                // ====================================================
                // Back to launcher
                // ====================================================

                Key::Escape => {
                    return;
                }

                // ====================================================
                // Backspace
                // ====================================================

                Key::Backspace => {
                    if length > 0 {
                        length -= 1;
                        input_buffer[length] = 0;
                    }

                    redraw(
                        framebuffer,
                        &input_buffer,
                        length,
                        &message,
                        message_length,
                    );
                }

                // ====================================================
                // Enter
                // ====================================================

                Key::Enter => {
                    message_length =
                        execute_command(
                            &input_buffer,
                            length,
                            &mut message,
                        );

                    length = 0;

                    input_buffer.fill(0);

                    redraw(
                        framebuffer,
                        &input_buffer,
                        length,
                        &message,
                        message_length,
                    );
                }

                // ====================================================
                // Character
                // ====================================================

                Key::Character(c) => {
                    if length < MAX_INPUT {
                        input_buffer[length] =
                            c as u8;

                        length += 1;
                    }

                    redraw(
                        framebuffer,
                        &input_buffer,
                        length,
                        &message,
                        message_length,
                    );
                }

                _ => {}
            }
        }

        core::hint::spin_loop();
    }
}

// ============================================================
// REDRAW
// ============================================================

fn redraw(
    framebuffer: &mut FrameBuffer,
    input_buffer: &[u8; MAX_INPUT],
    length: usize,
    message: &[u8; MAX_MESSAGE],
    message_length: usize,
) {
    let width =
        graphics::width();

    let height =
        graphics::height();

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
            .saturating_sub(panel_width)
            / 2;

    let panel_y =
        height
            .saturating_sub(panel_height)
            / 2;

    // ========================================================
    // Restore static Rusty background + panel
    // ========================================================

    graphics::begin_frame();

    // ========================================================
    // Terminal header
    // ========================================================

    graphics::draw_string(
        panel_x + 16,
        panel_y + 28,
        "RUSTY TERMINAL",
        graphics::Color::WHITE,
        2,
    );

    graphics::draw_rect(
        panel_x + 16,
        panel_y + 73,
        panel_width.saturating_sub(32),
        1,
        graphics::Color::BORDER,
    );

    graphics::draw_string(
        panel_x + 16,
        panel_y + 97,
        "RUSTY SHELL",
        graphics::Color::GREEN,
        1,
    );

    graphics::draw_string(
        panel_x + 16,
        panel_y + 127,
        "TYPE HELP FOR COMMANDS",
        graphics::Color::TEXT_DIM,
        1,
    );

    // ========================================================
    // Prompt
    // ========================================================

    let prompt_x =
        panel_x + 16;

    let prompt_y =
        panel_y + 165;

    graphics::draw_string(
        prompt_x,
        prompt_y,
        "Terminal >",
        graphics::Color::CYAN,
        1,
    );

    // ========================================================
    // Input
    // ========================================================

    if let Ok(input) =
        core::str::from_utf8(
            &input_buffer[..length],
        )
    {
        graphics::draw_string(
            panel_x + 120,
            prompt_y,
            input,
            graphics::Color::WHITE,
            1,
        );
    }

    // ========================================================
    // Cursor
    // ========================================================

    let input =
        core::str::from_utf8(
            &input_buffer[..length],
        )
            .unwrap_or("");

    let cursor_x =
        panel_x
            + 120
            + graphics::text_width(
            input,
            1,
        );

    graphics::draw_rect(
        cursor_x,
        prompt_y.saturating_sub(1),
        12,
        17,
        graphics::Color::GREEN,
    );

    // ========================================================
    // Command result
    // ========================================================

    if message_length > 0 {
        if let Ok(message_str) =
            core::str::from_utf8(
                &message[..message_length],
            )
        {
            graphics::draw_string(
                panel_x + 16,
                panel_y + 205,
                message_str,
                graphics::Color::GREEN,
                1,
            );
        }
    }

    // ========================================================
    // Footer
    // ========================================================

    graphics::draw_string(
        panel_x + 16,
        panel_y
            + panel_height
            .saturating_sub(34),
        "ESC BACK",
        graphics::Color::TEXT_DIM,
        1,
    );

    // ========================================================
    // Present completed frame
    // ========================================================

    graphics::present(
        framebuffer,
    );
}

// ============================================================
// COMMAND EXECUTION
// ============================================================

fn execute_command(
    input_buffer: &[u8; MAX_INPUT],
    length: usize,
    message: &mut [u8; MAX_MESSAGE],
) -> usize {
    let command: &[u8];

    if equals(
        input_buffer,
        length,
        b"help",
    ) {
        command =
            b"COMMANDS: HELP CLEAR HELLO VERSION";
    } else if equals(
        input_buffer,
        length,
        b"clear",
    ) {
        return 0;
    } else if equals(
        input_buffer,
        length,
        b"hello",
    ) {
        command =
            b"HELLO FROM RUSTY";
    } else if equals(
        input_buffer,
        length,
        b"version",
    ) {
        command =
            b"RUSTY 0.1.0 KERNEL";
    } else if length == 0 {
        return 0;
    } else {
        command =
            b"UNKNOWN COMMAND";
    }

    let copy_length =
        command
            .len()
            .min(MAX_MESSAGE);

    message[..copy_length]
        .copy_from_slice(
            &command[..copy_length],
        );

    message[copy_length..].fill(0);

    copy_length
}

// ============================================================
// STRING COMPARISON
// ============================================================

fn equals(
    input_buffer: &[u8; MAX_INPUT],
    length: usize,
    command: &[u8],
) -> bool {
    if length != command.len() {
        return false;
    }

    input_buffer[..length]
        == command[..]
}