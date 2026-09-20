#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use spin::Mutex;

use super::window::Window;

pub static WM: Mutex<Option<Compositor>> =
    Mutex::new(None);

// ============================================================
// Cursor configuration
// ============================================================

const CURSOR_WIDTH: usize = 14;
const CURSOR_HEIGHT: usize = 19;

const CURSOR_OUTLINE_COLOR: u32 =
    0xFF000000;

const CURSOR_COLOR: u32 =
    0xFFFFFFFF;

// Extra pixels around the cursor used when
// restoring the previous cursor area.
const CURSOR_RESTORE_PADDING: i64 = 1;

// ============================================================
// Mouse button bit flags
// ============================================================

const MOUSE_BUTTON_LEFT: u8 = 1 << 0;
const MOUSE_BUTTON_RIGHT: u8 = 1 << 1;
const MOUSE_BUTTON_MIDDLE: u8 = 1 << 2;
const MOUSE_BUTTON_4: u8 = 1 << 3;
const MOUSE_BUTTON_5: u8 = 1 << 4;

// ============================================================
// Compositor
// ============================================================

pub struct Compositor {
    pub windows: BTreeMap<u64, Window>,

    pub screen_width: usize,

    pub screen_height: usize,

    pub hardware_framebuffer: &'static mut [u32],

    // --------------------------------------------------------
    // Click detection state
    // --------------------------------------------------------

    /// Whether a real hardware left click has been detected
    /// and is waiting to be consumed by sys_click().
    click_pending: bool,

    /// Screen X coordinate of the detected click.
    click_x: i32,

    /// Screen Y coordinate of the detected click.
    click_y: i32,

    /// Whether the left mouse button is currently held.
    ///
    /// This is kept separately so we can detect:
    ///
    ///     press -> held -> release
    ///
    /// as one real click.
    left_button_down: bool,

    /// Screen position where the current left-button press
    /// started.
    click_start_x: i32,

    /// Screen position where the current left-button press
    /// started.
    click_start_y: i32,

    // --------------------------------------------------------
    // Clean composited screen
    // --------------------------------------------------------

    /// Clean composited screen.
    ///
    /// IMPORTANT:
    /// The mouse cursor is NEVER stored in this buffer.
    pub backbuffer: Vec<u32>,

    // --------------------------------------------------------
    // Mouse cursor state
    // --------------------------------------------------------

    /// Current cursor X position.
    pub mouse_x: i32,

    /// Current cursor Y position.
    pub mouse_y: i32,

    /// Current mouse button bitmap.
    pub mouse_buttons: u8,

    /// X position where the cursor was last drawn directly
    /// into the hardware framebuffer.
    cursor_drawn_x: i32,

    /// Y position where the cursor was last drawn directly
    /// into the hardware framebuffer.
    cursor_drawn_y: i32,

    /// Whether a cursor currently exists in the hardware
    /// framebuffer.
    cursor_visible: bool,

    next_window_id: u64,

    next_z_index: usize,
}

impl Compositor {
    // ========================================================
    // Initialization
    // ========================================================

    pub fn init(
        width: usize,
        height: usize,
        fb_ptr: *mut u32,
    ) {
        let size = width
            .checked_mul(height)
            .expect(
                "Rusty: framebuffer size overflow",
            );

        let hw_fb = unsafe {
            core::slice::from_raw_parts_mut(
                fb_ptr,
                size,
            )
        };

        let mut backbuffer =
            Vec::with_capacity(size);

        backbuffer.resize(
            size,
            0xFF0F172A,
        );

        let initial_mouse_x =
            (width / 2) as i32;

        let initial_mouse_y =
            (height / 2) as i32;

        *WM.lock() = Some(
            Compositor {
                windows: BTreeMap::new(),

                screen_width: width,

                screen_height: height,

                hardware_framebuffer: hw_fb,

                // ------------------------------------------------
                // Click detection starts completely idle.
                // ------------------------------------------------

                click_pending: false,

                click_x: 0,

                click_y: 0,

                left_button_down: false,

                click_start_x: initial_mouse_x,

                click_start_y: initial_mouse_y,

                // ------------------------------------------------
                // Cursor is intentionally NOT included
                // in the backbuffer.
                // ------------------------------------------------

                backbuffer,

                mouse_x: initial_mouse_x,

                mouse_y: initial_mouse_y,

                mouse_buttons: 0,

                cursor_drawn_x: initial_mouse_x,

                cursor_drawn_y: initial_mouse_y,

                cursor_visible: false,

                next_window_id: 1,

                next_z_index: 1,
            },
        );
    }

