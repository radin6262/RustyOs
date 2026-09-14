#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,

    Enter,
    Escape,
    Backspace,
    Tab,
    Space,

    Character(char),

    Unknown,
}