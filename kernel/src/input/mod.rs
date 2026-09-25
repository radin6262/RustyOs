#![allow(dead_code)]

pub mod key;
pub mod usb;

use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicU8, Ordering};

use spin::Mutex;

pub use key::Key;

// ============================================================
// Public input API
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
    pub scroll_lock: bool,
    pub left_shift: bool,
    pub right_shift: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Button4,
    Button5,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    KeyDown {
        key: Key,
        modifiers: Modifiers,
    },

    KeyUp {
        key: Key,
        modifiers: Modifiers,
    },

    MouseMove {
        x: i32,
        y: i32,
    },

    MouseButtonDown(MouseButton),
    MouseButtonUp(MouseButton),

    MouseWheel {
        delta: i16,
    },
}

// ============================================================
// Queue storage
// ============================================================

const EVENT_QUEUE_LIMIT: usize = 256;

static KEYBOARD_EVENTS: Mutex<VecDeque<InputEvent>> =
    Mutex::new(VecDeque::new());

static MOUSE_EVENTS: Mutex<VecDeque<InputEvent>> =
    Mutex::new(VecDeque::new());

static KEYBOARD_STATE: Mutex<[u8; 8]> =
    Mutex::new([0; 8]);

static MOUSE_BUTTONS: Mutex<u8> =
    Mutex::new(0);

static MOUSE_POSITION: Mutex<(i32, i32)> =
    Mutex::new((0, 0));

static MODIFIERS: Mutex<Modifiers> =
    Mutex::new(Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
        logo: false,
        caps_lock: false,
        num_lock: false,
        scroll_lock: false,
        left_shift: false,
        right_shift: false,
    });

static KEYBOARD_PRESENT: AtomicU8 =
    AtomicU8::new(0);

static MOUSE_PRESENT: AtomicU8 =
    AtomicU8::new(0);

// ============================================================
// Initialization / compatibility
// ============================================================

pub fn init(_bar0: usize) {
    reset_input_state();

    // The old API receives BAR0 from kernel_main(), but the new USB
    // subsystem owns PCI discovery and xHCI initialization itself.
    crate::usb::init::init();
}

fn reset_input_state() {
    {
        let mut queue = KEYBOARD_EVENTS.lock();
        queue.clear();
    }

    {
        let mut queue = MOUSE_EVENTS.lock();
        queue.clear();
    }

    *KEYBOARD_STATE.lock() = [0; 8];
    *MOUSE_BUTTONS.lock() = 0;

    let cursor_x = (crate::graphics::width().saturating_sub(1) / 2) as i32;
    let cursor_y = (crate::graphics::height().saturating_sub(1) / 2) as i32;

    *MOUSE_POSITION.lock() = (cursor_x, cursor_y);

    *MODIFIERS.lock() = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
        logo: false,
        caps_lock: false,
        num_lock: false,
        scroll_lock: false,
        left_shift: false,
        right_shift: false,
    };

    KEYBOARD_PRESENT.store(0, Ordering::Release);
    MOUSE_PRESENT.store(0, Ordering::Release);
}

pub(crate) fn set_keyboard_present(present: bool) {
    KEYBOARD_PRESENT.store(present as u8, Ordering::Release);
}

pub(crate) fn set_mouse_present(present: bool) {
    MOUSE_PRESENT.store(present as u8, Ordering::Release);
}

pub fn has_keyboard() -> bool {
    KEYBOARD_PRESENT.load(Ordering::Acquire) != 0
}

pub fn has_mouse() -> bool {
    MOUSE_PRESENT.load(Ordering::Acquire) != 0
}

// ============================================================
// USB service
// ============================================================

#[inline]
fn service_usb() {
    if crate::usb::init::is_initialized() {
        crate::usb::poll::poll();
    }
}

// ============================================================
// Legacy keyboard API
// ============================================================

pub fn read_key() -> Option<Key> {
    service_usb();

    let event = poll_keyboard_event()?;

    match event {
        InputEvent::KeyDown {
            key,
            modifiers,
        } => key_to_character(key, modifiers),

        _ => None,
    }
}

pub fn poll_keyboard_event() -> Option<InputEvent> {
    KEYBOARD_EVENTS.lock().pop_front()
}