    // ========================================================
    // Create window
    // ========================================================

    pub fn create_window(
        &mut self,
        x: i32,
        y: i32,
        width: usize,
        height: usize,
    ) -> u64 {
        let id = self.next_window_id;

        self.next_window_id =
            self.next_window_id
                .saturating_add(1);

        let z = self.next_z_index;

        self.next_z_index =
            self.next_z_index
                .saturating_add(1);

        let window = Window::new(
            id,
            x,
            y,
            width,
            height,
            z,
        );

        self.windows.insert(
            id,
            window,
        );

        id
    }

    // ========================================================
    // Destroy window
    // ========================================================

    /// Destroys a window and releases its resources.
    ///
    /// Returns `true` if the window existed and was removed.
    /// Returns `false` if the window ID was invalid.
    pub fn destroy_window(
        &mut self,
        id: u64,
    ) -> bool {
        self.windows
            .remove(&id)
            .is_some()
    }

    // ========================================================
    // Update window pixels
    // ========================================================

    pub fn update_window_pixels(
        &mut self,
        id: u64,
        user_buffer: &[u32],
    ) -> bool {
        if let Some(win) =
            self.windows.get_mut(&id)
        {
            if user_buffer.len()
                != win.pixels.len()
            {
                return false;
            }

            win.pixels
                .copy_from_slice(
                    user_buffer,
                );

            return true;
        }

        false
    }

    // ========================================================
    // Mouse / Input
    // ========================================================

    /// Poll all currently available input events and update
    /// the compositor cursor position and physical button state.
    ///
    /// Mouse movement comes from:
    ///
    ///     InputEvent::MouseMove
    ///
    /// Mouse buttons come from:
    ///
    ///     InputEvent::MouseButtonDown
    ///     InputEvent::MouseButtonUp
    ///
    /// Keyboard and mouse-wheel events are ignored here because
    /// they belong to higher-level input consumers.
    ///
    /// A click is generated ONLY from a real hardware:
    ///
    ///     left button press
    ///             +
    ///     left button release
    ///
    /// Returns true when at least one input event was received.
    pub fn update_mouse(
        &mut self,
    ) -> bool {
        let mut changed = false;

        // Drain multiple events so the cursor catches up
        // if several reports arrived between compositor ticks.
        for _ in 0..32 {
            let Some(event) =
                crate::input::poll_mouse_event()
            else {
                break;
            };

            match event {
                // ------------------------------------------------
                // Mouse movement
                // ------------------------------------------------

                crate::input::InputEvent::MouseMove {
                    x,
                    y,
                } => {
                    self.mouse_x =
                        self.mouse_x
                            .saturating_add(
                                x as i32,
                            );

                    self.mouse_y =
                        self.mouse_y
                            .saturating_add(
                                y as i32,
                            );

                    self.clamp_mouse();

                    changed = true;
                }

                // ------------------------------------------------
                // Mouse button press
                // ------------------------------------------------

                crate::input::InputEvent::MouseButtonDown(
                    button,
                ) => {
                    let mask =
                        Self::mouse_button_mask(
                            button,
                        );

                    self.mouse_buttons |= mask;

                    if matches!(
                        button,
                        crate::input::MouseButton::Left
                    ) {
                        self.left_button_down =
                            true;

                        self.click_start_x =
                            self.mouse_x;

                        self.click_start_y =
                            self.mouse_y;
                    }

                    changed = true;
                }

                // ------------------------------------------------
                // Mouse button release
                // ------------------------------------------------

                crate::input::InputEvent::MouseButtonUp(
                    button,
                ) => {
                    let mask =
                        Self::mouse_button_mask(
                            button,
                        );

                    self.mouse_buttons &=
                        !mask;

                    if matches!(
                        button,
                        crate::input::MouseButton::Left
                    ) {
                        if self.left_button_down {
                            self.left_button_down =
                                false;

                            // The click belongs to where the
                            // physical button was pressed.
                            self.click_x =
                                self.click_start_x;

                            self.click_y =
                                self.click_start_y;

                            self.click_pending =
                                true;
                        }
                    }

                    changed = true;
                }

                // ------------------------------------------------
                // Mouse wheel
                // ------------------------------------------------

                crate::input::InputEvent::MouseWheel {
                    ..
                } => {
                    // The compositor does not currently use
                    // mouse-wheel events.
                    changed = true;
                }

                // ------------------------------------------------
                // Keyboard
                // ------------------------------------------------

                crate::input::InputEvent::KeyDown {
                    ..
                }
                | crate::input::InputEvent::KeyUp {
                    ..
                } => {
                    // Keyboard input is handled by higher-level
                    // input consumers/syscalls.
                }
            }
        }

        changed
    }

