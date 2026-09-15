mod boothandler;

use bootloader_api::info::FrameBuffer;

use crate::{
    graphics,
    input::{self, Key},
    terminal,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Program {
    Launcher,
    Terminal,
}

pub fn run(
    framebuffer: &mut FrameBuffer,
) -> ! {
    let mut current =
        Program::Launcher;

    let mut selected =
        0usize;

    loop {
        match current {
            // ====================================================
            // Launcher
            // ====================================================

            Program::Launcher => {
                draw_launcher(
                    framebuffer,
                    selected,
                );

                loop {
                    if let Some(key) =
                        input::read_key()
                    {
                        match key {
                            // ------------------------------------
                            // Up
                            // ------------------------------------

                            Key::Up => {
                                if selected > 0 {
                                    selected -= 1;

                                    draw_launcher(
                                        framebuffer,
                                        selected,
                                    );
                                }
                            }

                            // ------------------------------------
                            // Down
                            // ------------------------------------

                            Key::Down => {
                                if selected < 2 {
                                    selected += 1;

                                    draw_launcher(
                                        framebuffer,
                                        selected,
                                    );
                                }
                            }

                            // ------------------------------------
                            // Enter
                            // ------------------------------------

                            Key::Enter => {
                                match selected {
                                    0 => {
                                        current =
                                            Program::Terminal;

                                        break;
                                    }

                                    // Files
                                    // Settings
                                    //
                                    // Not implemented yet.
                                    _ => {}
                                }
                            }

                            _ => {}
                        }
                    }

                    core::hint::spin_loop();
                }
            }

            // ====================================================
            // Terminal
            // ====================================================

            Program::Terminal => {
                terminal::run(
                    framebuffer,
                );

                current =
                    Program::Launcher;

                draw_launcher(
                    framebuffer,
                    selected,
                );
            }
        }
    }
}

// ============================================================
// Launcher
// ============================================================

fn draw_launcher(
    framebuffer: &mut FrameBuffer,
    selected: usize,
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
    // Restore cached background + panel
    // ========================================================

    graphics::begin_frame();

    // ========================================================
    // Hardware status
    // ========================================================

    let status_x =
        panel_x + 16;

    let status_y =
        panel_y + 10;

    if input::usb::is_initialized() {
        graphics::draw_string(
            status_x,
            status_y,
            "USB: INITIALIZED",
            graphics::Color::GREEN,
            1,
        );
    } else {
        graphics::draw_string(
            status_x,
            status_y,
            "USB: FAILED",
            graphics::Color::TEXT_DIM,
            1,
        );
    }

    if input::usb::has_keyboard() {
        graphics::draw_string(
            status_x,
            status_y + 18,
            "KEYBOARD: DETECTED",
            graphics::Color::GREEN,
            1,
        );
    } else {
        graphics::draw_string(
            status_x,
            status_y + 18,
            "KEYBOARD: NOT FOUND",
            graphics::Color::TEXT_DIM,
            1,
        );
    }

    // ========================================================
    // Header
    // ========================================================

    graphics::draw_string(
        panel_x + 16,
        panel_y + 55,
        "RUSTY OS",
        graphics::Color::WHITE,
        2,
    );

    graphics::draw_string(
        panel_x + 16,
        panel_y + 100,
        "WELCOME",
        graphics::Color::TEXT_DIM,
        1,
    );

    graphics::draw_rect(
        panel_x + 16,
        panel_y + 120,
        panel_width.saturating_sub(32),
        1,
        graphics::Color::BORDER,
    );

    // ========================================================
    // Programs
    // ========================================================

    draw_program(
        panel_x + 16,
        panel_y + 140,
        "TERMINAL",
        0,
        selected,
    );

    draw_program(
        panel_x + 16,
        panel_y + 176,
        "FILES",
        1,
        selected,
    );

    draw_program(
        panel_x + 16,
        panel_y + 212,
        "SETTINGS",
        2,
        selected,
    );

    // ========================================================
    // Footer
    // ========================================================

    graphics::draw_string(
        panel_x + 16,
        panel_y
            + panel_height
            .saturating_sub(34),
        "UP DOWN NAVIGATE   ENTER OPEN",
        graphics::Color::TEXT_DIM,
        1,
    );

    // ========================================================
    // Present
    // ========================================================

    graphics::present(
        framebuffer,
    );
}

// ============================================================
// Program item
// ============================================================

fn draw_program(
    x: usize,
    y: usize,
    name: &str,
    index: usize,
    selected: usize,
) {
    let active =
        index == selected;

    if active {
        graphics::draw_rect(
            x.saturating_sub(6),
            y.saturating_sub(4),
            210,
            25,
            graphics::Color::PANEL_LIGHT,
        );

        graphics::draw_rect(
            x.saturating_sub(6),
            y.saturating_sub(4),
            3,
            25,
            graphics::Color::GREEN,
        );

        graphics::draw_string(
            x,
            y,
            ">",
            graphics::Color::GREEN,
            1,
        );
    }

    graphics::draw_string(
        x + 20,
        y,
        name,
        if active {
            graphics::Color::WHITE
        } else {
            graphics::Color::TEXT_DIM
        },
        1,
    );
}