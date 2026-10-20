#![no_std]

// ============================================================
// Desktop chrome
// ============================================================
//
// The compositor dresses the desktop with this module:
//
//     wallpaper     colourful gradient with a soft light source
//     menu bar      translucent bar across the top edge
//     window frame  rounded corners, drop shadow and a title bar
//                   carrying the macOS traffic lights
//     dock          translucent panel pinned to the right edge
//                   with hover labels, like the macOS side dock
//
// Everything here draws into a `Surface`, so the same primitives
// work on the screen and on window pixel buffers.
//
// ============================================================

use alloc::vec;
use alloc::vec::Vec;

use crate::wm::surface::{
    self,
    Surface,
};
use crate::wm::window::Window;

// ============================================================
// Menu bar geometry
// ============================================================

pub const MENU_BAR_HEIGHT: i64 = 26;

/// Blur radius of the frosted glass behind the menu bar.
const MENU_BAR_BLUR: usize =
    12;

const MENU_BAR_TOP_COLOR: u32 =
    0x2C2C35;

const MENU_BAR_BOTTOM_COLOR: u32 =
    0x1C1C23;

const MENU_BAR_ALPHA: u8 =
    0xD8;

const MENU_BAR_HAIRLINE_ALPHA: u8 =
    0x33;

/// Menu titles shown to the right of the logo, macOS style.
const MENU_TITLES: &[&str] = &[
    "File",
    "Edit",
    "View",
    "Window",
    "Help",
];

// ============================================================
// Window frame geometry
// ============================================================

/// Height of the title bar drawn above every window.
pub const TITLE_BAR_HEIGHT: i64 = 30;

const WINDOW_RADIUS: usize =
    10;

const FRAME_COLOR: u32 =
    0x22222A;

const FRAME_ALPHA: u8 =
    0xFF;

const TITLE_BAR_COLOR: u32 =
    0x2E2E37;

const TITLE_BAR_FOCUSED_COLOR: u32 =
    0x34343E;

const TITLE_BAR_ALPHA: u8 =
    0xF2;

const TITLE_BAR_SHEEN_ALPHA: u8 =
    0x12;

const TITLE_BAR_EDGE_ALPHA: u8 =
    0x22;

const FRAME_EDGE_ALPHA: u8 =
    0x2E;

/// macOS traffic lights.
const TRAFFIC_LIGHT_CLOSE: u32 =
    0xFF5F57;

const TRAFFIC_LIGHT_MINIMIZE: u32 =
    0xFEBC2E;

const TRAFFIC_LIGHT_ZOOM: u32 =
    0x28C840;

const TRAFFIC_LIGHT_RADIUS: i64 =
    6;

const TRAFFIC_LIGHT_FIRST_X: i64 =
    18;

const TRAFFIC_LIGHT_SPACING: i64 =
    22;

/// Pointer radius around a traffic light that activates it. A little
/// larger than the lens so the buttons stay easy to hit.
const TRAFFIC_LIGHT_HIT_RADIUS: i64 =
    9;

/// Window controls, in title bar order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrafficLight {
    Close,
    Minimize,
    Zoom,
}

/// Rings of shadow under every window.
const SHADOW_RINGS: usize =
    22;

/// Alpha of the innermost shadow ring; outer rings fade to zero.
const SHADOW_STRENGTH: i64 =
    74;

/// How far the shadow is pushed down from the frame.
const SHADOW_OFFSET_Y: i64 =
    4;

// ============================================================
// Dock geometry
// ============================================================

const DOCK_ITEM_SIZE: usize =
    56;

const DOCK_ITEM_GAP: usize =
    10;

const DOCK_PADDING: usize =
    12;

const DOCK_RADIUS: usize =
    20;

const DOCK_MARGIN: usize =
    18;

const DOCK_PANEL_TOP_COLOR: u32 =
    0x23232B;

const DOCK_PANEL_BOTTOM_COLOR: u32 =
    0x0C0C10;

const DOCK_PANEL_ALPHA: u8 =
    0xBE;

/// Blur radius of the frosted glass behind the dock.
const DOCK_BLUR: usize =
    14;

/// Hover magnification, in percent of the dock item size.
const DOCK_MAGNIFIED: usize =
    114;

const DOCK_NEIGHBOUR: usize =
    106;

const DOCK_SECOND_NEIGHBOUR: usize =
    102;

const DOCK_ICON_RADIUS: usize =
    14;

/// Vertical space taken by the separator above the last item.
const DOCK_SEPARATOR_HEIGHT: usize =
    14;

const DOCK_LABEL_HEIGHT: usize =
    28;

const DOCK_LABEL_ALPHA: u8 =
    0xE6;

// ============================================================
// Dock icons
// ============================================================

#[derive(Clone, Copy)]
pub enum Icon {
    Terminal,
    Applications,
    Folder,
    Photos,
    Music,
    Settings,
    Trash,
}

pub struct DockItem {
    pub label: &'static str,

    /// Base color of the rounded icon tile.
    pub accent: u32,

    pub icon: Icon,
}

/// The part of a window's state the dock visualizes: a dot marks a
/// running app, and a dim dot marks a minimized window that a click
/// will bring back.
pub struct DockWindowState<'a> {
    pub title: &'a str,
    pub minimized: bool,
}

pub static DOCK_ITEMS: &[DockItem] = &[
    DockItem {
        label: "Terminal",
        accent: 0x1F2937,
        icon: Icon::Terminal,
    },
    DockItem {
        label: "Applications",
        accent: 0x0EA5E9,
        icon: Icon::Applications,
    },
    DockItem {
        label: "Files",
        accent: 0x3B82F6,
        icon: Icon::Folder,
    },
    DockItem {
        label: "Photos",
        accent: 0xF5F5F7,
        icon: Icon::Photos,
    },
    DockItem {
        label: "Music",
        accent: 0xEC4899,
        icon: Icon::Music,
    },
    DockItem {
        label: "Settings",
        accent: 0x6B7280,
        icon: Icon::Settings,
    },
    DockItem {
        label: "Trash",
        accent: 0x9CA3AF,
        icon: Icon::Trash,
    },
];