    // ========================================================
    // Mouse button conversion
    // ========================================================

    fn mouse_button_mask(
        button: crate::input::MouseButton,
    ) -> u8 {
        match button {
            crate::input::MouseButton::Left =>
                MOUSE_BUTTON_LEFT,

            crate::input::MouseButton::Right =>
                MOUSE_BUTTON_RIGHT,

            crate::input::MouseButton::Middle =>
                MOUSE_BUTTON_MIDDLE,

            crate::input::MouseButton::Button4 =>
                MOUSE_BUTTON_4,

            crate::input::MouseButton::Button5 =>
                MOUSE_BUTTON_5,
        }
    }

    // ========================================================
    // Click detection
    // ========================================================

    /// Checks whether a real hardware left click was detected
    /// at exactly the requested screen coordinate.
    ///
    /// This function NEVER:
    ///
    /// - moves the cursor
    /// - presses the mouse
    /// - releases the mouse
    /// - generates fake input
    ///
    /// It only consumes a click event that was already received
    /// from the physical mouse.
    pub fn sys_click(
        &mut self,
        x: usize,
        y: usize,
    ) -> bool {
        if !self.click_pending {
            return false;
        }

        if self.click_x < 0
            || self.click_y < 0
        {
            self.click_pending = false;

            return false;
        }

        let click_x =
            self.click_x as usize;

        let click_y =
            self.click_y as usize;

        if click_x != x
            || click_y != y
        {
            return false;
        }

        self.click_pending = false;

        true
    }

    // ========================================================
    // Rectangle click detection
    // ========================================================

    /// Checks whether a real hardware left click was detected
    /// inside the specified screen-space rectangle.
    ///
    /// Rectangle semantics:
    ///
    ///     x <= click_x < x + width
    ///     y <= click_y < y + height
    pub fn sys_click_rect(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) -> bool {
        if !self.click_pending {
            return false;
        }

        if self.click_x < 0
            || self.click_y < 0
        {
            self.click_pending = false;

            return false;
        }

        let click_x =
            self.click_x as usize;

        let click_y =
            self.click_y as usize;

        if width == 0
            || height == 0
        {
            return false;
        }

        let right =
            match x.checked_add(width) {
                Some(value) => value,
                None => return false,
            };

        let bottom =
            match y.checked_add(height) {
                Some(value) => value,
                None => return false,
            };

        let inside =
            click_x >= x
                && click_x < right
                && click_y >= y
                && click_y < bottom;

        if !inside {
            return false;
        }

        self.click_pending = false;

        true
    }

