pub mod keyboard;
pub mod mouse;
pub mod types;

pub use types::{
    InputEvent,
    Key,
    Modifiers,
    MouseButton,
};

use core::cell::UnsafeCell;

use keyboard::{
    KeyboardEvent,
    KeyboardState,
};

use mouse::{
    MouseEvent,
    MouseState,
};

use crate::serial;

// ============================================================
// Input queue
// ============================================================

const INPUT_QUEUE_SIZE: usize = 256;

struct InputQueue {
    events: [Option<InputEvent>; INPUT_QUEUE_SIZE],
    read: usize,
    write: usize,
}

impl InputQueue {
    const fn new() -> Self {
        Self {
            events: [None; INPUT_QUEUE_SIZE],
            read: 0,
            write: 0,
        }
    }

    fn push(
        &mut self,
        event: InputEvent,
    ) {
        let next =
            (self.write + 1)
                % INPUT_QUEUE_SIZE;

        if next == self.read {
            self.read =
                (self.read + 1)
                    % INPUT_QUEUE_SIZE;
        }

        self.events[self.write] =
            Some(event);

        self.write = next;
    }

    fn pop(
        &mut self,
    ) -> Option<InputEvent> {
        if self.read == self.write {
            return None;
        }

        let event =
            self.events[self.read].take();

        self.read =
            (self.read + 1)
                % INPUT_QUEUE_SIZE;

        event
    }
}

// ============================================================
// Global input state
// ============================================================

struct InputState {
    keyboard: KeyboardState,
    mouse: MouseState,

    keyboard_queue: InputQueue,
    mouse_queue: InputQueue,
}

impl InputState {
    const fn new() -> Self {
        Self {
            keyboard:
            KeyboardState::new(),

            mouse:
            MouseState::new(),

            keyboard_queue:
            InputQueue::new(),

            mouse_queue:
            InputQueue::new(),
        }
    }
}

struct InputStorage {
    state: UnsafeCell<InputState>,
}

unsafe impl Sync for InputStorage {}

static INPUT_STORAGE: InputStorage =
    InputStorage {
        state: UnsafeCell::new(
            InputState::new(),
        ),
    };

// ============================================================
// Internal state access
// ============================================================

unsafe fn state_mut()
    -> &'static mut InputState
{
    unsafe {
        &mut *INPUT_STORAGE
            .state
            .get()
    }
}

// ============================================================
// Initialization
// ============================================================

pub fn init() {
    serial::write_str(
        "INPUT: initializing\n",
    );

    unsafe {
        *INPUT_STORAGE
            .state
            .get() = InputState::new();
    }

    serial::write_str(
        "INPUT: initialization complete\n",
    );
}

// ============================================================
// Keyboard input
// ============================================================

pub fn process_keyboard_report(
    report: &[u8],
) {
    serial::write_str(
        "INPUT KEYBOARD: received report\n",
    );

    if report.len() < 8 {
        serial::write_str(
            "INPUT KEYBOARD: ERROR report too short\n",
        );

        return;
    }

    serial::write_str(
        "INPUT KEYBOARD: report = ",
    );

    for byte in report.iter().take(8) {
        serial::write_str("0x");

        let high =
            (byte >> 4) as usize;

        let low =
            (byte & 0x0F) as usize;

        serial::write_str(
            match high {
                0 => "0",
                1 => "1",
                2 => "2",
                3 => "3",
                4 => "4",
                5 => "5",
                6 => "6",
                7 => "7",
                8 => "8",
                9 => "9",
                10 => "A",
                11 => "B",
                12 => "C",
                13 => "D",
                14 => "E",
                15 => "F",
                _ => "?",
            },
        );

        serial::write_str(
            match low {
                0 => "0",
                1 => "1",
                2 => "2",
                3 => "3",
                4 => "4",
                5 => "5",
                6 => "6",
                7 => "7",
                8 => "8",
                9 => "9",
                10 => "A",
                11 => "B",
                12 => "C",
                13 => "D",
                14 => "E",
                15 => "F",
                _ => "?",
            },
        );

        serial::write_str(" ");
    }

    serial::write_str(
        "\nINPUT KEYBOARD: calling parser\n",
    );

    unsafe {
        let state =
            state_mut();

        let keyboard =
            &mut state.keyboard;

        let keyboard_queue =
            &mut state.keyboard_queue;

        keyboard::parse_report(
            report,
            keyboard,
            |event| {
                match event {
                    KeyboardEvent::KeyDown {
                        key,
                        modifiers,
                    } => {
                        serial::write_str(
                            "INPUT KEYBOARD: KEY DOWN\n",
                        );

                        keyboard_queue.push(
                            InputEvent::KeyDown {
                                key,
                                modifiers,
                            },
                        );

                        serial::write_str(
                            "INPUT KEYBOARD: KEY DOWN queued\n",
                        );
                    }

                    KeyboardEvent::KeyUp {
                        key,
                        modifiers,
                    } => {
                        serial::write_str(
                            "INPUT KEYBOARD: KEY UP\n",
                        );

                        keyboard_queue.push(
                            InputEvent::KeyUp {
                                key,
                                modifiers,
                            },
                        );

                        serial::write_str(
                            "INPUT KEYBOARD: KEY UP queued\n",
                        );
                    }
                }
            },
        );

        serial::write_str(
            "INPUT KEYBOARD: parser returned\n",
        );
    }
}

// ============================================================
// Mouse input
// ============================================================

pub fn process_mouse_report(
    report: &[u8],
) {
    unsafe {
        let state =
            state_mut();

        let mouse =
            &mut state.mouse;

        let mouse_queue =
            &mut state.mouse_queue;

        mouse::parse_report(
            report,
            mouse,
            |event| {
                let input_event =
                    match event {
                        MouseEvent::Move {
                            x,
                            y,
                        } => {
                            InputEvent::MouseMove {
                                x,
                                y,
                            }
                        }

                        MouseEvent::ButtonDown(
                            button,
                        ) => {
                            InputEvent::MouseButtonDown(
                                button,
                            )
                        }

                        MouseEvent::ButtonUp(
                            button,
                        ) => {
                            InputEvent::MouseButtonUp(
                                button,
                            )
                        }

                        MouseEvent::Wheel(
                            delta,
                        ) => {
                            InputEvent::MouseWheel {
                                delta,
                            }
                        }
                    };

                mouse_queue.push(
                    input_event,
                );
            },
        );
    }
}

// ============================================================
// Keyboard event retrieval
// ============================================================

pub fn poll_keyboard_event()
    -> Option<InputEvent>
{
    unsafe {
        state_mut()
            .keyboard_queue
            .pop()
    }
}

// ============================================================
// Mouse event retrieval
// ============================================================

pub fn poll_mouse_event()
    -> Option<InputEvent>
{
    unsafe {
        state_mut()
            .mouse_queue
            .pop()
    }
}