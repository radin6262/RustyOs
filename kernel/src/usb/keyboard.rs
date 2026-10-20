// ============================================================
// USB HID boot keyboard decoder
// ============================================================
//
// A boot-protocol keyboard report is eight bytes:
//
//     byte 0     modifier bitmap
//     byte 1     reserved
//     bytes 2-7  currently pressed key usages (HID usage page 0x07)
//
// Unlike PS/2, the report is a snapshot of which keys are held, not a stream
// of make/break codes. The decoder therefore remembers the previous six
// usages and diffs them to produce KeyDown/KeyUp events.
// ============================================================

use crate::input::{InputEvent, Key};

pub const USAGE_ROLLOVER: u8 = 0x01;

const USAGE_CAPS_LOCK: u8 = 0x39;
const USAGE_SCROLL_LOCK: u8 = 0x47;
const USAGE_NUM_LOCK: u8 = 0x53;

const LETTERS: [Key; 26] = [
    Key::A,
    Key::B,
    Key::C,
    Key::D,
    Key::E,
    Key::F,
    Key::G,
    Key::H,
    Key::I,
    Key::J,
    Key::K,
    Key::L,
    Key::M,
    Key::N,
    Key::O,
    Key::P,
    Key::Q,
    Key::R,
    Key::S,
    Key::T,
    Key::U,
    Key::V,
    Key::W,
    Key::X,
    Key::Y,
    Key::Z,
];

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

const KEYPAD_DIGITS: [Key; 10] = [
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

/// Translate a HID keyboard usage to the kernel's `Key`.
///
/// Usages without a `Key` variant (function keys, navigation cluster,
/// keypad operators) are ignored, matching the PS/2 decoder.
pub fn usage_to_key(usage: u8) -> Option<Key> {
    Some(match usage {
        // a-z
        0x04..=0x1D => LETTERS[(usage - 0x04) as usize],

        // 1-9, 0
        0x1E..=0x27 => DIGITS[(usage - 0x1E) as usize],

        0x28 => Key::Enter,
        0x29 => Key::Escape,
        0x2A => Key::Backspace,
        0x2B => Key::Tab,
        0x2C => Key::Space,

        0x2D => Key::Minus,
        0x2E => Key::Equal,
        0x2F => Key::LeftBracket,
        0x30 => Key::RightBracket,
        0x31 => Key::Backslash,
        // 0x32 is the non-US hash key.
        0x33 => Key::Semicolon,
        0x34 => Key::Apostrophe,
        0x35 => Key::Grave,
        0x36 => Key::Comma,
        0x37 => Key::Dot,
        0x38 => Key::Slash,

        0x4F => Key::Right,
        0x50 => Key::Left,
        0x51 => Key::Down,
        0x52 => Key::Up,

        // Keypad 1-9 and 0.
        0x59..=0x61 => KEYPAD_DIGITS[(usage - 0x59) as usize],
        0x62 => Key::Num0,

        _ => return None,
    })
}

/// Decode one boot keyboard report.
///
/// Returns the first newly pressed usage, so the caller can emit a one-shot
/// diagnostic without inspecting input state.
pub fn decode(
    report: &[u8],
    previous_keys: &mut [u8; 6],
    previous_modifiers: &mut u8,
) -> Option<u8> {
    if report.len() < 3 {
        return None;
    }

    let modifiers = report[0];

    update_modifiers(*previous_modifiers, modifiers);
    *previous_modifiers = modifiers;

    // Collect the current usages, dropping duplicates and the error-rollover
    // marker.
    let mut current = [0u8; 6];
    let mut count = 0usize;
    let mut rollover = false;

    let limit = report.len().min(8);

    for index in 2..limit {
        let usage = report[index];

        if usage == USAGE_ROLLOVER {
            rollover = true;
            break;
        }

        if usage == 0 || current[..count].contains(&usage) {
            continue;
        }

        current[count] = usage;
        count += 1;
    }

    if rollover {
        *previous_keys = [0; 6];
        return None;
    }

    // Releases first, so a report that moves from Shift+A to A behaves like
    // two distinct transitions.
    for index in 0..6 {
        let old = previous_keys[index];

        if old == 0 || current[..count].contains(&old) {
            continue;
        }

        if let Some(key) = usage_to_key(old) {
            push_key_event(key, false);
        }
    }

    let mut first_pressed = None;

    for index in 0..count {
        let usage = current[index];

        if previous_keys.contains(&usage) {
            continue;
        }

        if is_lock_key(usage) {
            toggle_lock(usage);
            continue;
        }

        if let Some(key) = usage_to_key(usage) {
            if first_pressed.is_none() {
                first_pressed = Some(usage);
            }

            push_key_event(key, true);
        }
    }

    *previous_keys = current;

    first_pressed
}

/// Apply the modifier bitmap from a boot report to the shared input state.
fn update_modifiers(previous: u8, modifiers: u8) {
    if previous == modifiers {
        return;
    }

    let left_ctrl = modifiers & 0x01 != 0;
    let left_shift = modifiers & 0x02 != 0;
    let left_alt = modifiers & 0x04 != 0;
    let left_gui = modifiers & 0x08 != 0;
    let right_ctrl = modifiers & 0x10 != 0;
    let right_shift = modifiers & 0x20 != 0;
    let right_alt = modifiers & 0x40 != 0;
    let right_gui = modifiers & 0x80 != 0;

    crate::input::update_modifiers(|state| {
        state.left_shift = left_shift;
        state.right_shift = right_shift;
        state.shift = left_shift || right_shift;

        state.ctrl = left_ctrl || right_ctrl;
        state.alt = left_alt || right_alt;
        state.logo = left_gui || right_gui;
    });
}

fn is_lock_key(usage: u8) -> bool {
    usage == USAGE_CAPS_LOCK || usage == USAGE_NUM_LOCK || usage == USAGE_SCROLL_LOCK
}

fn toggle_lock(usage: u8) {
    crate::input::update_modifiers(|state| match usage {
        USAGE_CAPS_LOCK => state.caps_lock = !state.caps_lock,
        USAGE_NUM_LOCK => state.num_lock = !state.num_lock,
        USAGE_SCROLL_LOCK => state.scroll_lock = !state.scroll_lock,
        _ => {}
    });
}

fn push_key_event(key: Key, pressed: bool) {
    let modifiers = crate::input::modifiers();

    let event = if pressed {
        InputEvent::KeyDown {
            key,
            modifiers,
        }
    } else {
        InputEvent::KeyUp {
            key,
            modifiers,
        }
    };

    crate::input::push_keyboard_event(event);
}