    // ========================================================
    // Cursor-only update
    // ========================================================

    /// Update only the mouse cursor directly in the hardware
    /// framebuffer.
    ///
    /// This avoids recompositing the entire desktop for every
    /// mouse movement.
    pub fn update_cursor_only(
        &mut self,
    ) {
        // ----------------------------------------------------
        // Restore the area underneath the previous cursor.
        // ----------------------------------------------------

        if self.cursor_visible {
            self.restore_cursor_area(
                self.cursor_drawn_x,
                self.cursor_drawn_y,
            );
        }

        // ----------------------------------------------------
        // Draw the cursor directly on the hardware framebuffer.
        // ----------------------------------------------------

        Self::draw_cursor_impl(
            &mut self.hardware_framebuffer,
            self.screen_width,
            self.screen_height,
            self.mouse_x,
            self.mouse_y,
        );

        self.cursor_drawn_x =
            self.mouse_x;

        self.cursor_drawn_y =
            self.mouse_y;

        self.cursor_visible = true;
    }

    // ========================================================
    // Restore previous cursor area
    // ========================================================

    fn restore_cursor_area(
        &mut self,
        cursor_x: i32,
        cursor_y: i32,
    ) {
        let left =
            cursor_x as i64
                - CURSOR_RESTORE_PADDING;

        let top =
            cursor_y as i64
                - CURSOR_RESTORE_PADDING;

        let right =
            cursor_x as i64
                + CURSOR_WIDTH as i64
                + CURSOR_RESTORE_PADDING;

        let bottom =
            cursor_y as i64
                + CURSOR_HEIGHT as i64
                + CURSOR_RESTORE_PADDING;

        let screen_width =
            self.screen_width as i64;

        let screen_height =
            self.screen_height as i64;

        let start_x =
            left.max(0);

        let start_y =
            top.max(0);

        let end_x =
            right.min(
                screen_width
                    .saturating_sub(1),
            );

        let end_y =
            bottom.min(
                screen_height
                    .saturating_sub(1),
            );

        if start_x > end_x
            || start_y > end_y
        {
            return;
        }

        for y in start_y..=end_y {
            let row =
                y as usize
                    * self.screen_width;

            for x in start_x..=end_x {
                let index =
                    row + x as usize;

                self.hardware_framebuffer[
                    index
                    ] =
                    self.backbuffer[
                        index
                        ];
            }
        }
    }

    // ========================================================
    // Mouse position
    // ========================================================

    pub fn mouse_position(
        &self,
    ) -> (i32, i32) {
        (
            self.mouse_x,
            self.mouse_y,
        )
    }

    // ========================================================
    // Mouse buttons
    // ========================================================

    pub fn mouse_buttons(
        &self,
    ) -> u8 {
        self.mouse_buttons
    }

    pub fn left_mouse_pressed(
        &self,
    ) -> bool {
        self.mouse_buttons
            & MOUSE_BUTTON_LEFT
            != 0
    }

    pub fn right_mouse_pressed(
        &self,
    ) -> bool {
        self.mouse_buttons
            & MOUSE_BUTTON_RIGHT
            != 0
    }

    pub fn middle_mouse_pressed(
        &self,
    ) -> bool {
        self.mouse_buttons
            & MOUSE_BUTTON_MIDDLE
            != 0
    }

    // ========================================================
    // Clamp cursor
    // ========================================================