// ============================================================
// Dock layout
// ============================================================

#[derive(Clone, Copy)]
pub struct DockLayout {
    pub x: i64,
    pub y: i64,
    pub width: usize,
    pub height: usize,

    items: usize,

    item_size: usize,

    item_gap: usize,

    padding: usize,

    /// Item index from which the separator adds extra space.
    separator_before: usize,
}

impl DockLayout {
    /// Top-left corner of one dock item.
    pub fn item_origin(
        &self,
        index: usize,
    ) -> (i64, i64) {
        let mut offset =
            self.padding
                + index
                    * (self.item_size + self.item_gap);

        if index >= self.separator_before {
            offset +=
                DOCK_SEPARATOR_HEIGHT;
        }

        (
            self.x + self.padding as i64,
            self.y + offset as i64,
        )
    }

    /// Which dock item covers a screen coordinate, if any.
    pub fn item_at(
        &self,
        x: i64,
        y: i64,
    ) -> Option<usize> {
        for index in 0..self.items {
            let (item_x, item_y) =
                self.item_origin(
                    index,
                );

            if x >= item_x
                && x < item_x + self.item_size as i64
                && y >= item_y
                && y < item_y + self.item_size as i64
            {
                return Some(index);
            }
        }

        None
    }

    pub fn item_size(
        &self,
    ) -> usize {
        self.item_size
    }

    pub fn item_count(
        &self,
    ) -> usize {
        self.items
    }

    /// Vertical center of one dock item.
    pub fn item_center_y(
        &self,
        index: usize,
    ) -> i64 {
        let (_, item_y) =
            self.item_origin(
                index,
            );

        item_y + self.item_size as i64 / 2
    }
}

// ============================================================
// Chrome
// ============================================================

pub struct Chrome {
    wallpaper:
        Vec<u32>,

    screen_width:
        usize,

    screen_height:
        usize,

    /// Scratch buffers for the frosted glass blur. Allocated once so
    /// compositing never touches the kernel allocator.
    blur_a:
        Vec<u32>,

    blur_b:
        Vec<u32>,
}

impl Chrome {
    // ========================================================
    // Construction
    // ========================================================

    pub fn new(
        screen_width: usize,
        screen_height: usize,
    ) -> Self {
        let mut chrome =
            Self {
                wallpaper:
                    build_wallpaper(
                        screen_width,
                        screen_height,
                    ),

                screen_width,

                screen_height,

                blur_a: Vec::new(),

                blur_b: Vec::new(),
            };

        let dock =
            chrome.dock_layout();

        let blur_capacity =
            (screen_width
                * MENU_BAR_HEIGHT as usize)
                .max(
                    dock.width * dock.height,
                )
                + 16;

        chrome.blur_a =
            vec![0u32; blur_capacity];

        chrome.blur_b =
            vec![0u32; blur_capacity];

        crate::serial::write_str(
            "WM: chrome ready menu-bar=",
        );

        crate::serial::write_usize(
            MENU_BAR_HEIGHT as usize,
        );

        crate::serial::write_str(
            " dock=",
        );

        crate::serial::write_usize(
            dock.width,
        );

        crate::serial::write_str("x");

        crate::serial::write_usize(
            dock.height,
        );

        crate::serial::write_str(" at (");

        crate::serial::write_usize(
            dock.x as usize,
        );

        crate::serial::write_str(",");

        crate::serial::write_usize(
            dock.y as usize,
        );

        crate::serial::write_str(") items=");

        crate::serial::write_usize(
            dock.item_count(),
        );

        crate::serial::write_str("\n");

        chrome
    }

    /// The desktop wallpaper, already rendered at screen size.
    pub fn wallpaper(
        &self,
    ) -> &[u32] {
        &self.wallpaper
    }

    // ========================================================
    // Menu bar
    // ========================================================

    pub fn draw_menu_bar(
        &mut self,
        surface: &mut Surface,
        focused_title: &str,
    ) {
        let width =
            surface.width() as i64;

        // Frosted glass: blur whatever is behind the bar first, so
        // windows scrolling underneath read as colour, not detail.
        surface.blur_rounded(
            0,
            0,
            surface.width(),
            MENU_BAR_HEIGHT as usize,
            0,
            MENU_BAR_BLUR,
            &mut self.blur_a,
            &mut self.blur_b,
        );

        // Vertical tint gradient over the blurred backdrop.
        for row in 0..MENU_BAR_HEIGHT {
            let t =
                (row * 255 / MENU_BAR_HEIGHT) as u32;

            let color =
                mix(
                    MENU_BAR_TOP_COLOR,
                    MENU_BAR_BOTTOM_COLOR,
                    t,
                );

            surface.row(
                0,
                row,
                width as usize,
                color,
                MENU_BAR_ALPHA,
            );
        }

        // Top sheen and bottom hairline give the glass an edge.
        surface.row(
            0,
            0,
            width as usize,
            0xFFFFFF,
            0x14,
        );

        surface.row(
            0,
            MENU_BAR_HEIGHT - 1,
            width as usize,
            0xFFFFFF,
            MENU_BAR_HAIRLINE_ALPHA,
        );

        // Logo.
        self.draw_logo(
            surface,
            16,
            MENU_BAR_HEIGHT / 2,
        );

        surface.text(
            36,
            5,
            "RustyOS",
            crate::graphics::Color::WHITE,
            1,
        );

        // Menu titles.
        let mut menu_x =
            36
                + crate::graphics::text_width(
                    "RustyOS",
                    1,
                )
                + 20;

        for title in MENU_TITLES {
            surface.text(
                menu_x,
                5,
                title,
                crate::graphics::Color::TEXT_DIM,
                1,
            );

            menu_x +=
                crate::graphics::text_width(
                    title,
                    1,
                )
                    + 16;
        }

        // Focused application name, centered like the macOS menu bar.
        if !focused_title.is_empty() {
            let title_width =
                crate::graphics::text_width(
                    focused_title,
                    1,
                );

            let title_x =
                (surface.width()
                    .saturating_sub(
                        title_width,
                    ))
                    / 2;

            surface.text(
                title_x,
                5,
                focused_title,
                crate::graphics::Color::TEXT_DIM,
                1,
            );
        }

        // Status glyphs on the right.
        let status_y =
            MENU_BAR_HEIGHT / 2;

        self.draw_search(
            surface,
            surface.width() as i64 - 96,
            status_y,
        );

        self.draw_wifi(
            surface,
            surface.width() as i64 - 66,
            status_y,
        );

        self.draw_battery(
            surface,
            surface.width() as i64 - 44,
            status_y,
        );
    }