// ============================================================
// Mouse event API
// ============================================================

pub fn poll_mouse_event() -> Option<InputEvent> {
    service_usb();
    MOUSE_EVENTS.lock().pop_front()
}

// ============================================================
// HID keyboard report processing
// ============================================================
//
// Boot-protocol keyboard report:
//
//   byte 0 = modifier bits
//   byte 1 = reserved
//   bytes 2..8 = six simultaneous key usages
//
// ============================================================

pub fn process_keyboard_report(report: &[u8]) {
    if report.len() < 2 {
        return;
    }

    KEYBOARD_PRESENT.store(1, Ordering::Release);

    let modifier_bits = report[0];

    let mut modifiers = *MODIFIERS.lock();

    modifiers.left_shift = (modifier_bits & (1 << 1)) != 0;
    modifiers.right_shift = (modifier_bits & (1 << 5)) != 0;
    modifiers.shift = modifiers.left_shift || modifiers.right_shift;
    modifiers.ctrl = (modifier_bits & 0x11) != 0;
    modifiers.alt = (modifier_bits & 0x44) != 0;
    modifiers.logo = (modifier_bits & 0x88) != 0;

    let mut current = [0u8; 8];
    current[0] = modifier_bits;

    let key_count = report.len().saturating_sub(2).min(6);
    for index in 0..key_count {
        current[index + 2] = report[index + 2];
    }

    let previous = *KEYBOARD_STATE.lock();

    // Lock-state keys toggle when the corresponding usage first appears.
    for index in 2..8 {
        let usage = current[index];
        if usage == 0 || usage == 0x01 {
            continue;
        }

        if !contains_usage(&previous[2..8], usage) {
            match usage {
                0x39 => modifiers.caps_lock = !modifiers.caps_lock,
                0x53 => modifiers.num_lock = !modifiers.num_lock,
                0x47 => modifiers.scroll_lock = !modifiers.scroll_lock,
                _ => {}
            }
        }
    }

    {
        let mut modifier_state = MODIFIERS.lock();
        *modifier_state = modifiers;
    }

    // New key-downs.
    for index in 2..8 {
        let usage = current[index];

        if usage == 0 || usage == 0x01 {
            continue;
        }

        if contains_usage(&previous[2..8], usage) {
            continue;
        }

        let Some(key) = usage_to_key(usage) else {
            continue;
        };

        push_keyboard_event(InputEvent::KeyDown {
            key,
            modifiers,
        });
    }

    // Key releases.
    for index in 2..8 {
        let usage = previous[index];

        if usage == 0 || usage == 0x01 {
            continue;
        }

        if contains_usage(&current[2..8], usage) {
            continue;
        }

        let Some(key) = usage_to_key(usage) else {
            continue;
        };

        push_keyboard_event(InputEvent::KeyUp {
            key,
            modifiers,
        });
    }

    *KEYBOARD_STATE.lock() = current;
}

// ============================================================
// HID mouse report processing
// ============================================================
//
// Boot-protocol mouse report:
//
//   byte 0 = buttons
//   byte 1 = relative X
//   byte 2 = relative Y
//   byte 3 = wheel (when present)
//
// ============================================================

pub fn process_mouse_report(report: &[u8]) {
    if report.len() < 3 {
        return;
    }

    MOUSE_PRESENT.store(1, Ordering::Release);

    let buttons = report[0] & 0x1F;
    let previous = *MOUSE_BUTTONS.lock();

    let dx = report[1] as i8 as i32;
    let dy = report[2] as i8 as i32;

    if dx != 0 || dy != 0 {
        let (x, y) = {
            let mut position = MOUSE_POSITION.lock();

            let max_x = crate::graphics::width()
                .saturating_sub(1) as i32;
            let max_y = crate::graphics::height()
                .saturating_sub(1) as i32;

            let next_x = position.0.saturating_add(dx);
            let next_y = position.1.saturating_add(dy);

            position.0 = if max_x <= 0 {
                0
            } else {
                next_x.clamp(0, max_x)
            };

            position.1 = if max_y <= 0 {
                0
            } else {
                next_y.clamp(0, max_y)
            };

            *position
        };

        push_mouse_event(InputEvent::MouseMove {
            x,
            y,
        });
    }

    let button_map = [
        (0x01, MouseButton::Left),
        (0x02, MouseButton::Right),
        (0x04, MouseButton::Middle),
        (0x08, MouseButton::Button4),
        (0x10, MouseButton::Button5),
    ];

    for &(mask, button) in &button_map {
        let was_down = (previous & mask) != 0;
        let is_down = (buttons & mask) != 0;

        if !was_down && is_down {
            push_mouse_event(InputEvent::MouseButtonDown(button));
        } else if was_down && !is_down {
            push_mouse_event(InputEvent::MouseButtonUp(button));
        }
    }

    if report.len() >= 4 {
        let wheel = report[3] as i8 as i16;

        if wheel != 0 {
            push_mouse_event(InputEvent::MouseWheel {
                delta: wheel,
            });
        }
    }

    *MOUSE_BUTTONS.lock() = buttons;
}