    fn clamp_mouse(
        &mut self,
    ) {
        if self.screen_width == 0
            || self.screen_height == 0
        {
            self.mouse_x = 0;
            self.mouse_y = 0;

            return;
        }

        let max_x =
            self.screen_width
                .saturating_sub(1)
                as i32;

        let max_y =
            self.screen_height
                .saturating_sub(1)
                as i32;

        self.mouse_x =
            self.mouse_x.clamp(
                0,
                max_x,
            );

        self.mouse_y =
            self.mouse_y.clamp(
                0,
                max_y,
            );
    }

    // ========================================================
    // Draw compositor
    // ========================================================

    pub fn draw(
        &mut self,
    ) {
        let screen_width =
            self.screen_width;

        let screen_height =
            self.screen_height;

        // ====================================================
        // 1. Render desktop background
        // ====================================================

        for y in 0..screen_height {
            let ratio =
                (y * 255)
                    / screen_height.max(1);

            let r =
                (15 + ratio / 15)
                    as u32;

            let g =
                (23 + ratio / 10)
                    as u32;

            let b =
                (42 + ratio / 6)
                    as u32;

            let bg_color =
                0xFF000000
                    | (r << 16)
                    | (g << 8)
                    | b;

            let row_start =
                y * screen_width;

            self.backbuffer[
                row_start
                    ..row_start + screen_width
                ]
                .fill(bg_color);
        }

        // ====================================================
        // 2. Sort windows by Z-index
        // ====================================================

        let mut sorted_windows:
            Vec<&Window> =
            self.windows
                .values()
                .collect();

        sorted_windows.sort_by_key(
            |w| w.z_index,
        );

        // ====================================================
        // 3. Composite windows
        // ====================================================

        let backbuffer =
            &mut self.backbuffer;

        for win in sorted_windows {
            let start_x =
                core::cmp::max(
                    0,
                    win.x,
                ) as usize;

            let start_y =
                core::cmp::max(
                    0,
                    win.y,
                ) as usize;

            let end_x_i64 =
                (win.x as i64)
                    .saturating_add(
                        win.width as i64,
                    );

            let end_y_i64 =
                (win.y as i64)
                    .saturating_add(
                        win.height as i64,
                    );

            let end_x =
                core::cmp::min(
                    screen_width as i64,
                    end_x_i64,
                )
                    .max(0) as usize;

            let end_y =
                core::cmp::min(
                    screen_height as i64,
                    end_y_i64,
                )
                    .max(0) as usize;

            if start_x >= end_x
                || start_y >= end_y
            {
                continue;
            }

            for py in start_y..end_y {
                for px in start_x..end_x {
                    let win_x =
                        (
                            px as i64
                                - win.x as i64
                        ) as usize;

                    let win_y =
                        (
                            py as i64
                                - win.y as i64
                        ) as usize;

                    if win_x >= win.width
                        || win_y >= win.height
                    {
                        continue;
                    }

                    let win_idx =
                        win_y
                            * win.width
                            + win_x;

                    let screen_idx =
                        py
                            * screen_width
                            + px;

                    let pixel =
                        win.pixels[
                            win_idx
                            ];

                    let alpha =
                        (pixel >> 24)
                            & 0xFF;

                    if alpha == 255 {
                        backbuffer[
                            screen_idx
                            ] = pixel;
                    } else if alpha > 0 {
                        let dst =
                            backbuffer[
                                screen_idx
                                ];

                        let sa =
                            alpha as u32;

                        let inv_a =
                            255 - sa;

                        let sr =
                            (pixel >> 16)
                                & 0xFF;

                        let sg =
                            (pixel >> 8)
                                & 0xFF;

                        let sb =
                            pixel & 0xFF;

                        let dr =
                            (dst >> 16)
                                & 0xFF;

                        let dg =
                            (dst >> 8)
                                & 0xFF;

                        let db =
                            dst & 0xFF;

                        let r =
                            (sr * sa
                                + dr * inv_a)
                                / 255;

                        let g =
                            (sg * sa
                                + dg * inv_a)
                                / 255;

                        let b =
                            (sb * sa
                                + db * inv_a)
                                / 255;

                        backbuffer[
                            screen_idx
                            ] =
                            0xFF000000
                                | (r << 16)
                                | (g << 8)
                                | b;
                    }
                }
            }

            // =================================================
            // Window border
            // =================================================

            Self::draw_window_border_impl(
                backbuffer,
                screen_width,
                screen_height,
                win,
            );
        }

        // ====================================================
        // 4. Present clean backbuffer
        // ====================================================
        //
        // The cursor is NOT part of the backbuffer.
        //
        // This means a subsequent cursor-only update can
        // restore the old cursor position from this buffer.
        //

        self.hardware_framebuffer
            .copy_from_slice(
                &self.backbuffer,
            );

        // ====================================================
        // 5. Draw cursor directly to hardware framebuffer
        // ====================================================

        Self::draw_cursor_impl(
            &mut self.hardware_framebuffer,
            screen_width,
            screen_height,
            self.mouse_x,
            self.mouse_y,
        );

        self.cursor_drawn_x =
            self.mouse_x;

        self.cursor_drawn_y =
            self.mouse_y;

        self.cursor_visible = true;
    }

