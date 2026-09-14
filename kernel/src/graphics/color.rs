#[derive(Clone, Copy)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    pub const WHITE: Color = Color {
        r: 245,
        g: 248,
        b: 255,
        a: 255,
    };

    pub const TEXT_DIM: Color = Color {
        r: 150,
        g: 165,
        b: 185,
        a: 255,
    };

    pub const GREEN: Color = Color {
        r: 100,
        g: 255,
        b: 145,
        a: 255,
    };

    pub const GREEN_DARK: Color = Color {
        r: 35,
        g: 110,
        b: 65,
        a: 255,
    };

    pub const BLUE: Color = Color {
        r: 100,
        g: 165,
        b: 255,
        a: 255,
    };

    pub const CYAN: Color = Color {
        r: 80,
        g: 225,
        b: 255,
        a: 255,
    };

    pub const PURPLE: Color = Color {
        r: 175,
        g: 120,
        b: 255,
        a: 255,
    };

    pub const PANEL: Color = Color {
        r: 20,
        g: 25,
        b: 35,
        a: 255,
    };

    pub const PANEL_LIGHT: Color = Color {
        r: 30,
        g: 37,
        b: 50,
        a: 255,
    };

    pub const BORDER: Color = Color {
        r: 55,
        g: 70,
        b: 95,
        a: 255,
    };

    pub const SHADOW: Color = Color {
        r: 5,
        g: 7,
        b: 12,
        a: 255,
    };

    pub const fn with_alpha(
        self,
        a: u8,
    ) -> Self {
        Self { a, ..self }
    }
}