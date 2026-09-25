use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use spin::Mutex;
use x86_64::instructions::port::Port;

use crate::graphics::Color;

const COM1: u16 = 0x3F8;

// ============================================================
// Screen logging
// ============================================================

static SCREEN_LOGGING: AtomicBool = AtomicBool::new(false);

static SCREEN_LOG_RENDERING: AtomicBool = AtomicBool::new(false);

static SCREEN_LOG_DIRTY: AtomicBool = AtomicBool::new(false);

static SCREEN_LOG_WINDOW: AtomicU64 = AtomicU64::new(u64::MAX);

static SCREEN_LOG: Mutex<Option<String>> = Mutex::new(None);

// Hard upper bound for the retained log text.
//
// The logger never intentionally stores more than this many UTF-8 bytes.
const SCREEN_LOG_MAX: usize = 32 * 1024;

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
    screen_width().saturating_sub(SCREEN_LOG_MARGIN * 2)
}

fn screen_log_height() -> usize {
    screen_height().saturating_sub(SCREEN_LOG_MARGIN * 2)
}

// ============================================================
// Serial initialization
// ============================================================

pub fn init() {
    unsafe {
        let mut port = Port::<u8>::new(COM1 + 1);

        port.write(0x00);

        let mut port = Port::<u8>::new(COM1 + 3);

        port.write(0x80);

        let mut port = Port::<u8>::new(COM1);

        port.write(0x01);

        let mut port = Port::<u8>::new(COM1 + 1);

        port.write(0x00);

        let mut port = Port::<u8>::new(COM1 + 3);

        port.write(0x03);

        let mut port = Port::<u8>::new(COM1 + 2);

        port.write(0xC7);

        let mut port = Port::<u8>::new(COM1 + 4);

        port.write(0x0B);
    }
}

// ============================================================
// Serial output
// ============================================================

fn is_transmit_empty() -> bool {
    unsafe {
        let mut port = Port::<u8>::new(COM1 + 5);

        (port.read() & 0x20) != 0
    }
}

pub fn write_byte(byte: u8) {
    while !is_transmit_empty() {
        core::hint::spin_loop();
    }

    unsafe {
        let mut port = Port::<u8>::new(COM1);

        port.write(byte);
    }
}

pub fn write_str(string: &str) {
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

    if SCREEN_LOGGING.load(Ordering::Acquire) {
        write_screen_log(string);
    }
}

// ============================================================
// Hex output
// ============================================================

