#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseEvent {
    pub buttons: u8,

    pub dx: i8,

    pub dy: i8,

    pub wheel: i8,
}

impl MouseEvent {
    pub const LEFT_BUTTON: u8 =
        1 << 0;

    pub const RIGHT_BUTTON: u8 =
        1 << 1;

    pub const MIDDLE_BUTTON: u8 =
        1 << 2;

    pub const fn left_pressed(
        self,
    ) -> bool {
        self.buttons
            & Self::LEFT_BUTTON
            != 0
    }

    pub const fn right_pressed(
        self,
    ) -> bool {
        self.buttons
            & Self::RIGHT_BUTTON
            != 0
    }

    pub const fn middle_pressed(
        self,
    ) -> bool {
        self.buttons
            & Self::MIDDLE_BUTTON
            != 0
    }
}