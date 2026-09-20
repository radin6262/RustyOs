#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,

    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Num0,

    Enter,
    Escape,
    Backspace,
    Tab,
    Space,

    Minus,
    Equal,
    LeftBracket,
    RightBracket,
    Backslash,
    Semicolon,
    Apostrophe,
    Grave,
    Comma,
    Dot,
    Slash,

    CapsLock,

    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,

    PrintScreen,
    ScrollLock,
    Pause,

    Insert,
    Home,
    PageUp,
    Delete,
    End,
    PageDown,

    RightArrow,
    LeftArrow,
    DownArrow,
    UpArrow,

    NumLock,

    LeftControl,
    LeftShift,
    LeftAlt,
    LeftGui,

    RightControl,
    RightShift,
    RightAlt,
    RightGui,

    Unknown(u8),
}

impl Key {
    pub const fn usage(self) -> u8 {
        match self {
            Key::A => 0x04,
            Key::B => 0x05,
            Key::C => 0x06,
            Key::D => 0x07,
            Key::E => 0x08,
            Key::F => 0x09,
            Key::G => 0x0A,
            Key::H => 0x0B,
            Key::I => 0x0C,
            Key::J => 0x0D,
            Key::K => 0x0E,
            Key::L => 0x0F,
            Key::M => 0x10,
            Key::N => 0x11,
            Key::O => 0x12,
            Key::P => 0x13,
            Key::Q => 0x14,
            Key::R => 0x15,
            Key::S => 0x16,
            Key::T => 0x17,
            Key::U => 0x18,
            Key::V => 0x19,
            Key::W => 0x1A,
            Key::X => 0x1B,
            Key::Y => 0x1C,
            Key::Z => 0x1D,

            Key::Num1 => 0x1E,
            Key::Num2 => 0x1F,
            Key::Num3 => 0x20,
            Key::Num4 => 0x21,
            Key::Num5 => 0x22,
            Key::Num6 => 0x23,
            Key::Num7 => 0x24,
            Key::Num8 => 0x25,
            Key::Num9 => 0x26,
            Key::Num0 => 0x27,

            Key::Enter => 0x28,
            Key::Escape => 0x29,
            Key::Backspace => 0x2A,
            Key::Tab => 0x2B,
            Key::Space => 0x2C,

            Key::Minus => 0x2D,
            Key::Equal => 0x2E,
            Key::LeftBracket => 0x2F,
            Key::RightBracket => 0x30,
            Key::Backslash => 0x31,
            Key::Semicolon => 0x33,
            Key::Apostrophe => 0x34,
            Key::Grave => 0x35,
            Key::Comma => 0x36,
            Key::Dot => 0x37,
            Key::Slash => 0x38,

            Key::CapsLock => 0x39,

            Key::F1 => 0x3A,
            Key::F2 => 0x3B,
            Key::F3 => 0x3C,
            Key::F4 => 0x3D,
            Key::F5 => 0x3E,
            Key::F6 => 0x3F,
            Key::F7 => 0x40,
            Key::F8 => 0x41,
            Key::F9 => 0x42,
            Key::F10 => 0x43,
            Key::F11 => 0x44,
            Key::F12 => 0x45,

            Key::PrintScreen => 0x46,
            Key::ScrollLock => 0x47,
            Key::Pause => 0x48,

            Key::Insert => 0x49,
            Key::Home => 0x4A,
            Key::PageUp => 0x4B,
            Key::Delete => 0x4C,
            Key::End => 0x4D,
            Key::PageDown => 0x4E,

            Key::RightArrow => 0x4F,
            Key::LeftArrow => 0x50,
            Key::DownArrow => 0x51,
            Key::UpArrow => 0x52,

            Key::NumLock => 0x53,

            Key::LeftControl => 0xE0,
            Key::LeftShift => 0xE1,
            Key::LeftAlt => 0xE2,
            Key::LeftGui => 0xE3,
            Key::RightControl => 0xE4,
            Key::RightShift => 0xE5,
            Key::RightAlt => 0xE6,
            Key::RightGui => 0xE7,

            Key::Unknown(usage) => usage,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modifiers {
    pub left_ctrl: bool,
    pub left_shift: bool,
    pub left_alt: bool,
    pub left_gui: bool,

    pub right_ctrl: bool,
    pub right_shift: bool,
    pub right_alt: bool,
    pub right_gui: bool,
}

impl Modifiers {
    pub const fn empty() -> Self {
        Self {
            left_ctrl: false,
            left_shift: false,
            left_alt: false,
            left_gui: false,

            right_ctrl: false,
            right_shift: false,
            right_alt: false,
            right_gui: false,
        }
    }

    pub const fn from_byte(value: u8) -> Self {
        Self {
            left_ctrl: value & 0x01 != 0,
            left_shift: value & 0x02 != 0,
            left_alt: value & 0x04 != 0,
            left_gui: value & 0x08 != 0,

            right_ctrl: value & 0x10 != 0,
            right_shift: value & 0x20 != 0,
            right_alt: value & 0x40 != 0,
            right_gui: value & 0x80 != 0,
        }
    }

    pub const fn bits(self) -> u8 {
        (self.left_ctrl as u8)
            | ((self.left_shift as u8) << 1)
            | ((self.left_alt as u8) << 2)
            | ((self.left_gui as u8) << 3)
            | ((self.right_ctrl as u8) << 4)
            | ((self.right_shift as u8) << 5)
            | ((self.right_alt as u8) << 6)
            | ((self.right_gui as u8) << 7)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Button4,
    Button5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
        x: i8,
        y: i8,
    },

    MouseButtonDown(MouseButton),

    MouseButtonUp(MouseButton),

    MouseWheel {
        delta: i8,
    },
}