// ============================================================
// Queue helpers
// ============================================================

fn push_keyboard_event(event: InputEvent) {
    let mut queue = KEYBOARD_EVENTS.lock();

    if queue.len() >= EVENT_QUEUE_LIMIT {
        queue.pop_front();
    }

    queue.push_back(event);
}

fn push_mouse_event(event: InputEvent) {
    let mut queue = MOUSE_EVENTS.lock();

    if queue.len() >= EVENT_QUEUE_LIMIT {
        queue.pop_front();
    }

    queue.push_back(event);
}

fn contains_usage(report: &[u8], usage: u8) -> bool {
    report.iter().any(|&value| value == usage)
}

// ============================================================
// HID usage -> physical key
// ============================================================

fn usage_to_key(usage: u8) -> Option<Key> {
    Some(match usage {
        // A-Z
        0x04 => Key::A,
        0x05 => Key::B,
        0x06 => Key::C,
        0x07 => Key::D,
        0x08 => Key::E,
        0x09 => Key::F,
        0x0A => Key::G,
        0x0B => Key::H,
        0x0C => Key::I,
        0x0D => Key::J,
        0x0E => Key::K,
        0x0F => Key::L,
        0x10 => Key::M,
        0x11 => Key::N,
        0x12 => Key::O,
        0x13 => Key::P,
        0x14 => Key::Q,
        0x15 => Key::R,
        0x16 => Key::S,
        0x17 => Key::T,
        0x18 => Key::U,
        0x19 => Key::V,
        0x1A => Key::W,
        0x1B => Key::X,
        0x1C => Key::Y,
        0x1D => Key::Z,

        // Numbers
        0x1E => Key::Num1,
        0x1F => Key::Num2,
        0x20 => Key::Num3,
        0x21 => Key::Num4,
        0x22 => Key::Num5,
        0x23 => Key::Num6,
        0x24 => Key::Num7,
        0x25 => Key::Num8,
        0x26 => Key::Num9,
        0x27 => Key::Num0,

        // Controls
        0x28 => Key::Enter,
        0x29 => Key::Escape,
        0x2A => Key::Backspace,
        0x2B => Key::Tab,
        0x2C => Key::Space,

        // Punctuation
        0x2D => Key::Minus,
        0x2E => Key::Equal,
        0x2F => Key::LeftBracket,
        0x30 => Key::RightBracket,
        0x31 => Key::Backslash,
        0x33 => Key::Semicolon,
        0x34 => Key::Apostrophe,
        0x35 => Key::Grave,
        0x36 => Key::Comma,
        0x37 => Key::Dot,
        0x38 => Key::Slash,

        // Navigation
        0x4F => Key::Right,
        0x50 => Key::Left,
        0x51 => Key::Down,
        0x52 => Key::Up,

        _ => return None,
    })
}

// ============================================================
// Physical key -> produced character
// ============================================================

