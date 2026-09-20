use super::MouseButton;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseEvent {
    Move {
        x: i8,
        y: i8,
    },

    ButtonDown(MouseButton),
    ButtonUp(MouseButton),

    Wheel(i8),
}

pub struct MouseState {
    previous_buttons: u8,
}

impl MouseState {
    pub const fn new() -> Self {
        Self {
            previous_buttons: 0,
        }
    }
}

pub fn parse_report<F>(
    report: &[u8],
    state: &mut MouseState,
    mut emit: F,
)
where
    F: FnMut(MouseEvent),
{
    if report.len() < 4 {
        return;
    }

    let buttons = report[0];
    let x = report[1] as i8;
    let y = report[2] as i8;
    let wheel = report[3] as i8;

    if x != 0 || y != 0 {
        emit(MouseEvent::Move { x, y });
    }

    if wheel != 0 {
        emit(MouseEvent::Wheel(wheel));
    }

    let changed = state.previous_buttons ^ buttons;

    for bit in 0..5 {
        let mask = 1u8 << bit;

        if changed & mask == 0 {
            continue;
        }

        let button = match bit {
            0 => MouseButton::Left,
            1 => MouseButton::Right,
            2 => MouseButton::Middle,
            3 => MouseButton::Button4,
            4 => MouseButton::Button5,
            _ => continue,
        };

        if buttons & mask != 0 {
            emit(MouseEvent::ButtonDown(button));
        } else {
            emit(MouseEvent::ButtonUp(button));
        }
    }

    state.previous_buttons = buttons;
}

pub fn apply_report(
    report: &[u8],
    state: &mut MouseState,
) -> [Option<MouseEvent>; 7] {
    let mut events = [None; 7];
    let mut index = 0;

    parse_report(
        report,
        state,
        |event| {
            if index < events.len() {
                events[index] = Some(event);
                index += 1;
            }
        },
    );

    events
}