    fn draw_search(
        &self,
        surface: &mut Surface,
        center_x: i64,
        center_y: i64,
    ) {
        // Magnifier: a ring plus a diagonal handle.
        surface.round_stroke(
            center_x - 5,
            center_y - 5,
            9,
            9,
            4,
            0xFFFFFF,
            0xB0,
        );

        for step in 0..3 {
            surface.rect(
                center_x + 3 + step,
                center_y + 3 + step,
                2,
                2,
                0xFFFFFF,
                0xA8,
            );
        }
    }

    fn draw_logo(
        &self,
        surface: &mut Surface,
        center_x: i64,
        center_y: i64,
    ) {
        let color =
            0xFFFFFF;

        // Two lobes give the body its dip at the top.
        surface.circle(
            center_x - 3,
            center_y + 3,
            5,
            color,
            0xF0,
        );

        surface.circle(
            center_x + 3,
            center_y + 3,
            5,
            color,
            0xF0,
        );

        // Stem and leaf.
        surface.rect(
            center_x,
            center_y - 6,
            1,
            3,
            color,
            0xF0,
        );

        surface.circle(
            center_x + 3,
            center_y - 6,
            3,
            color,
            0xF0,
        );
    }

    fn draw_wifi(
        &self,
        surface: &mut Surface,
        center_x: i64,
        center_y: i64,
    ) {
        // Three stacked bars read as signal strength.
        for step in 0..3 {
            let width =
                (step + 1) * 3;

            surface.rect(
                center_x - width as i64 / 2,
                center_y + 2 - step as i64 * 3,
                width,
                2,
                0xFFFFFF,
                0xC0,
            );
        }
    }

    fn draw_battery(
        &self,
        surface: &mut Surface,
        center_x: i64,
        center_y: i64,
    ) {
        surface.round_stroke(
            center_x,
            center_y - 5,
            24,
            10,
            3,
            0xFFFFFF,
            0x90,
        );

        surface.round_rect(
            center_x + 3,
            center_y - 2,
            12,
            4,
            1,
            0xFFFFFF,
            0xE0,
        );

        surface.round_rect(
            center_x + 25,
            center_y - 2,
            2,
            4,
            1,
            0xFFFFFF,
            0x80,
        );
    }

    // ========================================================
    // Window frame
    // ========================================================

    /// Screen space taken by a window's frame, which is the content
    /// rect plus the title bar above it.
    pub fn frame_rect(
        window: &Window,
    ) -> (i64, i64, usize, usize) {
        (
            window.x as i64,
            window.y as i64 - TITLE_BAR_HEIGHT,
            window.width,
            window.height + TITLE_BAR_HEIGHT as usize,
        )
    }

    /// How far a window's content must be inset on one screen row so
    /// that it follows the frame's rounded bottom corners.
    pub fn content_row_inset(
        window: &Window,
        screen_y: i64,
    ) -> i64 {
        let (
            _,
            frame_y,
            _,
            frame_height,
        ) =
            Self::frame_rect(
                window,
            );

        let row =
            (screen_y - frame_y).max(0) as usize;

        surface::round_row_inset(
            row,
            frame_height,
            WINDOW_RADIUS,
        ) as i64
    }

    /// Whether a screen pixel belongs to a window's rounded frame.
    ///
    /// This is what rounds the content area's bottom corners: the
    /// compositor skips window pixels that fall outside the frame,
    /// leaving the frame color or the wallpaper underneath.
    pub fn frame_contains(
        window: &Window,
        x: i64,
        y: i64,
    ) -> bool {
        let (
            frame_x,
            frame_y,
            frame_width,
            frame_height,
        ) =
            Self::frame_rect(
                window,
            );

        if x < frame_x
            || y < frame_y
            || x >= frame_x + frame_width as i64
            || y >= frame_y + frame_height as i64
        {
            return false;
        }

        let row =
            (y - frame_y) as usize;

        let inset =
            surface::round_row_inset(
                row,
                frame_height,
                WINDOW_RADIUS,
            ) as i64;

        x >= frame_x + inset
            && x < frame_x + frame_width as i64 - inset
    }

    /// Whether a screen point is inside a window's title bar.
    pub fn title_bar_contains(
        window: &Window,
        x: i64,
        y: i64,
    ) -> bool {
        let (
            frame_x,
            frame_y,
            frame_width,
            _,
        ) =
            Self::frame_rect(
                window,
            );

        x >= frame_x
            && x < frame_x + frame_width as i64
            && y >= frame_y
            && y < frame_y + TITLE_BAR_HEIGHT
    }

    /// Center of one traffic light, in screen coordinates.
    pub fn traffic_light_center(
        window: &Window,
        light: TrafficLight,
    ) -> (i64, i64) {
        let (
            frame_x,
            frame_y,
            _,
            _,
        ) =
            Self::frame_rect(
                window,
            );

        let index =
            match light {
                TrafficLight::Close => 0,
                TrafficLight::Minimize => 1,
                TrafficLight::Zoom => 2,
            } as i64;

        (
            frame_x
                + TRAFFIC_LIGHT_FIRST_X
                + index * TRAFFIC_LIGHT_SPACING,
            frame_y + TITLE_BAR_HEIGHT / 2,
        )
    }

