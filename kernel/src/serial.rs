use alloc::string::String;
use alloc::vec::Vec;

use core::sync::atomic::{
    AtomicBool,
    AtomicU64,
    Ordering,
};

use spin::Mutex;
use x86_64::instructions::port::Port;

use crate::graphics::Color;

const COM1: u16 = 0x3F8;

// ============================================================
// Screen logging
// ============================================================

static SCREEN_LOGGING: AtomicBool =
    AtomicBool::new(false);

static SCREEN_LOG_RENDERING: AtomicBool =
    AtomicBool::new(false);

static SCREEN_LOG_WINDOW: AtomicU64 =
    AtomicU64::new(u64::MAX);

static SCREEN_LOG: Mutex<Option<String>> =
    Mutex::new(None);

const SCREEN_LOG_MAX: usize =
    32 * 1024;


// ============================================================
// Screen log configuration
// ============================================================

const SCREEN_LOG_MARGIN: usize = 32;

const SCREEN_LOG_PADDING: usize = 12;

const SCREEN_LOG_SCALE: usize = 1;

const SCREEN_LOG_LINE_HEIGHT: usize = 20;

// Width of one character at scale 1.
//
// This MUST match the actual font width used by
// Window::draw_string().
const SCREEN_LOG_CHAR_WIDTH: usize = 12;

const SCREEN_LOG_TITLE_BAR_HEIGHT: usize = 0;


// ============================================================
// Actual screen dimensions
// ============================================================

fn screen_width() -> usize {
    crate::graphics::width()
}

fn screen_height() -> usize {
    crate::graphics::height()
}


// ============================================================
// Logger geometry
// ============================================================

fn screen_log_x() -> i32 {
    SCREEN_LOG_MARGIN as i32
}

fn screen_log_y() -> i32 {
    SCREEN_LOG_MARGIN as i32
}

fn screen_log_width() -> usize {
    screen_width()
        .saturating_sub(
            SCREEN_LOG_MARGIN * 2,
        )
}

fn screen_log_height() -> usize {
    screen_height()
        .saturating_sub(
            SCREEN_LOG_MARGIN * 2,
        )
}


// ============================================================
// Serial initialization
// ============================================================

pub fn init() {
    unsafe {
        let mut port =
            Port::<u8>::new(COM1 + 1);

        port.write(0x00);

        let mut port =
            Port::<u8>::new(COM1 + 3);

        port.write(0x80);

        let mut port =
            Port::<u8>::new(COM1);

        port.write(0x01);

        let mut port =
            Port::<u8>::new(COM1 + 1);

        port.write(0x00);

        let mut port =
            Port::<u8>::new(COM1 + 3);

        port.write(0x03);

        let mut port =
            Port::<u8>::new(COM1 + 2);

        port.write(0xC7);

        let mut port =
            Port::<u8>::new(COM1 + 4);

        port.write(0x0B);
    }
}


// ============================================================
// Serial output
// ============================================================

fn is_transmit_empty() -> bool {
    unsafe {
        let mut port =
            Port::<u8>::new(COM1 + 5);

        (port.read() & 0x20) != 0
    }
}


pub fn write_byte(
    byte: u8,
) {
    while !is_transmit_empty() {
        core::hint::spin_loop();
    }

    unsafe {
        let mut port =
            Port::<u8>::new(COM1);

        port.write(byte);
    }
}


pub fn write_str(
    string: &str,
) {
    // --------------------------------------------------------
    // Real serial port.
    // --------------------------------------------------------

    for byte in string.bytes() {
        if byte == b'\n' {
            write_byte(b'\r');
        }

        write_byte(byte);
    }

    // --------------------------------------------------------
    // Screen mirror.
    // --------------------------------------------------------

    if SCREEN_LOGGING.load(
        Ordering::Acquire,
    ) {
        write_screen_log(
            string,
        );
    }
}


// ============================================================
// Hex output
// ============================================================