pub fn write_hex(value: u64) {
    let mut buffer = [0u8; 18];

    buffer[0] = b'0';
    buffer[1] = b'x';

    let mut i = 0usize;

    while i < 16 {
        let shift = 60 - i * 4;

        let digit = ((value >> shift) & 0xF) as u8;

        buffer[2 + i] = match digit {
            0..=9 => b'0' + digit,

            _ => b'A' + (digit - 10),
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

    if SCREEN_LOGGING.load(Ordering::Acquire) {
        if let Ok(text) = core::str::from_utf8(&buffer) {
            write_screen_log(text);
        }
    }
}

// ============================================================
// usize output
// ============================================================

pub fn write_usize(value: usize) {
    write_hex(value as u64);
}

// ============================================================
// Screen logger initialization
// ============================================================

pub fn enable_screen_logging() {
    let width = screen_width();

    let height = screen_height();

    if width == 0 || height == 0 {
        return;
    }

    let x = screen_log_x();

    let y = screen_log_y();

    let window_width = screen_log_width();

    let window_height = screen_log_height();

    if window_width == 0 || window_height == 0 {
        return;
    }

    let x_usize = if x < 0 {
        return;
    } else {
        x as usize
    };

    let y_usize = if y < 0 {
        return;
    } else {
        y as usize
    };

    let right = x_usize.saturating_add(window_width);

    let bottom = y_usize.saturating_add(window_height);

    if x_usize >= width || y_usize >= height || right > width || bottom > height {
        return;
    }

    // --------------------------------------------------------
    // Acquire compositor.
    // --------------------------------------------------------

    let Some(mut wm_lock) = crate::wm::WM.try_lock() else {
        return;
    };

    let Some(wm) = wm_lock.as_mut() else {
        return;
    };

    // --------------------------------------------------------
    // Create logger window.
    // --------------------------------------------------------

    let window_id = wm.create_window(x, y, window_width, window_height);

    // --------------------------------------------------------
    // Configure logger window.
    // --------------------------------------------------------

    let Some(window) = wm.windows.get_mut(&window_id) else {
        return;
    };

    window.set_title("Rusty Serial Log");

    window.clear(0xFF101010);

    // --------------------------------------------------------
    // Reset log.
    // --------------------------------------------------------

    {
        let mut log_lock = SCREEN_LOG.lock();

        *log_lock = Some(String::with_capacity(SCREEN_LOG_MAX));

        if let Some(log) = log_lock.as_mut() {
            append_screen_log_bounded(log, "Rusty serial screen logging enabled\n");
        }
    }

    // --------------------------------------------------------
    // Publish window.
    // --------------------------------------------------------

    SCREEN_LOG_WINDOW.store(window_id, Ordering::Release);

    SCREEN_LOG_DIRTY.store(true, Ordering::Release);

    SCREEN_LOG_RENDERING.store(false, Ordering::Release);

    SCREEN_LOGGING.store(true, Ordering::Release);

    // --------------------------------------------------------
    // Initial render.
    // --------------------------------------------------------

    render_screen_log();
}

// ============================================================
// Append bounded screen-log data
// ============================================================
//
// This function guarantees that `log.len()` never intentionally
// grows beyond SCREEN_LOG_MAX.
//
// This is important because a single very large write_str() must
// not temporarily allocate an arbitrarily large String.
//
// The old implementation did:
//
//     log.push_str(string);
//     trim afterwards;
//
// which could make the String grow far beyond the intended limit.
// It also had a specific newline-at-end case where trim_at became
// log.len(), causing the oversized string to remain permanently.
//
// ============================================================

fn append_screen_log_bounded(log: &mut String, string: &str) {
    if SCREEN_LOG_MAX == 0 {
        return;
    }

    // --------------------------------------------------------
    // Fast path: the incoming string itself is larger than the
    // entire retained history. Keep only its newest suffix.
    // --------------------------------------------------------

    if string.len() >= SCREEN_LOG_MAX {
        let mut start = string.len() - SCREEN_LOG_MAX;

        // Move forward to a UTF-8 character boundary.
        while start < string.len() && !string.is_char_boundary(start) {
            start += 1;
        }

        // Prefer beginning at a complete line when possible.
        // Do not use a newline at the very end as the trim point,
        // because that would remove the entire retained string.
        if let Some(relative) = string[start..].find('\n') {
            let candidate = start + relative + 1;

            if candidate < string.len() {
                start = candidate;
            }
        }

        log.clear();
        log.push_str(&string[start..]);

        // The boundary above ensures valid UTF-8 and the source
        // itself is at most SCREEN_LOG_MAX bytes after trimming.
        return;
    }

    // --------------------------------------------------------
    // Make room BEFORE appending.
    // --------------------------------------------------------

    let required = log.len().saturating_add(string.len());

    if required <= SCREEN_LOG_MAX {
        log.push_str(string);

        return;
    }

    let bytes_to_remove = required - SCREEN_LOG_MAX;

    // --------------------------------------------------------
    // Start at the minimum number of bytes that must be removed.
    // --------------------------------------------------------

    let mut trim_at = bytes_to_remove.min(log.len());

    // Never cut through a UTF-8 character.
    while trim_at < log.len() && !log.is_char_boundary(trim_at) {
        trim_at += 1;
    }

    // --------------------------------------------------------
    // Prefer dropping through the next newline so that the first
    // visible retained line remains complete.
    // --------------------------------------------------------

    if let Some(relative) = log[trim_at..].find('\n') {
        let candidate = trim_at + relative + 1;

        // CRITICAL:
        // Do NOT replace trim_at with `candidate` when candidate
        // equals log.len(). The old logger did that and then
        // refused to drain because trim_at == log.len(), allowing
        // the buffer to grow forever.
        if candidate < log.len() {
            trim_at = candidate;
        }
    }

    if trim_at > 0 {
        log.drain(..trim_at);
    }

    // --------------------------------------------------------
    // Now there is guaranteed room for the incoming string.
    // --------------------------------------------------------

    let remaining = SCREEN_LOG_MAX.saturating_sub(log.len());

    if string.len() <= remaining {
        log.push_str(string);
    } else {
        // This should only be reachable because `trim_at` was
        // rounded forward to a UTF-8 boundary and therefore the
        // retained old prefix can differ slightly from the exact
        // byte target.
        let mut start = string.len() - remaining;

        while start < string.len() && !string.is_char_boundary(start) {
            start += 1;
        }

        log.push_str(&string[start..]);
    }

    // Defensive invariant. This branch should never execute, but
    // keeping it here makes the memory bound explicit even if this
    // function is changed later.
    if log.len() > SCREEN_LOG_MAX {
        let mut start = log.len() - SCREEN_LOG_MAX;

        while start < log.len() && !log.is_char_boundary(start) {
            start += 1;
        }

        let suffix = log[start..].to_owned();

        log.clear();
        log.push_str(&suffix);
    }
}

// ============================================================
// Append to screen log
// ============================================================

fn write_screen_log(string: &str) {
    if SCREEN_LOG_WINDOW.load(Ordering::Acquire) == u64::MAX {
        return;
    }

    // --------------------------------------------------------
    // Append while keeping a hard memory bound.
    // --------------------------------------------------------

    {
        let mut log_lock = SCREEN_LOG.lock();

        let log = log_lock.get_or_insert_with(|| String::with_capacity(SCREEN_LOG_MAX));

        append_screen_log_bounded(log, string);
    }

    SCREEN_LOG_DIRTY.store(true, Ordering::Release);

    // --------------------------------------------------------
    // Render immediately.
    //
    // The dirty flag ensures that if another serial write occurs
    // re-entrantly while the compositor is rendering, the next
    // render pass will see the updated log.
    // --------------------------------------------------------

    render_screen_log();
}

// ============================================================
// Render screen log
// ============================================================

fn render_screen_log() {
    // --------------------------------------------------------
    // Never recursively enter the renderer.
    // --------------------------------------------------------

    if SCREEN_LOG_RENDERING.swap(true, Ordering::Acquire) {
        return;
    }

    // Render at least once. If serial output arrives while the
    // render is in progress, allow one additional pass. The loop
    // is deliberately bounded so recursive logging cannot create
    // an infinite render loop.
    for _ in 0..2 {
        SCREEN_LOG_DIRTY.store(false, Ordering::Release);

        render_screen_log_inner();

        if !SCREEN_LOG_DIRTY.load(Ordering::Acquire) {
            break;
        }
    }

    SCREEN_LOG_RENDERING.store(false, Ordering::Release);
}

// ============================================================
// Count wrapped lines
// ============================================================
//
// This computes how many rendered lines a single logical input
// line occupies without allocating temporary String objects.
//
// ============================================================

fn wrapped_line_count(line: &str, max_chars: usize) -> usize {
    let max_chars = max_chars.max(1);

    let line = line.trim_end_matches('\r');

    if line.is_empty() {
        return 1;
    }

    let char_count = line.chars().count();

    ((char_count - 1) / max_chars) + 1
}

// ============================================================
// Find the first visible logical/wrapped line
// ============================================================

fn total_wrapped_lines(log: &str, max_chars: usize) -> usize {
    let mut total = 0usize;

    for line in log.split('\n') {
        total = total.saturating_add(wrapped_line_count(line, max_chars));
    }

    // split('\n') intentionally produces one empty final item for
    // a trailing newline. That represents an actual blank terminal
    // line, but the original renderer suppressed the artificial
    // extra line. Preserve that behavior.
    if log.ends_with('\n') {
        total = total.saturating_sub(1);
    }

    total
}

// ============================================================
// Render one logical line in fixed-width character chunks
// ============================================================
//
// Returns the number of wrapped lines rendered.
//
// `skip` is the number of wrapped chunks to skip before drawing.
//
// This operates on UTF-8 character boundaries and does not create
// temporary Strings for each wrapped line.
//
// ============================================================

fn render_wrapped_line(
    window: &mut crate::wm::Window,
    line: &str,
    max_chars: usize,
    skip: &mut usize,
    remaining_lines: &mut usize,
    line_y: &mut usize,
    text_height: usize,
    scaled_line_height: usize,
) {
    let max_chars = max_chars.max(1);

    let line = line.trim_end_matches('\r');

    if line.is_empty() {
        if *skip > 0 {
            *skip -= 1;
            return;
        }

        if *remaining_lines == 0 {
            return;
        }

        if *line_y < text_height {
            // Empty line: consume vertical space without drawing.
            *line_y = line_y.saturating_add(scaled_line_height);
        }

        *remaining_lines -= 1;
        return;
    }

    let mut chunk_start = 0usize;

    let mut chunk_chars = 0usize;

    for (index, ch) in line.char_indices() {
        if chunk_chars == max_chars {
            render_one_chunk(
                window,
                line,
                chunk_start,
                index,
                skip,
                remaining_lines,
                line_y,
                text_height,
                scaled_line_height,
            );

            if *remaining_lines == 0 {
                return;
            }

            chunk_start = index;
            chunk_chars = 0;
        }

        let _ = ch;
        chunk_chars += 1;
    }

    // Final chunk.
    render_one_chunk(
        window,
        line,
        chunk_start,
        line.len(),
        skip,
        remaining_lines,
        line_y,
        text_height,
        scaled_line_height,
    );
}

// ============================================================
// Draw one wrapped chunk
// ============================================================

fn render_one_chunk(
    window: &mut crate::wm::Window,
    line: &str,
    start: usize,
    end: usize,
    skip: &mut usize,
    remaining_lines: &mut usize,
    line_y: &mut usize,
    text_height: usize,
    scaled_line_height: usize,
) {
    if start >= end {
        return;
    }

    if *skip > 0 {
        *skip -= 1;
        return;
    }

    if *remaining_lines == 0 {
        return;
    }

    if *line_y < text_height {
        window.draw_string(
            SCREEN_LOG_PADDING,
            *line_y,
            &line[start..end],
            Color {
                r: 235,
                g: 235,
                b: 235,
                a: 255,
            },
            SCREEN_LOG_SCALE,
        );
    }

    *line_y = line_y.saturating_add(scaled_line_height);

    *remaining_lines -= 1;
}

// ============================================================
// Actual renderer
// ============================================================

fn render_screen_log_inner() {
    let window_id = SCREEN_LOG_WINDOW.load(Ordering::Acquire);

    if window_id == u64::MAX {
        return;
    }

    // --------------------------------------------------------
    // Screen dimensions.
    // --------------------------------------------------------

    let width = screen_width();

    let height = screen_height();

    if width == 0 || height == 0 {
        return;
    }

    // --------------------------------------------------------
    // Logger dimensions.
    // --------------------------------------------------------

    let window_width = screen_log_width();

    let window_height = screen_log_height();

    if window_width == 0 || window_height == 0 {
        return;
    }

    // --------------------------------------------------------
    // Text area.
    // --------------------------------------------------------

    let text_width = window_width.saturating_sub(SCREEN_LOG_PADDING * 2);

    let text_height =
        window_height.saturating_sub(SCREEN_LOG_TITLE_BAR_HEIGHT + SCREEN_LOG_PADDING * 2);

    if text_width == 0 || text_height == 0 {
        return;
    }

    // --------------------------------------------------------
    // Account for scale.
    // --------------------------------------------------------

    let scale = SCREEN_LOG_SCALE.max(1);

    let scaled_char_width = SCREEN_LOG_CHAR_WIDTH.saturating_mul(scale);

    let scaled_line_height = SCREEN_LOG_LINE_HEIGHT.max(1).saturating_mul(scale);

    if scaled_char_width == 0 || scaled_line_height == 0 {
        return;
    }

    // --------------------------------------------------------
    // Maximum rendered characters per line.
    // --------------------------------------------------------

    let max_chars = (text_width / scaled_char_width).max(1);

    // --------------------------------------------------------
    // Maximum visible lines.
    // --------------------------------------------------------

    let max_lines = (text_height / scaled_line_height).max(1);

    // --------------------------------------------------------
    // Snapshot the bounded log.
    // --------------------------------------------------------

    let log = {
        let log_lock = SCREEN_LOG.lock();

        let Some(log) = log_lock.as_ref() else {
            return;
        };

        // The retained log is hard-bounded, so this copy is
        // also hard-bounded. A String is used because all
        // following operations are UTF-8 operations.
        String::from(log)
    };

    // --------------------------------------------------------
    // Determine how many wrapped lines exist without building a
    // huge Vec<String>.
    // --------------------------------------------------------

    let total_lines = total_wrapped_lines(&log, max_chars);

    let skip_lines = total_lines.saturating_sub(max_lines);

    // --------------------------------------------------------
    // Acquire compositor.
    // --------------------------------------------------------

    let Some(mut wm_lock) = crate::wm::WM.try_lock() else {
        return;
    };

    let Some(wm) = wm_lock.as_mut() else {
        return;
    };

    // --------------------------------------------------------
    // Find logger window.
    // --------------------------------------------------------

    let Some(window) = wm.windows.get_mut(&window_id) else {
        // The logger window may have been destroyed by the window
        // manager. Do not keep attempting to render into it.
        SCREEN_LOG_WINDOW.store(u64::MAX, Ordering::Release);
        SCREEN_LOGGING.store(false, Ordering::Release);
        return;
    };

    // --------------------------------------------------------
    // Clear old frame.
    // --------------------------------------------------------

    window.clear(0xFF101010);

    // --------------------------------------------------------
    // Draw only the newest visible wrapped lines.
    //
    // No Vec<String> is created here.
    // --------------------------------------------------------

    let mut skip = skip_lines;

    let mut remaining_lines = max_lines.min(total_lines);

    let mut line_y = SCREEN_LOG_PADDING;

    for line in log.split('\n') {
        if remaining_lines == 0 {
            break;
        }

        render_wrapped_line(
            window,
            line,
            max_chars,
            &mut skip,
            &mut remaining_lines,
            &mut line_y,
            text_height + SCREEN_LOG_PADDING,
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
    SCREEN_LOGGING.store(false, Ordering::Release);

    SCREEN_LOG_RENDERING.store(false, Ordering::Release);

    SCREEN_LOG_DIRTY.store(false, Ordering::Release);

    SCREEN_LOG_WINDOW.store(u64::MAX, Ordering::Release);

    let mut log_lock = SCREEN_LOG.lock();

    *log_lock = None;
}