    /// Which traffic light, if any, lies under a screen point.
    pub fn traffic_light_at(
        window: &Window,
        x: i64,
        y: i64,
    ) -> Option<TrafficLight> {
        if !Self::title_bar_contains(
            window,
            x,
            y,
        ) {
            return None;
        }

        for light in [
            TrafficLight::Close,
            TrafficLight::Minimize,
            TrafficLight::Zoom,
        ] {
            let (light_x, light_y) =
                Self::traffic_light_center(
                    window,
                    light,
                );

            let offset_x =
                x - light_x;

            let offset_y =
                y - light_y;

            if offset_x * offset_x
                + offset_y * offset_y
                <= TRAFFIC_LIGHT_HIT_RADIUS
                    * TRAFFIC_LIGHT_HIT_RADIUS
            {
                return Some(light);
            }
        }

        None
    }

    /// Drop shadow and frame background.
    ///
    /// Drawn before the window's content so the compositor can
    /// paint the content on top of it.
    pub fn draw_window_backdrop(
        &self,
        surface: &mut Surface,
        window: &Window,
    ) {
        let (
            frame_x,
            frame_y,
            frame_width,
            frame_height,
        ) =
            Self::frame_rect(
                window,
            );

        // ----------------------------------------------------
        // Shadow.
        //
        // One outline per ring, each thicker than the last: a smooth
        // falloff replaces the hard bands a handful of rings gave.
        // ----------------------------------------------------

        for ring in (1..=SHADOW_RINGS).rev() {
            let grow =
                ring as i64;

            let t =
                ring as i64;

            let alpha =
                SHADOW_STRENGTH
                    * (SHADOW_RINGS as i64 + 1 - t)
                    * (SHADOW_RINGS as i64 + 1 - t)
                    / ((SHADOW_RINGS as i64 + 1)
                        * (SHADOW_RINGS as i64 + 1));

            if alpha <= 0 {
                continue;
            }

            surface.round_stroke(
                frame_x - grow,
                frame_y - grow + SHADOW_OFFSET_Y,
                frame_width + (grow * 2) as usize,
                frame_height + (grow * 2) as usize,
                WINDOW_RADIUS + ring,
                0x000000,
                alpha.min(255) as u8,
            );
        }

        // ----------------------------------------------------
        // Frame background: shows at the rounded corners where
        // the content is masked out.
        // ----------------------------------------------------

        surface.round_rect(
            frame_x,
            frame_y,
            frame_width,
            frame_height,
            WINDOW_RADIUS,
            FRAME_COLOR,
            FRAME_ALPHA,
        );
    }

    /// Title bar, traffic lights, title text and outer edge.
    ///
    /// Drawn after the window's content.
    pub fn draw_window_title_bar(
        &self,
        surface: &mut Surface,
        window: &Window,
        focused: bool,
        hovered_light: Option<TrafficLight>,
        show_controls: bool,
    ) {
        let (
            frame_x,
            frame_y,
            frame_width,
            _,
        ) =
            Self::frame_rect(
                window,
            );

        // ----------------------------------------------------
        // Title bar glass.
        //
        // Rounded at the top only: the content area sits square
        // underneath it.
        // ----------------------------------------------------

        let bar_color =
            if focused {
                TITLE_BAR_FOCUSED_COLOR
            } else {
                TITLE_BAR_COLOR
            };

        surface.top_round_rect(
            frame_x,
            frame_y,
            frame_width,
            TITLE_BAR_HEIGHT as usize,
            WINDOW_RADIUS,
            bar_color,
            TITLE_BAR_ALPHA,
        );

        // Glass sheen: a soft highlight along the top edge that fades
        // over the first few pixels.
        surface.row(
            frame_x + WINDOW_RADIUS as i64,
            frame_y,
            frame_width
                .saturating_sub(2 * WINDOW_RADIUS),
            0xFFFFFF,
            0x22,
        );

        surface.row(
            frame_x + WINDOW_RADIUS as i64,
            frame_y + 1,
            frame_width
                .saturating_sub(2 * WINDOW_RADIUS),
            0xFFFFFF,
            TITLE_BAR_SHEEN_ALPHA,
        );

        // Hairline separating the title bar from the content.
        surface.row(
            frame_x,
            frame_y + TITLE_BAR_HEIGHT - 1,
            frame_width,
            0x000000,
            TITLE_BAR_EDGE_ALPHA,
        );

        // ----------------------------------------------------
        // Traffic lights.
        // ----------------------------------------------------

        let light_y =
            frame_y + TITLE_BAR_HEIGHT / 2;

        let light_alpha =
            if focused {
                0xFF
            } else {
                0xB4
            };

        for (index, color) in [
            TRAFFIC_LIGHT_CLOSE,
            TRAFFIC_LIGHT_MINIMIZE,
            TRAFFIC_LIGHT_ZOOM,
        ]
            .into_iter()
            .enumerate()
        {
            let light_x =
                frame_x
                    + TRAFFIC_LIGHT_FIRST_X
                    + index as i64 * TRAFFIC_LIGHT_SPACING;

            // Dark rim for definition against the title bar.
            surface.circle(
                light_x,
                light_y,
                TRAFFIC_LIGHT_RADIUS + 1,
                0x000000,
                0x30,
            );

            surface.circle(
                light_x,
                light_y,
                TRAFFIC_LIGHT_RADIUS,
                color,
                light_alpha,
            );

            // Specular highlight, upper left of each lens.
            surface.circle(
                light_x - 2,
                light_y - 2,
                2,
                0xFFFFFF,
                0x48,
            );

            // macOS only reveals the glyphs while the pointer is over
            // the title bar. The hovered lens gets a touch more light
            // and a fully opaque symbol.
            if show_controls {
                let is_hovered =
                    hovered_light
                        == Some(
                            match index {
                                0 => TrafficLight::Close,
                                1 => TrafficLight::Minimize,
                                _ => TrafficLight::Zoom,
                            },
                        );

                if is_hovered {
                    surface.circle(
                        light_x,
                        light_y,
                        TRAFFIC_LIGHT_RADIUS,
                        0xFFFFFF,
                        0x26,
                    );
                }

                let glyph_alpha =
                    if is_hovered {
                        0xFF
                    } else {
                        0xB8
                    };

                match index {
                    // Close: a compact cross.
                    0 => {
                        let glyph =
                            0x5A0F0B;

                        surface.line(
                            light_x - 3,
                            light_y - 3,
                            light_x + 3,
                            light_y + 3,
                            glyph,
                            glyph_alpha,
                        );

                        surface.line(
                            light_x - 3,
                            light_y + 3,
                            light_x + 3,
                            light_y - 3,
                            glyph,
                            glyph_alpha,
                        );
                    }

                    // Minimize: a single horizontal bar.
                    1 => {
                        surface.line(
                            light_x - 3,
                            light_y,
                            light_x + 3,
                            light_y,
                            0x6B4900,
                            glyph_alpha,
                        );
                    }

                    // Zoom: a plus.
                    _ => {
                        let glyph =
                            0x0B3F0F;

                        surface.line(
                            light_x - 3,
                            light_y,
                            light_x + 3,
                            light_y,
                            glyph,
                            glyph_alpha,
                        );

                        surface.line(
                            light_x,
                            light_y - 3,
                            light_x,
                            light_y + 3,
                            glyph,
                            glyph_alpha,
                        );
                    }
                }
            }
        }

        // ----------------------------------------------------
        // Title.
        // ----------------------------------------------------

        let title =
            if window.title.is_empty() {
                "RustyOS"
            } else {
                window.title()
            };

        let title_width =
            crate::graphics::text_width(
                title,
                1,
            );

        let title_x =
            frame_x
                + (frame_width
                    .saturating_sub(
                        title_width,
                    )) as i64
                    / 2;

        surface.text(
            title_x.max(frame_x + 80) as usize,
            (frame_y + 6) as usize,
            title,
            if focused {
                crate::graphics::Color::WHITE
            } else {
                crate::graphics::Color::TEXT_DIM
            },
            1,
        );

        // ----------------------------------------------------
        // Outer edge.
        // ----------------------------------------------------

        surface.round_stroke(
            frame_x,
            frame_y,
            frame_width,
            window.height
                + TITLE_BAR_HEIGHT as usize,
            WINDOW_RADIUS,
            0xFFFFFF,
            FRAME_EDGE_ALPHA,
        );
    }