pub fn write_hex(
    value: u64,
) {
    let mut buffer =
        [0u8; 18];

    buffer[0] = b'0';
    buffer[1] = b'x';

    let mut i = 0usize;

    while i < 16 {
        let shift =
            60 - i * 4;

        let digit =
            ((value >> shift) & 0xF)
                as u8;

        buffer[2 + i] =
            match digit {
                0..=9 =>
                    b'0' + digit,

                _ =>
                    b'A' +
                        (digit - 10),
            };

        i += 1;
    }

    // --------------------------------------------------------
    // Real serial port.
    // --------------------------------------------------------

    for byte in buffer {
        write_byte(byte);
    }

    // --------------------------------------------------------
    // Screen mirror.
    // --------------------------------------------------------

    if SCREEN_LOGGING.load(
        Ordering::Acquire,
    ) {
        if let Ok(text) =
            core::str::from_utf8(
                &buffer,
            )
        {
            write_screen_log(
                text,
            );
        }
    }
}


// ============================================================
// usize output
// ============================================================

pub fn write_usize(
    value: usize,
) {
    write_hex(
        value as u64,
    );
}


// ============================================================
// Screen logger initialization
// ============================================================

pub fn enable_screen_logging() {
    let width =
        screen_width();

    let height =
        screen_height();

    if width == 0 ||
        height == 0
    {
        return;
    }

    let x =
        screen_log_x();

    let y =
        screen_log_y();

    let window_width =
        screen_log_width();

    let window_height =
        screen_log_height();

    if window_width == 0 ||
        window_height == 0
    {
        return;
    }

    let x_usize =
        if x < 0 {
            return;
        } else {
            x as usize
        };

    let y_usize =
        if y < 0 {
            return;
        } else {
            y as usize
        };

    let right =
        x_usize.saturating_add(
            window_width,
        );

    let bottom =
        y_usize.saturating_add(
            window_height,
        );

    if x_usize >= width ||
        y_usize >= height ||
        right > width ||
        bottom > height
    {
        return;
    }

    // --------------------------------------------------------
    // Acquire compositor.
    // --------------------------------------------------------

    let Some(mut wm_lock) =
        crate::wm::WM.try_lock()
    else {
        return;
    };

    let Some(wm) =
        wm_lock.as_mut()
    else {
        return;
    };

    // --------------------------------------------------------
    // Create logger window.
    // --------------------------------------------------------

    let window_id =
        wm.create_window(
            x,
            y,
            window_width,
            window_height,
        );

    // --------------------------------------------------------
    // Configure logger window.
    // --------------------------------------------------------

    let Some(window) =
        wm.windows.get_mut(
            &window_id,
        )
    else {
        return;
    };

    window.set_title(
        "Rusty Serial Log",
    );

    window.clear(
        0xFF101010,
    );

    // --------------------------------------------------------
    // Reset log.
    // --------------------------------------------------------

    {
        let mut log_lock =
            SCREEN_LOG.lock();

        *log_lock =
            Some(
                String::from(
                    "Rusty serial screen logging enabled\n",
                ),
            );
    }

    // --------------------------------------------------------
    // Publish window.
    // --------------------------------------------------------

    SCREEN_LOG_WINDOW.store(
        window_id,
        Ordering::Release,
    );

    SCREEN_LOG_RENDERING.store(
        false,
        Ordering::Release,
    );

    SCREEN_LOGGING.store(
        true,
        Ordering::Release,
    );

    // --------------------------------------------------------
    // Initial render.
    // --------------------------------------------------------

    render_screen_log();
}


// ============================================================
// Append to screen log
// ============================================================

fn write_screen_log(
    string: &str,
) {
    if SCREEN_LOG_WINDOW.load(
        Ordering::Acquire,
    ) == u64::MAX
    {
        return;
    }

    // --------------------------------------------------------
    // Append.
    // --------------------------------------------------------

    {
        let mut log_lock =
            SCREEN_LOG.lock();

        let log =
            log_lock
                .get_or_insert_with(
                    String::new,
                );

        log.push_str(
            string,
        );

        // ----------------------------------------------------
        // Keep newest data.
        // ----------------------------------------------------

        if log.len() >
            SCREEN_LOG_MAX
        {
            let remove_before =
                log.len()
                    - SCREEN_LOG_MAX;

            let mut trim_at =
                remove_before;

            // ------------------------------------------------
            // UTF-8 boundary.
            // ------------------------------------------------

            while trim_at <
                log.len()
                &&
                !log.is_char_boundary(
                    trim_at,
                )
            {
                trim_at += 1;
            }

            // ------------------------------------------------
            // Prefer removing complete lines.
            // ------------------------------------------------

            if let Some(relative) =
                log[trim_at..]
                    .find('\n')
            {
                trim_at +=
                    relative + 1;
            }

            // ------------------------------------------------
            // Never remove the entire string.
            // ------------------------------------------------

            if trim_at > 0 &&
                trim_at < log.len()
            {
                log.drain(
                    ..trim_at,
                );
            }
        }
    }

    // --------------------------------------------------------
    // Render immediately.
    // --------------------------------------------------------

    render_screen_log();
}