    // ========================================================
    // Cursor
    // ========================================================

    fn draw_cursor_impl(
        framebuffer: &mut [u32],
        screen_width: usize,
        screen_height: usize,
        x: i32,
        y: i32,
    ) {
        const ROW_WIDTHS: &[usize] = &[
            1, 2, 3, 4, 5, 6, 7, 8,
            9, 10, 11, 9, 7, 5, 3, 2,
        ];

        let x = x as i64;
        let y = y as i64;

        for (row, width)
        in ROW_WIDTHS.iter().enumerate()
        {
            let row_y =
                y + row as i64;

            for col in 0..*width {
                let px =
                    x + col as i64;

                Self::set_pixel_clamped_impl(
                    framebuffer,
                    screen_width,
                    screen_height,
                    px,
                    row_y,
                    CURSOR_COLOR,
                );
            }
        }
    }

    // ========================================================
    // Window border
    // ========================================================

    fn draw_window_border_impl(
        backbuffer: &mut [u32],
        screen_width: usize,
        screen_height: usize,
        win: &Window,
    ) {
        let border_color =
            0xFF45475A;

        let start_x =
            win.x;

        let start_y =
            win.y;

        let end_x =
            (win.x as i64)
                .saturating_add(
                    win.width as i64,
                )
                .saturating_sub(1);

        let end_y =
            (win.y as i64)
                .saturating_add(
                    win.height as i64,
                )
                .saturating_sub(1);

        if end_x < start_x as i64
            || end_y < start_y as i64
        {
            return;
        }

        for x in
            start_x as i64..=end_x
        {
            Self::set_pixel_clamped_impl(
                backbuffer,
                screen_width,
                screen_height,
                x,
                start_y as i64,
                border_color,
            );

            Self::set_pixel_clamped_impl(
                backbuffer,
                screen_width,
                screen_height,
                x,
                end_y,
                border_color,
            );
        }

        for y in
            start_y as i64..=end_y
        {
            Self::set_pixel_clamped_impl(
                backbuffer,
                screen_width,
                screen_height,
                start_x as i64,
                y,
                border_color,
            );

            Self::set_pixel_clamped_impl(
                backbuffer,
                screen_width,
                screen_height,
                end_x,
                y,
                border_color,
            );
        }
    }

    // ========================================================
    // Clamped pixel
    // ========================================================

    fn set_pixel_clamped_impl(
        framebuffer: &mut [u32],
        screen_width: usize,
        screen_height: usize,
        x: i64,
        y: i64,
        color: u32,
    ) {
        if x >= 0
            && x < screen_width as i64
            && y >= 0
            && y < screen_height as i64
        {
            let idx =
                (y as usize)
                    * screen_width
                    + x as usize;

            framebuffer[idx] = color;
        }
    }
}