#![no_std]
#![no_main]

use core::panic::PanicInfo;

use rusty_sdk::{
    syscalls,
    ui::{
        Color,
        Dimensions,
        Position,
        Window,
    },
};

// ============================================================
// Launcher configuration
// ============================================================

const WIDTH: usize = 700;
const HEIGHT: usize = 450;

const WINDOW_X: i32 = 290;
const WINDOW_Y: i32 = 175;

// ============================================================
// Terminal application
// ============================================================
//
// Directory:
//
// user/
// ├── launcher/
// │   └── src/
// │       └── main.rs
// │
// └── terminal/
//     └── Terminal
//
// The ELF is embedded directly into this launcher executable.
// ============================================================

static TERMINAL_ELF:
&[u8] =
    include_bytes!(
        "../../terminal/Terminal"
    );

// ============================================================
// Terminal launcher hitbox
// ============================================================
//
// The Terminal tile is:
//
//     local x = 50
//     local y = 85
//     width  = 220
//     height = 140
//
// Launcher window:
//
//     x = 290
//     y = 175
//
// Therefore the Terminal tile occupies:
//
//     screen x = 340 .. 560
//     screen y = 260 .. 400
//
// Any real hardware left click inside this rectangle
// launches Terminal.
//
// sys_click_rect() does NOT move the cursor or simulate
// a mouse click. It only checks a real click that has
// already been detected by the kernel.
// ============================================================

const TERMINAL_X: usize =
    WINDOW_X as usize + 50;

const TERMINAL_Y: usize =
    WINDOW_Y as usize + 85;

const TERMINAL_WIDTH: usize =
    220;

const TERMINAL_HEIGHT: usize =
    140;

// ============================================================
// Entry point
// ============================================================

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    // --------------------------------------------------------
    // Create launcher window
    // --------------------------------------------------------

    let mut window =
        Window::create(
            Position::new(
                WINDOW_X,
                WINDOW_Y,
            ),
            Dimensions::new(
                WIDTH,
                HEIGHT,
            ),
        );

    if window.id() == 0 {
        syscalls::stdout(
            "Failed to create RustyOS app launcher window!\n",
        );

        syscalls::exit(
            1,
        );
    }

    // --------------------------------------------------------
    // Draw launcher
    // --------------------------------------------------------

    draw_launcher(
        &mut window,
    );

    // --------------------------------------------------------
    // Input loop
    // --------------------------------------------------------

    loop {
        // ----------------------------------------------------
        // Terminal application
        // ----------------------------------------------------
        //
        // Check whether a real hardware left click occurred
        // anywhere inside the Terminal tile.
        //
        // This does NOT move the cursor and does NOT generate
        // or simulate any mouse input.
        //

        if syscalls::sys_click_rect(
            (
                TERMINAL_X,
                TERMINAL_Y,
                TERMINAL_WIDTH,
                TERMINAL_HEIGHT,
            ),
        ) {
            // -----------------------------------------------
            // Destroy this launcher window first.
            // -----------------------------------------------

            let window_id =
                window.id();

            syscalls::destroy_window(
                window_id,
            );

            // -----------------------------------------------
            // Launch Terminal.
            //
            // This syscall does not return after a successful
            // process transition.
            // -----------------------------------------------

            syscalls::launch_app(
                TERMINAL_ELF,
            );
        }

        // ----------------------------------------------------
        // Give the scheduler CPU time.
        // ----------------------------------------------------

        syscalls::yield_now();
    }
}

// ============================================================
// Draw launcher
// ============================================================

fn draw_launcher(
    window: &mut Window,
) {
    // --------------------------------------------------------
    // Background
    // --------------------------------------------------------

    window.fill(
        Color::DARK_GRAY,
    );

    // --------------------------------------------------------
    // Header
    // --------------------------------------------------------

    window.draw_string(
        30,
        25,
        "RustyOS Applications",
        2,
    );

    window.draw_string(
        30,
        58,
        "Select an application",
        1,
    );

    // --------------------------------------------------------
    // Terminal tile
    //
    // Local coordinates:
    //
    //     x      = 50
    //     y      = 85
    //     width  = 220
    //     height = 140
    //
    // Screen coordinates:
    //
    //     x      = 340
    //     y      = 260
    //     width  = 220
    //     height = 140
    //
    // The same values are used by the Terminal hitbox above.
    // --------------------------------------------------------

    const TILE_X: usize =
        50;

    const TILE_Y: usize =
        85;

    const TILE_WIDTH: usize =
        220;

    const TILE_HEIGHT: usize =
        140;

    // --------------------------------------------------------
    // Tile background
    // --------------------------------------------------------

    window.fill_rect(
        TILE_X,
        TILE_Y,
        TILE_WIDTH,
        TILE_HEIGHT,
        Color::BLACK,
    );

    // --------------------------------------------------------
    // Tile border
    // --------------------------------------------------------

    window.draw_rect(
        TILE_X,
        TILE_Y,
        TILE_WIDTH,
        TILE_HEIGHT,
        Color::GREEN,
    );

    // --------------------------------------------------------
    // Terminal icon
    // --------------------------------------------------------

    const ICON_X: usize =
        TILE_X + 20;

    const ICON_Y: usize =
        TILE_Y + 25;

    const ICON_WIDTH: usize =
        70;

    const ICON_HEIGHT: usize =
        55;

    window.fill_rect(
        ICON_X,
        ICON_Y,
        ICON_WIDTH,
        ICON_HEIGHT,
        Color::DARK_GRAY,
    );

    window.draw_rect(
        ICON_X,
        ICON_Y,
        ICON_WIDTH,
        ICON_HEIGHT,
        Color::WHITE,
    );

    // --------------------------------------------------------
    // Terminal prompt
    // --------------------------------------------------------

    window.draw_string(
        ICON_X + 10,
        ICON_Y + 12,
        ">_",
        2,
    );

    // --------------------------------------------------------
    // Terminal title
    // --------------------------------------------------------

    window.draw_string(
        TILE_X + 20,
        TILE_Y + 90,
        "Terminal",
        2,
    );

    // --------------------------------------------------------
    // Description
    // --------------------------------------------------------

    window.draw_string(
        TILE_X + 20,
        TILE_Y + 116,
        "RustyOS terminal",
        1,
    );

    // --------------------------------------------------------
    // Present
    // --------------------------------------------------------

    window.present();
}

// ============================================================
// Panic handler
// ============================================================

#[panic_handler]
fn panic(
    _info: &PanicInfo,
) -> ! {
    loop {
        core::hint::spin_loop();
    }
}