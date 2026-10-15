#![allow(dead_code)]

// ============================================================
// PS/2 scancode set 1 decoder
// ============================================================
//
// The i8042 is configured with translation enabled (see ps2::mod), so the
// keyboard hands us scancode set 1:
//
//     0x01..0x53          make code
//     make | 0x80         break code (key release)
//     0xE0 <code>         extended (arrow keys, right-hand modifiers, gui)
//     0xE1 1D 45 E1 9D C5 Pause (no Key variant, swallowed)
//
// Decoding produces the same `Key`/`Modifiers` events the previous USB HID
// layer produced, so the compositor and the syscall layer are unaffected.
// ============================================================

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use spin::Mutex;

use crate::input::{InputEvent, Key};

// ============================================================
// Prefix state
// ============================================================

/// Set while the previous byte was the 0xE0 escape prefix.
static EXTENDED: AtomicBool = AtomicBool::new(false);

/// Number of bytes of an 0xE1 (Pause) sequence still to be discarded.
static E1_SKIP: AtomicU8 = AtomicU8::new(0);

/// The first decoded key press plus the scancode it came from, for the
/// one-shot diagnostic in crate::input.
static FIRST_KEY: Mutex<Option<(u8, Key)>> = Mutex::new(None);

/// Set once that diagnostic has been printed.
static FIRST_KEY_REPORTED: AtomicBool = AtomicBool::new(false);

// ============================================================
// Modifier keys
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum Modifier {
    LeftShift,
    RightShift,
    LeftCtrl,
    RightCtrl,
    LeftAlt,
    RightAlt,
    LeftGui,
    RightGui,
    CapsLock,
    NumLock,
    ScrollLock,
}

// ============================================================
// Entry point
// ============================================================

pub fn handle_scancode(byte: u8) {
    if byte == 0xE0 {
        EXTENDED.store(true, Ordering::Release);
        return;
    }

    if byte == 0xE1 {
        // Pause: no Key variant exists, so discard the whole sequence.
        EXTENDED.store(false, Ordering::Release);
        E1_SKIP.store(5, Ordering::Release);
        return;
    }

    let skip = E1_SKIP.load(Ordering::Acquire);

    if skip != 0 {
        E1_SKIP.store(skip - 1, Ordering::Release);
        return;
    }

    let released = (byte & 0x80) != 0;
    let code = byte & 0x7F;
    let extended = EXTENDED.swap(false, Ordering::AcqRel);

    // --------------------------------------------------------
    // Modifier keys update shared state and emit no key event,
    // matching the behaviour of the previous HID decoder.
    // --------------------------------------------------------

    if let Some(modifier) = modifier_for(code, extended) {
        apply_modifier(modifier, !released);
        return;
    }

    let num_lock = crate::input::modifiers().num_lock;

    let Some(key) = key_for_scancode(code, extended, num_lock) else {
        return;
    };

    // Record the first decoded press for the one-shot diagnostic in
    // crate::input. Printing it needs the serial lock and must not happen in
    // interrupt context, so only the value is stored here.
    if !released && !FIRST_KEY_REPORTED.load(Ordering::Acquire) {
        let mut slot = FIRST_KEY.lock();

        if slot.is_none() {
            *slot = Some((byte, key));
        }
    }

    let modifiers = crate::input::modifiers();

    let event = if released {
        InputEvent::KeyUp {
            key,
            modifiers,
        }
    } else {
        InputEvent::KeyDown {
            key,
            modifiers,
        }
    };

    crate::input::push_keyboard_event(event);
}

/// Consumes the first decoded key press, once, for normal-context reporting.
pub fn take_first_key() -> Option<(u8, Key)> {
    let first = FIRST_KEY.lock().take();

    if first.is_some() {
        FIRST_KEY_REPORTED.store(true, Ordering::Release);
    }

    first
}

// ============================================================
// Modifier decoding
// ============================================================

fn modifier_for(code: u8, extended: bool) -> Option<Modifier> {
    Some(match (extended, code) {
        (false, 0x2A) => Modifier::LeftShift,
        (false, 0x36) => Modifier::RightShift,
        (false, 0x1D) => Modifier::LeftCtrl,
        (false, 0x38) => Modifier::LeftAlt,

        (true, 0x1D) => Modifier::RightCtrl,
        (true, 0x38) => Modifier::RightAlt,
        (true, 0x5B) => Modifier::LeftGui,
        (true, 0x5C) => Modifier::RightGui,

        (false, 0x3A) => Modifier::CapsLock,
        (false, 0x45) => Modifier::NumLock,
        (false, 0x46) => Modifier::ScrollLock,

        _ => return None,
    })
}