    // ========================================================
    // Dock
    // ========================================================

    pub fn dock_layout(
        &self,
    ) -> DockLayout {
        let items =
            DOCK_ITEMS.len();

        let separator_before =
            items.saturating_sub(
                1,
            );

        // ----------------------------------------------------
        // Shrink the icons when the panel would not fit.
        // ----------------------------------------------------

        let available =
            self.screen_height
                .saturating_sub(
                    MENU_BAR_HEIGHT as usize
                        + 2 * DOCK_MARGIN,
                );

        let mut item_size =
            DOCK_ITEM_SIZE;

        let mut item_gap =
            DOCK_ITEM_GAP;

        let mut padding =
            DOCK_PADDING;

        let needed =
            items * item_size
                + items.saturating_sub(1) * item_gap
                + DOCK_SEPARATOR_HEIGHT
                + 2 * padding;

        if needed > available {
            let usable =
                available
                    .saturating_sub(
                        DOCK_SEPARATOR_HEIGHT,
                    );

            let scale =
                usable
                    / (items * item_size
                        + items.saturating_sub(1) * item_gap)
                        .max(1);

            item_size =
                (item_size * scale)
                    .max(24);

            item_gap =
                (item_gap * scale)
                    .max(2);

            padding =
                (padding * scale)
                    .max(6);
        }

        let height =
            items * item_size
                + items.saturating_sub(1) * item_gap
                + DOCK_SEPARATOR_HEIGHT
                + 2 * padding;

        let width =
            item_size
                + 2 * padding;

        let x =
            self.screen_width as i64
                - DOCK_MARGIN as i64
                - width as i64;

        let y =
            (self.screen_height as i64
                - height as i64)
                / 2;

        DockLayout {
            x,
            y,
            width,
            height,

            items,

            item_size,

            item_gap,

            padding,

            separator_before,
        }
    }

    /// Where a dock item sits, which is what a hover label points
    /// at and what future click handling will use.
    pub fn dock_item_at(
        &self,
        x: i64,
        y: i64,
    ) -> Option<usize> {
        self.dock_layout()
            .item_at(
                x,
                y,
            )
    }

    /// Whether a screen pixel is inside the dock panel.
    pub fn dock_contains(
        &self,
        x: i64,
        y: i64,
    ) -> bool {
        let layout =
            self.dock_layout();

        x >= layout.x
            && x < layout.x + layout.width as i64
            && y >= layout.y
            && y < layout.y + layout.height as i64
    }

