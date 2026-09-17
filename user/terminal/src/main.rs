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

const WIDTH: usize = 700;
const HEIGHT: usize = 450;

const INPUT_MAX: usize = 128;

const TEXT_X: usize = 20;
const TEXT_Y: usize = 20;

const LINE_HEIGHT: usize = 16;
const CHAR_WIDTH: usize = 8;

// Space between the prompt and typed text.
const PROMPT_GAP: usize = 4;

// Extra vertical space between command output and the prompt.
const PROMPT_GAP_Y: usize = 8;

const MAX_LINES: usize =
    (HEIGHT - TEXT_Y * 2) / LINE_HEIGHT - 1;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut window =
        Window::create(
            Position::new(
                290,
                175,
            ),
            Dimensions::new(
                WIDTH,
                HEIGHT,
            ),
        );

    if window.id() == 0 {
        syscalls::stdout(
            "Failed to create RustyOS terminal window!\n",
        );

        syscalls::exit(1);
    }

    let mut lines =
        [[0u8; INPUT_MAX]; MAX_LINES];

    let mut line_lengths =
        [0usize; MAX_LINES];

    let mut line_count =
        0usize;

    let mut input =
        [0u8; INPUT_MAX];

    let mut input_length =
        0usize;

    // --------------------------------------------------------
    // Initial terminal contents
    // --------------------------------------------------------

    add_line(
        &mut lines,
        &mut line_lengths,
        &mut line_count,
        b"RustyOS Terminal",
    );

    add_line(
        &mut lines,
        &mut line_lengths,
        &mut line_count,
        b"Type 'help' for available commands.",
    );

    add_line(
        &mut lines,
        &mut line_lengths,
        &mut line_count,
        b"",
    );

    redraw(
        &mut window,
        &lines,
        &line_lengths,
        line_count,
        &input,
        input_length,
    );

    // --------------------------------------------------------
    // Input loop
    // --------------------------------------------------------

    let mut buffer =
        [0u8; 1];

    loop {
        let count =
            syscalls::read(
                0,
                &mut buffer,
            );

        if count == 0 {
            syscalls::yield_now();
            continue;
        }

        let key =
            buffer[0];

        match key {
            // ------------------------------------------------
            // Backspace
            // ------------------------------------------------

            0x08 => {
                if input_length > 0 {
                    input_length -= 1;

                    input[input_length] =
                        0;

                    redraw(
                        &mut window,
                        &lines,
                        &line_lengths,
                        line_count,
                        &input,
                        input_length,
                    );
                }
            }

            // ------------------------------------------------
            // Enter
            // ------------------------------------------------

            b'\n' => {
                add_line(
                    &mut lines,
                    &mut line_lengths,
                    &mut line_count,
                    &input[..input_length],
                );

                execute_command(
                    &input[..input_length],
                    &mut lines,
                    &mut line_lengths,
                    &mut line_count,
                );

                input_length =
                    0;

                redraw(
                    &mut window,
                    &lines,
                    &line_lengths,
                    line_count,
                    &input,
                    input_length,
                );
            }

            // ------------------------------------------------
            // Printable ASCII
            // ------------------------------------------------

            0x20..=0x7E => {
                if input_length
                    < INPUT_MAX
                {
                    input[input_length] =
                        key;

                    input_length +=
                        1;

                    redraw(
                        &mut window,
                        &lines,
                        &line_lengths,
                        line_count,
                        &input,
                        input_length,
                    );
                }
            }

            _ => {}
        }

        syscalls::yield_now();
    }
}

// ============================================================
// Command execution
// ============================================================