fn apply_modifier(modifier: Modifier, pressed: bool) {
    crate::input::update_modifiers(|modifiers| {
        match modifier {
            Modifier::LeftShift => modifiers.left_shift = pressed,
            Modifier::RightShift => modifiers.right_shift = pressed,

            Modifier::LeftCtrl | Modifier::RightCtrl => modifiers.ctrl = pressed,
            Modifier::LeftAlt | Modifier::RightAlt => modifiers.alt = pressed,
            Modifier::LeftGui | Modifier::RightGui => modifiers.logo = pressed,

            // Lock keys toggle on press.
            Modifier::CapsLock => {
                if pressed {
                    modifiers.caps_lock = !modifiers.caps_lock;
                }
            }

            Modifier::NumLock => {
                if pressed {
                    modifiers.num_lock = !modifiers.num_lock;
                }
            }

            Modifier::ScrollLock => {
                if pressed {
                    modifiers.scroll_lock = !modifiers.scroll_lock;
                }
            }
        }

        modifiers.shift = modifiers.left_shift || modifiers.right_shift;
    });
}

// ============================================================
// Scancode -> Key
// ============================================================

const DIGITS: [Key; 10] = [
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
    Key::Num9,
    Key::Num0,
];

// Letter keys in physical (QWERTY) order, one array per row. The index is the
// key's position in that row, not its place in the alphabet: scancode 0x10 is
// the Q key and must produce 'q'.
const QWERTY_ROW: [Key; 10] = [
    Key::Q,
    Key::W,
    Key::E,
    Key::R,
    Key::T,
    Key::Y,
    Key::U,
    Key::I,
    Key::O,
    Key::P,
];

const HOME_ROW: [Key; 9] = [
    Key::A,
    Key::S,
    Key::D,
    Key::F,
    Key::G,
    Key::H,
    Key::J,
    Key::K,
    Key::L,
];

const BOTTOM_ROW: [Key; 7] = [
    Key::Z,
    Key::X,
    Key::C,
    Key::V,
    Key::B,
    Key::N,
    Key::M,
];

fn key_for_scancode(code: u8, extended: bool, num_lock: bool) -> Option<Key> {
    if extended {
        return extended_key(code);
    }

    Some(match code {
        0x01 => Key::Escape,

        // Top row digits.
        0x02..=0x0B => DIGITS[(code - 0x02) as usize],

        0x0C => Key::Minus,
        0x0D => Key::Equal,
        0x0E => Key::Backspace,
        0x0F => Key::Tab,

        // QWERTY row.
        0x10..=0x19 => QWERTY_ROW[(code - 0x10) as usize],

        0x1A => Key::LeftBracket,
        0x1B => Key::RightBracket,
        0x1C => Key::Enter,

        // ASDF row.
        0x1E..=0x26 => HOME_ROW[(code - 0x1E) as usize],

        0x27 => Key::Semicolon,
        0x28 => Key::Apostrophe,
        0x29 => Key::Grave,
        0x2B => Key::Backslash,

        // ZXCV row.
        0x2C..=0x32 => BOTTOM_ROW[(code - 0x2C) as usize],

        0x33 => Key::Comma,
        0x34 => Key::Dot,
        0x35 => Key::Slash,

        0x39 => Key::Space,

        // Keypad.
        0x37 => Key::Character('*'),
        0x4A => Key::Minus,
        0x4E => Key::Character('+'),

        0x48 => {
            if num_lock {
                Key::Num8
            } else {
                Key::Up
            }
        }

        0x4B => {
            if num_lock {
                Key::Num4
            } else {
                Key::Left
            }
        }

        0x4D => {
            if num_lock {
                Key::Num6
            } else {
                Key::Right
            }
        }

        0x50 => {
            if num_lock {
                Key::Num2
            } else {
                Key::Down
            }
        }

        0x47 if num_lock => Key::Num7,
        0x49 if num_lock => Key::Num9,
        0x4C if num_lock => Key::Num5,
        0x4F if num_lock => Key::Num1,
        0x51 if num_lock => Key::Num3,
        0x52 if num_lock => Key::Num0,
        0x53 if num_lock => Key::Dot,

        // Function keys, navigation keys and the keypad's non-lock
        // variants have no `Key` counterpart in this kernel yet.
        _ => return None,
    })
}

fn extended_key(code: u8) -> Option<Key> {
    Some(match code {
        0x1C => Key::Enter,
        0x35 => Key::Slash,

        // Navigation cluster and keypad-with-num-lock-off.
        0x48 => Key::Up,
        0x4B => Key::Left,
        0x4D => Key::Right,
        0x50 => Key::Down,

        // Home, End, PageUp, PageDown, Insert, Delete, Menu and both
        // PrintScreen halves are not represented in `Key` yet.
        _ => return None,
    })
}
