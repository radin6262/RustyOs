use super::types::{Key, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardEvent {
    KeyDown {
        key: Key,
        modifiers: Modifiers,
    },

    KeyUp {
        key: Key,
        modifiers: Modifiers,
    },
}

pub struct KeyboardState {
    previous_keys: [u8; 6],
    previous_modifiers: u8,
}

impl KeyboardState {
    pub const fn new() -> Self {
        Self {
            previous_keys: [0; 6],
            previous_modifiers: 0,
        }
    }
}

pub fn parse_report<F>(
    report: &[u8],
    state: &mut KeyboardState,
    mut emit: F,
)
where
    F: FnMut(KeyboardEvent),
{
    if report.len() < 8 {
        return;
    }

    let modifiers = report[0];

    let current_keys = [
        report[2],
        report[3],
        report[4],
        report[5],
        report[6],
        report[7],
    ];

    // --------------------------------------------------------
    // Modifier keys
    // --------------------------------------------------------

    for bit in 0..8 {
        let mask = 1u8 << bit;

        let was_pressed =
            state.previous_modifiers & mask != 0;

        let is_pressed =
            modifiers & mask != 0;

        if was_pressed == is_pressed {
            continue;
        }

        let key = match bit {
            0 => Key::LeftControl,
            1 => Key::LeftShift,
            2 => Key::LeftAlt,
            3 => Key::LeftGui,
            4 => Key::RightControl,
            5 => Key::RightShift,
            6 => Key::RightAlt,
            7 => Key::RightGui,
            _ => unreachable!(),
        };

        let modifier_state =
            Modifiers::from_byte(modifiers);

        if is_pressed {
            emit(
                KeyboardEvent::KeyDown {
                    key,
                    modifiers: modifier_state,
                },
            );
        } else {
            emit(
                KeyboardEvent::KeyUp {
                    key,
                    modifiers: modifier_state,
                },
            );
        }
    }

    // --------------------------------------------------------
    // Newly pressed keys
    // --------------------------------------------------------

    for &usage in &current_keys {
        if usage == 0 {
            continue;
        }

        if !contains(
            &state.previous_keys,
            usage,
        ) {
            emit(
                KeyboardEvent::KeyDown {
                    key: usage_to_key(usage),
                    modifiers: Modifiers::from_byte(
                        modifiers,
                    ),
                },
            );
        }
    }

    // --------------------------------------------------------
    // Released keys
    // --------------------------------------------------------

    for &usage in &state.previous_keys {
        if usage == 0 {
            continue;
        }

        if !contains(
            &current_keys,
            usage,
        ) {
            emit(
                KeyboardEvent::KeyUp {
                    key: usage_to_key(usage),
                    modifiers: Modifiers::from_byte(
                        modifiers,
                    ),
                },
            );
        }
    }

    state.previous_keys = current_keys;
    state.previous_modifiers = modifiers;
}

fn contains(
    keys: &[u8; 6],
    value: u8,
) -> bool {
    keys.iter().any(|&key| key == value)
}

pub fn usage_to_key(
    usage: u8,
) -> Key {
    match usage {
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
        0x33 => Key::Semicolon,
        0x34 => Key::Apostrophe,
        0x35 => Key::Grave,
        0x36 => Key::Comma,
        0x37 => Key::Dot,
        0x38 => Key::Slash,

        0x39 => Key::CapsLock,

        0x3A => Key::F1,
        0x3B => Key::F2,
        0x3C => Key::F3,
        0x3D => Key::F4,
        0x3E => Key::F5,
        0x3F => Key::F6,
        0x40 => Key::F7,
        0x41 => Key::F8,
        0x42 => Key::F9,
        0x43 => Key::F10,
        0x44 => Key::F11,
        0x45 => Key::F12,

        0x46 => Key::PrintScreen,
        0x47 => Key::ScrollLock,
        0x48 => Key::Pause,

        0x49 => Key::Insert,
        0x4A => Key::Home,
        0x4B => Key::PageUp,
        0x4C => Key::Delete,
        0x4D => Key::End,
        0x4E => Key::PageDown,

        0x4F => Key::RightArrow,
        0x50 => Key::LeftArrow,
        0x51 => Key::DownArrow,
        0x52 => Key::UpArrow,

        0x53 => Key::NumLock,

        0xE0 => Key::LeftControl,
        0xE1 => Key::LeftShift,
        0xE2 => Key::LeftAlt,
        0xE3 => Key::LeftGui,
        0xE4 => Key::RightControl,
        0xE5 => Key::RightShift,
        0xE6 => Key::RightAlt,
        0xE7 => Key::RightGui,

        _ => Key::Unknown(usage),
    }
}