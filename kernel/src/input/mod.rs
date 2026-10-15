#![allow(dead_code)]

// ============================================================
// Input subsystem
// ============================================================
//
// Hardware backends decode their reports into a single InputEvent queue:
//
//     PS/2 i8042 (IRQ1 / IRQ12)
//           |
//           v
//     key + mouse decoders
//           |
//           v
//     KEYBOARD_EVENTS / MOUSE_EVENTS
//           |
//           +--> compositor (mouse)
//           +--> syscalls   (keyboard)
//
// Everything above this module consumes InputEvent, so swapping the input
// backend does not touch the window manager or userspace.
// ============================================================

pub mod key;
pub mod ps2;

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

static MOUSE_BUTTONS: Mutex<u8> =
    Mutex::new(0);

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
// Initialization
// ============================================================

pub fn init() {
    reset_input_state();

    ps2::init();
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

    *MOUSE_BUTTONS.lock() = 0;

    *MODIFIERS.lock() = Modifiers::default();

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

/// True once the PS/2 controller has been brought up.
pub fn is_initialized() -> bool {
    ps2::is_initialized()
}

// ============================================================
// Legacy keyboard API
// ============================================================

pub fn read_key() -> Option<Key> {
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
    report_first_key();

    KEYBOARD_EVENTS.lock().pop_front()
}

/// One-shot diagnostic: the IRQ handler records the first decoded key press and
/// it is reported from here, because printing needs the serial lock and normal
/// context must be the one to take it. The produced character is included, so a
/// wrong key mapping shows up on the serial log.
fn report_first_key() {
    let Some((scancode, key)) = ps2::keyboard::take_first_key() else {
        return;
    };

    crate::serial::write_str("PS/2: keyboard input, first scancode=");
    crate::serial::write_hex(scancode as u64);

    if let Some(Key::Character(character)) = key_to_character(key, modifiers()) {
        let mut buffer = [0u8; 4];

        crate::serial::write_str(" key=");
        crate::serial::write_str(character.encode_utf8(&mut buffer));
    }

    crate::serial::write_str("\n");
}

// ============================================================
// Mouse event API
// ============================================================

pub fn poll_mouse_event() -> Option<InputEvent> {
    report_first_mouse_packet();

    MOUSE_EVENTS.lock().pop_front()
}

/// One-shot diagnostic mirroring report_first_scancode(). The raw packet is
/// printed so a wrong axis, sync or wheel byte is visible on the serial log.
fn report_first_mouse_packet() {
    let Some((packet, size)) = ps2::mouse::take_first_packet() else {
        return;
    };

    crate::serial::write_str("PS/2: mouse reports input, bytes=");
    crate::serial::write_usize(size);
    crate::serial::write_str(" flags=");
    crate::serial::write_hex(packet[0] as u64);
    crate::serial::write_str(" dx=");
    crate::serial::write_hex((packet[1] as i8 as i64) as u64);
    crate::serial::write_str(" dy=");
    crate::serial::write_hex((packet[2] as i8 as i64) as u64);
    crate::serial::write_str("\n");
}

// ============================================================
// Backend hooks
// ============================================================
//
// Used by crate::input::ps2 to publish decoded input. Keeping the queue
// ownership here means every backend shares one cursor position, one
// modifier state and one button state.
// ============================================================

pub(crate) fn push_keyboard_event(event: InputEvent) {
    let mut queue = KEYBOARD_EVENTS.lock();

    if queue.len() >= EVENT_QUEUE_LIMIT {
        queue.pop_front();
    }

    queue.push_back(event);
}

pub(crate) fn push_mouse_event(event: InputEvent) {
    let mut queue = MOUSE_EVENTS.lock();

    if queue.len() >= EVENT_QUEUE_LIMIT {
        queue.pop_front();
    }

    queue.push_back(event);
}

pub(crate) fn modifiers() -> Modifiers {
    *MODIFIERS.lock()
}

pub(crate) fn update_modifiers<F>(update: F)
where
    F: FnOnce(&mut Modifiers),
{
    let mut state = MODIFIERS.lock();

    update(&mut state);
}

/// Publishes a relative mouse movement.
///
/// MouseMove carries a delta, not a position: the compositor accumulates it
/// into its own cursor coordinates and clamps them to the framebuffer.
pub(crate) fn move_mouse(dx: i32, dy: i32) {
    if dx == 0 && dy == 0 {
        return;
    }

    push_mouse_event(InputEvent::MouseMove {
        x: dx,
        y: dy,
    });
}

/// Publishes every button transition between the previous and the new
/// button bitmask (bit0 left, bit1 right, bit2 middle, bit3/4 extras).
pub(crate) fn set_mouse_buttons(buttons: u8) {
    let previous = {
        let mut state = MOUSE_BUTTONS.lock();

        let previous = *state;
        *state = buttons;

        previous
    };

    if previous == buttons {
        return;
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
}

pub(crate) fn mouse_wheel(delta: i16) {
    if delta == 0 {
        return;
    }

    push_mouse_event(InputEvent::MouseWheel {
        delta,
    });
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