// ============================================================
// Render screen log
// ============================================================

fn render_screen_log() {
    // --------------------------------------------------------
    // Only one renderer at a time.
    // --------------------------------------------------------

    if SCREEN_LOG_RENDERING.swap(
        true,
        Ordering::Acquire,
    ) {
        return;
    }

    render_screen_log_inner();

    SCREEN_LOG_RENDERING.store(
        false,
        Ordering::Release,
    );
}


// ============================================================
// Wrap one line
// ============================================================
//
// IMPORTANT:
//
// This function NEVER returns a line longer than max_chars.
//
// It operates on Unicode characters rather than bytes.
//
// That means a long write_str() such as:
//
// "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA..."
//
// is split into independent renderable lines.
//

fn wrap_line(
    line: &str,
    max_chars: usize,
    output: &mut Vec<String>,
) {
    let max_chars =
        max_chars.max(1);

    // --------------------------------------------------------
    // Remove carriage returns.
    //
    // Serial-style strings may contain \r\n.
    // \r has no useful meaning for the screen logger.
    // --------------------------------------------------------

    let line =
        line.trim_end_matches(
            '\r',
        );

    // --------------------------------------------------------
    // Empty line.
    // --------------------------------------------------------

    if line.is_empty() {
        output.push(
            String::new(),
        );

        return;
    }

    // --------------------------------------------------------
    // Build one wrapped line at a time.
    // --------------------------------------------------------

    let mut current =
        String::new();

    let mut count =
        0usize;

    for ch in line.chars() {
        // ----------------------------------------------------
        // If this character would exceed the available width,
        // finish the current rendered line FIRST.
        // ----------------------------------------------------

        if count >= max_chars {
            output.push(
                core::mem::take(
                    &mut current,
                ),
            );

            count = 0;
        }

        current.push(
            ch,
        );

        count += 1;
    }

    // --------------------------------------------------------
    // Final partial line.
    // --------------------------------------------------------

    if !current.is_empty() {
        output.push(
            current,
        );
    }
}


// ============================================================
// Actual renderer
// ============================================================