fn key_to_character(key: Key, modifiers: Modifiers) -> Option<Key> {
    let shift = modifiers.shift;
    let caps = modifiers.caps_lock;

    Some(match key {
        Key::A => Key::Character(if shift ^ caps { 'A' } else { 'a' }),
        Key::B => Key::Character(if shift ^ caps { 'B' } else { 'b' }),
        Key::C => Key::Character(if shift ^ caps { 'C' } else { 'c' }),
        Key::D => Key::Character(if shift ^ caps { 'D' } else { 'd' }),
        Key::E => Key::Character(if shift ^ caps { 'E' } else { 'e' }),
        Key::F => Key::Character(if shift ^ caps { 'F' } else { 'f' }),
        Key::G => Key::Character(if shift ^ caps { 'G' } else { 'g' }),
        Key::H => Key::Character(if shift ^ caps { 'H' } else { 'h' }),
        Key::I => Key::Character(if shift ^ caps { 'I' } else { 'i' }),
        Key::J => Key::Character(if shift ^ caps { 'J' } else { 'j' }),
        Key::K => Key::Character(if shift ^ caps { 'K' } else { 'k' }),
        Key::L => Key::Character(if shift ^ caps { 'L' } else { 'l' }),
        Key::M => Key::Character(if shift ^ caps { 'M' } else { 'm' }),
        Key::N => Key::Character(if shift ^ caps { 'N' } else { 'n' }),
        Key::O => Key::Character(if shift ^ caps { 'O' } else { 'o' }),
        Key::P => Key::Character(if shift ^ caps { 'P' } else { 'p' }),
        Key::Q => Key::Character(if shift ^ caps { 'Q' } else { 'q' }),
        Key::R => Key::Character(if shift ^ caps { 'R' } else { 'r' }),
        Key::S => Key::Character(if shift ^ caps { 'S' } else { 's' }),
        Key::T => Key::Character(if shift ^ caps { 'T' } else { 't' }),
        Key::U => Key::Character(if shift ^ caps { 'U' } else { 'u' }),
        Key::V => Key::Character(if shift ^ caps { 'V' } else { 'v' }),
        Key::W => Key::Character(if shift ^ caps { 'W' } else { 'w' }),
        Key::X => Key::Character(if shift ^ caps { 'X' } else { 'x' }),
        Key::Y => Key::Character(if shift ^ caps { 'Y' } else { 'y' }),
        Key::Z => Key::Character(if shift ^ caps { 'Z' } else { 'z' }),

        Key::Num1 => Key::Character(if shift { '!' } else { '1' }),
        Key::Num2 => Key::Character(if shift { '@' } else { '2' }),
        Key::Num3 => Key::Character(if shift { '#' } else { '3' }),
        Key::Num4 => Key::Character(if shift { '$' } else { '4' }),
        Key::Num5 => Key::Character(if shift { '%' } else { '5' }),
        Key::Num6 => Key::Character(if shift { '^' } else { '6' }),
        Key::Num7 => Key::Character(if shift { '&' } else { '7' }),
        Key::Num8 => Key::Character(if shift { '*' } else { '8' }),
        Key::Num9 => Key::Character(if shift { '(' } else { '9' }),
        Key::Num0 => Key::Character(if shift { ')' } else { '0' }),

        Key::Minus => Key::Character(if shift { '_' } else { '-' }),
        Key::Equal => Key::Character(if shift { '+' } else { '=' }),
        Key::LeftBracket => Key::Character(if shift { '{' } else { '[' }),
        Key::RightBracket => Key::Character(if shift { '}' } else { ']' }),
        Key::Backslash => Key::Character(if shift { '|' } else { '\\' }),
        Key::Semicolon => Key::Character(if shift { ':' } else { ';' }),
        Key::Apostrophe => Key::Character(if shift { '"' } else { '\'' }),
        Key::Grave => Key::Character(if shift { '~' } else { '`' }),
        Key::Comma => Key::Character(if shift { '<' } else { ',' }),
        Key::Dot => Key::Character(if shift { '>' } else { '.' }),
        Key::Slash => Key::Character(if shift { '?' } else { '/' }),

        Key::Enter => Key::Enter,
        Key::Escape => Key::Escape,
        Key::Backspace => Key::Backspace,
        Key::Tab => Key::Tab,
        Key::Space => Key::Space,

        Key::Up => Key::Up,
        Key::Down => Key::Down,
        Key::Left => Key::Left,
        Key::Right => Key::Right,

        Key::Character(c) => Key::Character(c),
        Key::Unknown => Key::Unknown,
    })
}