    pub fn draw_dock(
        &mut self,
        surface: &mut Surface,
        mouse_x: i64,
        mouse_y: i64,
        windows: &[DockWindowState<'_>],
    ) {
        let layout =
            self.dock_layout();

        let hovered =
            layout.item_at(
                mouse_x,
                mouse_y,
            );

        // ----------------------------------------------------
        // Frosted glass backdrop.
        // ----------------------------------------------------

        surface.blur_rounded(
            layout.x,
            layout.y,
            layout.width,
            layout.height,
            DOCK_RADIUS,
            DOCK_BLUR,
            &mut self.blur_a,
            &mut self.blur_b,
        );

        // ----------------------------------------------------
        // Panel shadow.
        // ----------------------------------------------------

        for ring in (1..=SHADOW_RINGS).rev() {
            let grow =
                ring as i64;

            let alpha =
                (26u8).saturating_sub(
                    (ring as u8) * 3,
                );

            surface.round_stroke(
                layout.x - grow,
                layout.y - grow,
                layout.width + (grow * 2) as usize,
                layout.height + (grow * 2) as usize,
                DOCK_RADIUS + ring,
                0x000000,
                alpha,
            );
        }

        // ----------------------------------------------------
        // Glass panel: vertical tint over the blurred backdrop.
        // ----------------------------------------------------

        for row in 0..layout.height {
            let inset =
                surface::round_row_inset(
                    row,
                    layout.height,
                    DOCK_RADIUS,
                ) as i64;

            let t =
                (row * 255
                    / layout.height.max(1)) as u32;

            let color =
                mix(
                    DOCK_PANEL_TOP_COLOR,
                    DOCK_PANEL_BOTTOM_COLOR,
                    t,
                );

            surface.row(
                layout.x + inset,
                layout.y + row as i64,
                layout.width
                    - 2 * inset as usize,
                color,
                DOCK_PANEL_ALPHA,
            );
        }

        // Top sheen: a bright rim that sells the glass.
        for row in 0..2 {
            let inset =
                surface::round_row_inset(
                    row,
                    layout.height,
                    DOCK_RADIUS,
                ) as i64;

            surface.row(
                layout.x + inset,
                layout.y + row as i64,
                layout.width
                    - 2 * inset as usize,
                0xFFFFFF,
                if row == 0 {
                    0x22
                } else {
                    0x10
                },
            );
        }

        surface.round_stroke(
            layout.x,
            layout.y,
            layout.width,
            layout.height,
            DOCK_RADIUS,
            0xFFFFFF,
            0x24,
        );

        // ----------------------------------------------------
        // Items.
        // ----------------------------------------------------

        for (index, item)
        in DOCK_ITEMS.iter().enumerate()
        {
            let (
                item_x,
                item_y,
            ) =
                layout.item_origin(
                    index,
                );

            // Separator above the last item, like the macOS
            // divider in front of the trash.
            if index == layout.separator_before {
                surface.row(
                    layout.x + layout.padding as i64,
                    item_y
                        - DOCK_SEPARATOR_HEIGHT as i64 / 2,
                    layout.width
                        - 2 * layout.padding,
                    0xFFFFFF,
                    0x26,
                );
            }

            if hovered == Some(index) {
                surface.round_rect(
                    item_x - 4,
                    item_y - 4,
                    layout.item_size + 8,
                    layout.item_size + 8,
                    DOCK_ICON_RADIUS + 4,
                    0xFFFFFF,
                    0x1C,
                );
            }

            // macOS style magnification: the hovered item grows and
            // its neighbours follow with a smaller step.
            let scale =
                match hovered {
                    Some(active) => {
                        let distance =
                            index.abs_diff(active);

                        match distance {
                            0 => DOCK_MAGNIFIED,

                            1 => DOCK_NEIGHBOUR,

                            2 => DOCK_SECOND_NEIGHBOUR,

                            _ => 100,
                        }
                    }

                    None => 100,
                };

            let scaled_size =
                (layout.item_size * scale / 100)
                    .max(1);

            // Magnified items grow outwards from their layout slot.
            let grow =
                (scaled_size
                    .saturating_sub(layout.item_size))
                    / 2;

            draw_icon(
                surface,
                item,
                item_x - grow as i64,
                item_y - grow as i64,
                scaled_size,
            );

            // Running indicator, macOS style: solid for a visible
            // window, dim for one that is minimized and can be
            // restored with a click.
            let state =
                windows.iter().find(|window| {
                    window
                        .title
                        .eq_ignore_ascii_case(item.label)
                });

            if let Some(state) = state {
                let indicator_x =
                    item_x + layout.item_size as i64 / 2;

                // Sit in the gap under the tile, like the running
                // dot of a macOS dock icon.
                let indicator_y =
                    item_y
                        + layout.item_size as i64
                        + DOCK_ITEM_GAP as i64 / 2;

                surface.circle(
                    indicator_x,
                    indicator_y,
                    3,
                    0x000000,
                    0x60,
                );

                surface.circle(
                    indicator_x,
                    indicator_y,
                    2,
                    0xFFFFFF,
                    if state.minimized {
                        0xC0
                    } else {
                        0xE0
                    },
                );

                // A hollow dot means minimized: the app is running
                // but has no window on screen.
                if state.minimized {
                    surface.circle(
                        indicator_x,
                        indicator_y,
                        1,
                        0x000000,
                        0xD0,
                    );
                }
            }
        }

        // ----------------------------------------------------
        // Hover label, drawn beside the panel so it is readable
        // over the wallpaper.
        // ----------------------------------------------------

        if let Some(index) = hovered {
            let label =
                DOCK_ITEMS[index].label;

            let text_width =
                crate::graphics::text_width(
                    label,
                    1,
                );

            let label_width =
                text_width + 24;

            let label_x =
                layout.x
                    - label_width as i64
                    - 12;

            let label_y =
                layout.item_center_y(
                    index,
                )
                    - DOCK_LABEL_HEIGHT as i64 / 2;

            surface.round_rect(
                label_x,
                label_y,
                label_width,
                DOCK_LABEL_HEIGHT,
                DOCK_LABEL_HEIGHT / 2,
                0x0B0B0E,
                DOCK_LABEL_ALPHA,
            );

            surface.text(
                (label_x + 12).max(0) as usize,
                (label_y + 6) as usize,
                label,
                crate::graphics::Color::WHITE,
                1,
            );
        }
    }
}

// ============================================================
// Dock icons
// ============================================================

fn draw_icon(
    surface: &mut Surface,
    item: &DockItem,
    x: i64,
    y: i64,
    size: usize,
) {
    let radius =
        DOCK_ICON_RADIUS
            .min(
                size / 4,
            );

    // Tile with a subtle vertical sheen.
    for row in 0..size {
        let brightness =
            26u32
                * (size - row) as u32
                / size.max(1) as u32;

        let color =
            lighten(
                item.accent,
                brightness,
            );

        let inset =
            surface::round_row_inset(
                row,
                size,
                radius,
            ) as i64;

        surface.row(
            x + inset,
            y + row as i64,
            size
                - 2 * inset as usize,
            color,
            0xFF,
        );
    }

    surface.round_stroke(
        x,
        y,
        size,
        size,
        radius,
        0xFFFFFF,
        0x24,
    );

    let center_x =
        x + size as i64 / 2;

    let center_y =
        y + size as i64 / 2;

    let white =
        0xFFFFFF;

    let unit =
        (size / 8).max(3) as i64;

    match item.icon {
        Icon::Terminal => {
            // A tiny terminal screen with a centred prompt.
            let screen_width =
                (unit * 4) as usize;

            let screen_height =
                (unit * 3) as usize;

            surface.round_stroke(
                center_x - screen_width as i64 / 2,
                center_y - screen_height as i64 / 2,
                screen_width,
                screen_height,
                (unit / 2) as usize,
                white,
                0xB0,
            );

            let glyph_width =
                crate::graphics::text_width(
                    ">_",
                    1,
                ) as i64;

            surface.text(
                (center_x - glyph_width / 2) as usize,
                (center_y - 7) as usize,
                ">_",
                crate::graphics::Color::WHITE,
                1,
            );
        }

        Icon::Applications => {
            let tile =
                unit * 2;

            for column in 0..2 {
                for row in 0..2 {
                    surface.round_rect(
                        center_x
                            - tile
                            - unit / 2
                            + column as i64 * (tile + unit),
                        center_y
                            - tile
                            - unit / 2
                            + row as i64 * (tile + unit),
                        tile as usize,
                        tile as usize,
                        (tile / 3) as usize,
                        white,
                        0xE6,
                    );
                }
            }
        }

        Icon::Folder => {
            let width =
                unit * 4;

            let height =
                unit * 3;

            surface.round_rect(
                center_x - width / 2,
                center_y - height / 2 + unit / 2,
                width as usize,
                height as usize,
                (unit / 2) as usize,
                white,
                0xE6,
            );

            surface.round_rect(
                center_x - width / 2,
                center_y - height / 2 - unit / 2,
                (width * 2 / 3) as usize,
                (unit * 2) as usize,
                (unit / 2) as usize,
                white,
                0xE6,
            );
        }

        Icon::Photos => {
            let petals = [
                0xFBBF24,
                0xF87171,
                0x60A5FA,
                0x34D399,
            ];

            let offsets: [(i64, i64); 4] = [
                (0, -unit),
                (unit, 0),
                (0, unit),
                (-unit, 0),
            ];

            for (index, (offset_x, offset_y))
            in offsets.into_iter().enumerate()
            {
                surface.circle(
                    center_x + offset_x,
                    center_y + offset_y,
                    unit,
                    petals[index],
                    0xF0,
                );
            }

            surface.circle(
                center_x,
                center_y,
                unit - 2,
                white,
                0x60,
            );
        }

        Icon::Music => {
            surface.circle(
                center_x - unit,
                center_y + unit,
                unit,
                white,
                0xE6,
            );

            surface.circle(
                center_x + unit,
                center_y - unit / 2,
                unit,
                white,
                0xE6,
            );

            surface.rect(
                center_x - unit,
                center_y - unit * 3,
                (unit / 2).max(2) as usize,
                (unit * 4) as usize,
                white,
                0xE6,
            );

            surface.rect(
                center_x + unit,
                center_y - unit * 4,
                (unit / 2).max(2) as usize,
                (unit * 4) as usize,
                white,
                0xE6,
            );

            surface.rect(
                center_x - unit,
                center_y - unit * 3,
                (unit * 2) as usize,
                (unit / 2).max(2) as usize,
                white,
                0xE6,
            );
        }

        Icon::Settings => {
            surface.circle(
                center_x,
                center_y,
                unit,
                white,
                0xE6,
            );

            surface.circle(
                center_x,
                center_y,
                unit * 2 / 3,
                item.accent,
                0xFF,
            );

            // Gear teeth: four orthogonal, four diagonal.
            let reach =
                unit * 2;

            let diagonal =
                unit * 3 / 2;

            for tooth in 0..8 {
                let (offset_x, offset_y) =
                    match tooth {
                        0 => (0, -reach),
                        1 => (diagonal, -diagonal),
                        2 => (reach, 0),
                        3 => (diagonal, diagonal),
                        4 => (0, reach),
                        5 => (-diagonal, -diagonal),
                        6 => (-reach, 0),
                        _ => (-diagonal, diagonal),
                    };

                surface.circle(
                    center_x + offset_x,
                    center_y + offset_y,
                    (unit / 2).max(2),
                    white,
                    0xE6,
                );
            }
        }

        Icon::Trash => {
            let width =
                unit * 3;

            let height =
                unit * 4;

            surface.round_rect(
                center_x - width / 2,
                center_y - height / 2 + unit,
                width as usize,
                height as usize,
                (unit / 2) as usize,
                0xE5E7EB,
                0xF0,
            );

            surface.rect(
                center_x - width / 2 - unit / 2,
                center_y - height / 2,
                (width + unit) as usize,
                (unit / 2).max(2) as usize,
                0xE5E7EB,
                0xF0,
            );

            surface.rect(
                center_x - unit / 2,
                center_y - height / 2 - unit / 2,
                unit as usize,
                (unit / 2).max(2) as usize,
                0xE5E7EB,
                0xF0,
            );

            // Two ribs.
            for rib in [-1i64, 1] {
                surface.rect(
                    center_x + rib * unit / 2 - 1,
                    center_y - height / 2 + unit * 2,
                    2,
                    (height - unit * 2) as usize,
                    0x9CA3AF,
                    0xC0,
                );
            }
        }
    }
}

// ============================================================
// Helpers
// ============================================================

/// Linear interpolation between two packed colors, `t` from 0 to 255.
fn mix(
    start: u32,
    end: u32,
    t: u32,
) -> u32 {
    let t =
        t.min(255);

    let lerp =
        |start: u32, end: u32| -> u32 {
            (start as i64
                + (end as i64 - start as i64) * t as i64 / 255)
                .clamp(0, 255) as u32
        };

    surface::argb(
        lerp(
            (start >> 16) & 0xFF,
            (end >> 16) & 0xFF,
        ),
        lerp(
            (start >> 8) & 0xFF,
            (end >> 8) & 0xFF,
        ),
        lerp(
            start & 0xFF,
            end & 0xFF,
        ),
    )
}

/// Add a fixed amount of white to a color, clamped per channel.
fn lighten(
    color: u32,
    amount: u32,
) -> u32 {
    let red =
        (((color >> 16) & 0xFF) + amount).min(255);

    let green =
        (((color >> 8) & 0xFF) + amount).min(255);

    let blue =
        ((color & 0xFF) + amount).min(255);

    surface::argb(
        red,
        green,
        blue,
    )
}

// ============================================================
// Wallpaper
// ============================================================
//
// A macOS style gradient: deep indigo at the bottom left running
// through violet and magenta into a warm light near the top
// right, with a soft glow where the light source sits.
//
// ============================================================

// ============================================================
// Wallpaper
// ============================================================
//
// A deep night sky with a few soft light sources: indigo in the
// lower left, magenta top right, a teal wash along the bottom and
// a small warm glow in the corner. Every blob is additive and
// smooth, so the result looks like a lit room rather than a
// striped gradient. A one bit dither hides 8-bit banding.

struct WallpaperBlob {
    center_x_percent: i64,
    center_y_percent: i64,
    radius_percent: i64,
    red: i64,
    green: i64,
    blue: i64,
    strength: i64,
}

const WALLPAPER_BLOBS: &[WallpaperBlob] = &[
    WallpaperBlob {
        center_x_percent: 14,
        center_y_percent: 82,
        radius_percent: 62,
        red: 84,
        green: 58,
        blue: 196,
        strength: 82,
    },
    WallpaperBlob {
        center_x_percent: 86,
        center_y_percent: 20,
        radius_percent: 48,
        red: 206,
        green: 72,
        blue: 132,
        strength: 66,
    },
    WallpaperBlob {
        center_x_percent: 70,
        center_y_percent: 96,
        radius_percent: 44,
        red: 26,
        green: 136,
        blue: 164,
        strength: 36,
    },
    WallpaperBlob {
        center_x_percent: 98,
        center_y_percent: 4,
        radius_percent: 26,
        red: 255,
        green: 174,
        blue: 118,
        strength: 44,
    },
];

fn build_wallpaper(
    width: usize,
    height: usize,
) -> Vec<u32> {
    let mut wallpaper =
        vec![
            0u32;
            width
                .saturating_mul(
                    height,
                )
        ];

    if width == 0
        || height == 0
    {
        return wallpaper;
    }

    let span =
        width.max(height) as i64;

    let half_width =
        (width as i64) / 2;

    let half_height =
        (height as i64) / 2;

    // Precompute blob geometry once, so the inner loop is only
    // arithmetic on the current pixel.
    let mut blob_geometry =
        [(0i64, 0i64, 1i64); WALLPAPER_BLOBS.len()];

    for (index, blob) in
        WALLPAPER_BLOBS.iter().enumerate()
    {
        blob_geometry[index] = (
            width as i64
                * blob.center_x_percent
                / 100,
            height as i64
                * blob.center_y_percent
                / 100,
            (span
                * blob.radius_percent
                / 100)
                .max(1),
        );
    }

    for y in 0..height {
        for x in 0..width {
            // Base: a diagonal wash from deep blue to near black.
            let diagonal =
                ((x * 128 / width)
                    + ((height - 1 - y) * 127 / height))
                    as u32;

            let base =
                mix(
                    0x10142A,
                    0x07080F,
                    diagonal,
                );

            let mut red =
                ((base >> 16) & 0xFF) as i64;

            let mut green =
                ((base >> 8) & 0xFF) as i64;

            let mut blue =
                (base & 0xFF) as i64;

            // Additive light blobs.
            for (index, blob) in
                WALLPAPER_BLOBS.iter().enumerate()
            {
                let (center_x, center_y, radius) =
                    blob_geometry[index];

                let dx =
                    x as i64 - center_x;

                let dy =
                    y as i64 - center_y;

                let distance_squared =
                    dx * dx + dy * dy;

                let radius_squared =
                    radius * radius;

                if distance_squared >= radius_squared {
                    continue;
                }

                // Smooth falloff: quadratic in the normalized
                // distance, zero at the edge of the blob.
                let falloff =
                    (radius_squared - distance_squared)
                        * 256
                        / radius_squared;

                let falloff =
                    falloff * falloff / 256;

                let amount =
                    falloff * blob.strength / 256;

                red += blob.red * amount / 100;
                green += blob.green * amount / 100;
                blue += blob.blue * amount / 100;
            }

            // Vignette: darken the corners so the centre reads as
            // the focal point.
            let dx =
                (x as i64 - half_width) * 256
                    / half_width.max(1);

            let dy =
                (y as i64 - half_height) * 256
                    / half_height.max(1);

            let vignette =
                256
                    - (dx * dx + dy * dy)
                        .min(512 * 512)
                        * 64
                        / (512 * 512);

            red = red * vignette / 256;
            green = green * vignette / 256;
            blue = blue * vignette / 256;

            // Ordered hash dither, ±1 per channel.
            let noise =
                (((x as u32)
                    .wrapping_mul(0x9E37_79B1)
                    ^ (y as u32)
                        .wrapping_mul(0x85EB_CA6B))
                    >> 13
                    & 3) as i64
                    - 2;

            wallpaper[y * width + x] =
                surface::argb(
                    (red + noise).clamp(0, 255) as u32,
                    (green + noise).clamp(0, 255) as u32,
                    (blue + noise).clamp(0, 255) as u32,
                );
        }
    }

    wallpaper
}