fn render_screen_log_inner() {
    let window_id =
        SCREEN_LOG_WINDOW.load(
            Ordering::Acquire,
        );

    if window_id ==
        u64::MAX
    {
        return;
    }

    // --------------------------------------------------------
    // Screen dimensions.
    // --------------------------------------------------------

    let width =
        screen_width();

    let height =
        screen_height();

    if width == 0 ||
        height == 0
    {
        return;
    }

    // --------------------------------------------------------
    // Logger dimensions.
    // --------------------------------------------------------

    let window_width =
        screen_log_width();

    let window_height =
        screen_log_height();

    if window_width == 0 ||
        window_height == 0
    {
        return;
    }

    // --------------------------------------------------------
    // Text area.
    // --------------------------------------------------------

    let text_width =
        window_width
            .saturating_sub(
                SCREEN_LOG_PADDING * 2,
            );

    let text_height =
        window_height
            .saturating_sub(
                SCREEN_LOG_TITLE_BAR_HEIGHT
                    + SCREEN_LOG_PADDING * 2,
            );

    if text_width == 0 ||
        text_height == 0
    {
        return;
    }

    // --------------------------------------------------------
    // Account for scale.
    // --------------------------------------------------------

    let scale =
        SCREEN_LOG_SCALE.max(1);

    let scaled_char_width =
        SCREEN_LOG_CHAR_WIDTH
            .saturating_mul(
                scale,
            );

    let scaled_line_height =
        SCREEN_LOG_LINE_HEIGHT
            .max(1)
            .saturating_mul(
                scale,
            );

    if scaled_char_width == 0 ||
        scaled_line_height == 0
    {
        return;
    }

    // --------------------------------------------------------
    // Calculate maximum characters.
    //
    // Reserve one complete glyph width so that the final
    // character cannot touch the right edge.
    // --------------------------------------------------------

    let max_chars =
        text_width
            .saturating_sub(
                scaled_char_width,
            )
            / scaled_char_width;

    let max_chars =
        max_chars.max(1);

    // --------------------------------------------------------
    // Calculate maximum visible lines.
    // --------------------------------------------------------

    let max_lines =
        text_height
            / scaled_line_height;

    let max_lines =
        max_lines.max(1);

    // --------------------------------------------------------
    // Snapshot the log.
    // --------------------------------------------------------

    let log =
        {
            let log_lock =
                SCREEN_LOG.lock();

            let Some(log) =
                log_lock.as_ref()
            else {
                return;
            };

            String::from(
                log,
            )
        };

    // --------------------------------------------------------
    // Wrap EVERYTHING into independent lines.
    // --------------------------------------------------------

    let mut lines =
        Vec::<String>::new();

    for line in
        log.split('\n')
    {
        wrap_line(
            line,
            max_chars,
            &mut lines,
        );
    }

    // --------------------------------------------------------
    // If the log ends with \n, split('\n') creates one
    // artificial empty line. Remove it.
    // --------------------------------------------------------

    if log.ends_with('\n') {
        if lines
            .last()
            .map(
                |line| line.is_empty(),
            )
            .unwrap_or(false)
        {
            lines.pop();
        }
    }

    // --------------------------------------------------------
    // Determine the newest visible lines.
    // --------------------------------------------------------

    let first_visible =
        lines.len()
            .saturating_sub(
                max_lines,
            );

    // --------------------------------------------------------
    // Acquire compositor.
    // --------------------------------------------------------

    let Some(mut wm_lock) =
        crate::wm::WM.try_lock()
    else {
        return;
    };

    let Some(wm) =
        wm_lock.as_mut()
    else {
        return;
    };

    // --------------------------------------------------------
    // Find logger window.
    // --------------------------------------------------------

    let Some(window) =
        wm.windows.get_mut(
            &window_id,
        )
    else {
        return;
    };

    // --------------------------------------------------------
    // Clear old frame.
    // --------------------------------------------------------

    window.clear(
        0xFF101010,
    );

    // --------------------------------------------------------
    // Draw EVERY LINE SEPARATELY.
    //
    // THIS IS THE IMPORTANT FIX.
    //
    // We do NOT pass "\n" to draw_string().
    //
    // Every call starts at exactly the same X coordinate.
    // Therefore a 100-line write_str() can never accumulate
    // X position across lines.
    // --------------------------------------------------------

    let mut line_y =
        SCREEN_LOG_PADDING;

    for index in
        first_visible..lines.len()
    {
        // ----------------------------------------------------
        // Absolute safety check.
        // ----------------------------------------------------

        if line_y >=
            text_height
                + SCREEN_LOG_PADDING
        {
            break;
        }

        let line =
            &lines[index];

        // ----------------------------------------------------
        // Empty lines still consume vertical space.
        // ----------------------------------------------------

        if !line.is_empty() {
            window.draw_string(
                SCREEN_LOG_PADDING,
                line_y,
                line,
                Color {
                    r: 235,
                    g: 235,
                    b: 235,
                    a: 255,
                },
                SCREEN_LOG_SCALE,
            );
        }

        // ----------------------------------------------------
        // Move ONLY vertically.
        //
        // X is reset on every draw_string() call.
        // ----------------------------------------------------

        line_y =
            line_y.saturating_add(
                scaled_line_height,
            );
    }

    // --------------------------------------------------------
    // Present.
    // --------------------------------------------------------

    wm.draw();
}


// ============================================================
// Shutdown
// ============================================================

pub fn disable_screen_logging() {
    SCREEN_LOGGING.store(
        false,
        Ordering::Release,
    );

    SCREEN_LOG_RENDERING.store(
        false,
        Ordering::Release,
    );

    SCREEN_LOG_WINDOW.store(
        u64::MAX,
        Ordering::Release,
    );

    let mut log_lock =
        SCREEN_LOG.lock();

    *log_lock =
        None;
}