fn execute_command(
    command: &[u8],
    lines: &mut [[u8; INPUT_MAX]; MAX_LINES],
    line_lengths: &mut [usize; MAX_LINES],
    line_count: &mut usize,
) {
    // Empty command.
    if command.is_empty() {
        return;
    }

    // --------------------------------------------------------
    // help
    // --------------------------------------------------------

    if command == b"help" {
        add_line(
            lines,
            line_lengths,
            line_count,
            b"Available commands:",
        );

        add_line(
            lines,
            line_lengths,
            line_count,
            b"  help",
        );

        add_line(
            lines,
            line_lengths,
            line_count,
            b"  clear",
        );

        add_line(
            lines,
            line_lengths,
            line_count,
            b"  pid",
        );

        add_line(
            lines,
            line_lengths,
            line_count,
            b"  echo <text>",
        );

        return;
    }

    // --------------------------------------------------------
    // clear
    // --------------------------------------------------------

    if command == b"clear" {
        *line_count =
            0;

        return;
    }

    // --------------------------------------------------------
    // pid
    // --------------------------------------------------------

    if command == b"pid" {
        let pid =
            syscalls::getpid();

        let mut pid_line =
            [0u8; INPUT_MAX];

        let prefix =
            b"PID: ";

        for i in 0..prefix.len() {
            pid_line[i] =
                prefix[i];
        }

        let digits =
            write_number(
                pid,
                &mut pid_line[prefix.len()..],
            );

        add_line(
            lines,
            line_lengths,
            line_count,
            &pid_line[..prefix.len() + digits],
        );

        return;
    }

    // --------------------------------------------------------
    // echo
    // --------------------------------------------------------

    if command.len() >= 5
        && &command[..5] == b"echo "
    {
        add_line(
            lines,
            line_lengths,
            line_count,
            &command[5..],
        );

        return;
    }

    // --------------------------------------------------------
    // Unknown command
    // --------------------------------------------------------

    add_line(
        lines,
        line_lengths,
        line_count,
        b"Unknown command.",
    );

    add_line(
        lines,
        line_lengths,
        line_count,
        b"Type 'help' for help.",
    );
}

// ============================================================
// Add terminal line
// ============================================================

fn add_line(
    lines: &mut [[u8; INPUT_MAX]; MAX_LINES],
    line_lengths: &mut [usize; MAX_LINES],
    line_count: &mut usize,
    text: &[u8],
) {
    let length =
        core::cmp::min(
            text.len(),
            INPUT_MAX,
        );

    if *line_count >= MAX_LINES {
        for i in 1..MAX_LINES {
            lines[i - 1] =
                lines[i];

            line_lengths[i - 1] =
                line_lengths[i];
        }

        *line_count =
            MAX_LINES - 1;
    }

    for i in 0..INPUT_MAX {
        lines[*line_count][i] =
            0;
    }

    for i in 0..length {
        lines[*line_count][i] =
            text[i];
    }

    line_lengths[*line_count] =
        length;

    *line_count +=
        1;
}

// ============================================================
// Redraw terminal
// ============================================================

fn redraw(
    window: &mut Window,
    lines: &[[u8; INPUT_MAX]; MAX_LINES],
    line_lengths: &[usize; MAX_LINES],
    line_count: usize,
    input: &[u8; INPUT_MAX],
    input_length: usize,
) {
    window.fill(
        Color::BLACK,
    );

    // --------------------------------------------------------
    // Draw terminal history
    // --------------------------------------------------------

    let mut y =
        TEXT_Y;

    for line_index in 0..line_count {
        window.draw_string(
            TEXT_X,
            y,
            bytes_to_str(
                &lines[line_index]
                    [..line_lengths[line_index]],
            ),
            1,
        );

        y +=
            LINE_HEIGHT;
    }

    // --------------------------------------------------------
    // Extra space before prompt
    // --------------------------------------------------------

    y +=
        PROMPT_GAP_Y;

    // --------------------------------------------------------
    // Prompt
    // --------------------------------------------------------

    window.draw_string(
        TEXT_X,
        y,
        ">",
        1,
    );

    // --------------------------------------------------------
    // Input text
    // --------------------------------------------------------

    let input_x =
        TEXT_X
            + CHAR_WIDTH
            + PROMPT_GAP;

    window.draw_string(
        input_x,
        y,
        bytes_to_str(
            &input[..input_length],
        ),
        1,
    );

    window.present();
}

// ============================================================
// Convert ASCII bytes to &str
// ============================================================

fn bytes_to_str(
    bytes: &[u8],
) -> &str {
    unsafe {
        core::str::from_utf8_unchecked(
            bytes,
        )
    }
}

// ============================================================
// Write number into byte buffer
// ============================================================

fn write_number(
    mut value: u64,
    output: &mut [u8],
) -> usize {
    if value == 0 {
        if !output.is_empty() {
            output[0] =
                b'0';

            return 1;
        }

        return 0;
    }

    let mut digits =
        [0u8; 20];

    let mut count =
        0usize;

    while value > 0
        && count < digits.len()
    {
        digits[count] =
            b'0'
                + (value % 10) as u8;

        value /=
            10;

        count +=
            1;
    }

    let length =
        core::cmp::min(
            count,
            output.len(),
        );

    for i in 0..length {
        output[i] =
            digits[count - 1 - i];
    }

    length
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