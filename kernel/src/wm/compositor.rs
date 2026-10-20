#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use spin::Mutex;

use super::chrome::{
    DockWindowState,
    MENU_BAR_HEIGHT,
    TrafficLight,
};

use super::window::Window;

// ============================================================
// Press routing
// ============================================================

/// What a mouse press did to the desktop.
#[derive(Clone, Copy)]
struct PressResult {
    /// The chrome handled the click, so the application must not
    /// receive it.
    consumed: bool,

    /// Windows moved, appeared or disappeared, so the desktop has to
    /// be recomposited.
    redraw: bool,
}

impl PressResult {
    const IGNORED: Self =
        Self {
            consumed: false,
            redraw: false,
        };
}

pub static WM: Mutex<Option<Compositor>> =
    Mutex::new(None);

// ============================================================
// Cursor configuration
// ============================================================

const CURSOR_WIDTH: usize = 14;
const CURSOR_HEIGHT: usize = 22;

const CURSOR_OUTLINE_COLOR: u32 =
    0xFF000000;

const CURSOR_COLOR: u32 =
    0xFFFFFFFF;

/// Soft shadow drawn beneath the arrow, so it stays readable on
/// bright parts of the wallpaper.
const CURSOR_SHADOW_ALPHA: u8 =
    78;

// Extra pixels around the cursor used when
// restoring the previous cursor area.
const CURSOR_RESTORE_PADDING: i64 = 4;

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
    // Desktop chrome
    // --------------------------------------------------------

    /// Wallpaper, menu bar, window frames and dock.
    chrome: super::chrome::Chrome,

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

    /// Set when the chrome consumed the current press (a traffic
    /// light, the menu bar or the dock), so releasing the button must
    /// not leave a stale click for the application.
    suppress_click: bool,

    /// The last mouse report changed the desktop and the next service
    /// tick has to recomposite instead of only repainting the cursor.
    chrome_redraw: bool,

    /// Owner of a window closed with the red traffic light. The
    /// service tick quits this process once the WM lock is released.
    exit_pid: Option<u64>,

    /// Set when the Terminal dock icon is clicked with no Terminal
    /// window left. The idle desktop starts a fresh copy.
    launch_requested: bool,

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

        let chrome =
            super::chrome::Chrome::new(
                width,
                height,
            );

        *WM.lock() = Some(
            Compositor {
                windows: BTreeMap::new(),

                screen_width: width,

                screen_height: height,

                hardware_framebuffer: hw_fb,

                chrome,

                // ------------------------------------------------
                // Click detection starts completely idle.
                // ------------------------------------------------

                click_pending: false,

                click_x: 0,

                click_y: 0,

                left_button_down: false,

                suppress_click: false,

                chrome_redraw: false,

                exit_pid: None,

                launch_requested: false,

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
        owner_pid: u64,
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
            owner_pid,
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

    /// Remove every window owned by a process that is quitting.
    ///
    /// Window drop releases the pixel buffer, so quitting an app returns
    /// its window RAM as well as its address space.
    pub fn release_windows_owned_by(
        &mut self,
        pid: u64,
    ) -> bool {
        let ids:
            Vec<u64> =
            self.windows
                .values()
                .filter(|window| {
                    window.owner_pid == pid
                })
                .map(|window| window.id)
                .collect();

        for id in &ids {
            self.windows.remove(id);
        }

        !ids.is_empty()
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
    // Shared window buffer
    // ========================================================

    /// Physical address and byte size of a window's pixel buffer.
    ///
    /// This is the kernel side of a window's shared memory region
    /// and is what gets mapped into the owning userspace process.
    pub fn shared_buffer_info(
        &self,
        id: u64,
    ) -> Option<(u64, usize)> {
        self.windows
            .get(
                &id,
            )
            .map(|window| {
                (
                    window.buffer_physical,
                    window.buffer_map_byte_len(),
                )
            })
    }

    /// Record the virtual address a window's pixel buffer is mapped
    /// at inside the owning process.
    pub fn set_window_buffer_va(
        &mut self,
        id: u64,
        virtual_address: u64,
    ) {
        if let Some(window) =
            self.windows.get_mut(
                &id,
            )
        {
            window.set_buffer_va(
                virtual_address,
            );
        }
    }

    /// Virtual address of a window's shared pixel buffer, or `None`
    /// when the window has no userspace mapping yet.
    pub fn window_buffer_va(
        &self,
        id: u64,
    ) -> Option<u64> {
        self.windows
            .get(
                &id,
            )?
            .shared_buffer_address()
    }

    // ========================================================
    // Window focus and stacking
    // ========================================================

    /// Topmost window the user can see, which is the one that owns
    /// the focused title bar.
    fn focused_visible_id(
        &self,
    ) -> Option<u64> {
        self.windows
            .values()
            .filter(|window| {
                window.is_visible()
            })
            .max_by_key(|window| {
                window.z_index
            })
            .map(|window| window.id)
    }

    /// Move a window to the top of the stack.
    fn raise_window(
        &mut self,
        id: u64,
    ) -> bool {
        let top =
            self.windows
                .values()
                .map(|window| window.z_index)
                .max()
                .unwrap_or(0);

        let z =
            self.next_z_index;

        let Some(window) =
            self.windows.get_mut(&id)
        else {
            return false;
        };

        if window.z_index == top {
            return false;
        }

        window.z_index = z;

        self.next_z_index =
            z.saturating_add(1);

        true
    }

    /// Show the window whose title matches a dock item, restoring it
    /// after minimize or close.
    fn reveal_window_by_title(
        &mut self,
        title: &str,
    ) -> bool {
        let target =
            self.windows
                .values()
                .filter(|window| {
                    window
                        .title()
                        .eq_ignore_ascii_case(
                            title,
                        )
                })
                .max_by_key(|window| {
                    (window.minimized, window.z_index)
                })
                .map(|window| window.id);

        let Some(id) = target else {
            return false;
        };

        if let Some(window) =
            self.windows.get_mut(&id)
        {
            window.restore();
        }

        self.raise_window(id);

        true
    }

    /// Whether the last mouse report requires a full composite.
    pub fn take_chrome_redraw(
        &mut self,
    ) -> bool {
        core::mem::take(
            &mut self.chrome_redraw,
        )
    }

    /// Process that must be terminated after the next composite.
    pub fn take_exit_pid(
        &mut self,
    ) -> Option<u64> {
        self.exit_pid.take()
    }

    /// Whether the dock asked for a fresh Terminal window.
    pub fn take_launch_request(
        &mut self,
    ) -> bool {
        core::mem::take(
            &mut self.launch_requested,
        )
    }

    // ========================================================
    // Chrome hit testing
    // ========================================================

    /// Route a left-button press through the desktop chrome.
    ///
    /// The menu bar and dock float above the windows; below them the
    /// topmost window under the pointer wins. Pressing a traffic light
    /// performs the window action immediately, which is how the real
    /// macOS buttons behave.
    fn route_press(
        &mut self,
        x: i64,
        y: i64,
    ) -> PressResult {
        // ----------------------------------------------------
        // Menu bar takes clicks so they never leak into a window.
        // ----------------------------------------------------

        if y < MENU_BAR_HEIGHT {
            return PressResult {
                consumed: true,
                redraw: false,
            };
        }

        // ----------------------------------------------------
        // Dock: a click on a running app's icon raises its window,
        // brings a minimized window back, or starts a fresh Terminal
        // when the previous one was closed and quit.
        // ----------------------------------------------------

        if self.chrome.dock_contains(x, y) {
            if let Some(index) =
                self.chrome.dock_item_at(x, y)
            {
                let label =
                    super::chrome::DOCK_ITEMS[index]
                        .label;

                if self.reveal_window_by_title(
                    label,
                ) {
                    return PressResult {
                        consumed: true,
                        redraw: true,
                    };
                }

                if label.eq_ignore_ascii_case(
                    "Terminal",
                ) {
                    self.launch_requested =
                        true;
                }
            }

            return PressResult {
                consumed: true,
                redraw: false,
            };
        }

        // ----------------------------------------------------
        // Windows, topmost first.
        // ----------------------------------------------------

        let mut order:
            Vec<(usize, u64)> =
            self.windows
                .values()
                .map(|window| {
                    (
                        window.z_index,
                        window.id,
                    )
                })
                .collect();

        order.sort_by_key(
            |(z, _)| *z,
        );

        for (_, id)
        in order.into_iter().rev()
        {
            let Some(window) =
                self.windows.get(&id)
            else {
                continue;
            };

            if !window.is_visible()
                || !super::chrome::Chrome::frame_contains(
                    window,
                    x,
                    y,
                )
            {
                continue;
            }

            let light =
                super::chrome::Chrome::traffic_light_at(
                    window,
                    x,
                    y,
                );

            let in_title_bar =
                super::chrome::Chrome::title_bar_contains(
                    window,
                    x,
                    y,
                );

            // ------------------------------------------------
            // Traffic lights.
            // ------------------------------------------------

            if let Some(light) = light {
                match light {
                    // Closing destroys the window and quits the
                    // process that owns it. Removing the entry drops
                    // the pixel buffer back into the allocator.
                    TrafficLight::Close => {
                        let owner =
                            self.windows
                                .get(&id)
                                .map(|window| {
                                    window.owner_pid
                                })
                                .unwrap_or(0);

                        if owner != 0 {
                            self.exit_pid =
                                Some(owner);
                        }

                        self.windows
                            .remove(&id);
                    }

                    TrafficLight::Minimize => {
                        if let Some(window) =
                            self.windows.get_mut(&id)
                        {
                            window.minimized =
                                true;
                        }
                    }

                    // Zoom is reserved: the pixel buffer is a
                    // fixed size, so it would need a resize.
                    TrafficLight::Zoom => {}
                }

                return PressResult {
                    consumed: true,
                    redraw: true,
                };
            }

            // ------------------------------------------------
            // Any other press focuses the window.
            // ------------------------------------------------

            let needs_raise =
                self.focused_visible_id()
                    != Some(id);

            if needs_raise {
                self.raise_window(id);
            }

            return PressResult {
                consumed: in_title_bar,
                redraw: needs_raise,
            };
        }

        PressResult::IGNORED
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

                        // Traffic lights, the title bar and the dock
                        // act on press, just like macOS.
                        let result =
                            self.route_press(
                                self.mouse_x as i64,
                                self.mouse_y as i64,
                            );

                        if result.consumed {
                            self.suppress_click =
                                true;
                        }

                        if result.redraw {
                            self.chrome_redraw =
                                true;
                        }
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

                            if self.suppress_click {
                                // The chrome handled this press;
                                // the application must not see it.
                                self.suppress_click =
                                    false;
                            } else {
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
        //
        // The wallpaper is generated once at startup, which makes
        // this a single memory copy per frame.

        self.backbuffer
            .copy_from_slice(
                self.chrome.wallpaper(),
            );

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

        let focused_id =
            sorted_windows
                .iter()
                .rev()
                .find(|window| {
                    window.is_visible()
                })
                .map(|window| window.id);

        let mouse_x =
            self.mouse_x as i64;

        let mouse_y =
            self.mouse_y as i64;

        // ====================================================
        // 3. Composite windows
        // ====================================================

        let backbuffer =
            &mut self.backbuffer;

        for win in sorted_windows {
            // Minimized windows keep their buffer and geometry, but
            // own no screen space.
            if !win.is_visible() {
                continue;
            }

            let controls_hovered =
                super::chrome::Chrome::title_bar_contains(
                    win,
                    mouse_x,
                    mouse_y,
                );

            let hovered_light =
                super::chrome::Chrome::traffic_light_at(
                    win,
                    mouse_x,
                    mouse_y,
                );

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

            // ------------------------------------------------
            // Shadow and frame background, so the content and its
            // rounded corners are painted on top of them.
            // ------------------------------------------------

            self.chrome
                .draw_window_backdrop(
                    &mut super::surface::Surface::new(
                        &mut backbuffer[..],
                        screen_width,
                        screen_height,
                    ),
                    win,
                );

            for py in start_y..end_y {
                // --------------------------------------------
                // Rounded corners.
                //
                // Rows near the bottom of the frame are inset so
                // the content stops before the rounded corner.
                // --------------------------------------------

                let inset =
                    super::chrome::Chrome::content_row_inset(
                        win,
                        py as i64,
                    );

                let row_start_x =
                    core::cmp::max(
                        start_x as i64,
                        win.x as i64
                            + inset,
                    ) as usize;

                let row_end_x =
                    core::cmp::min(
                        end_x as i64,
                        win.x as i64
                            + win.width as i64
                            - inset,
                    ) as usize;

                if row_start_x >= row_end_x {
                    continue;
                }

                for px in row_start_x..row_end_x {
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
            // Title bar
            // =================================================

            self.chrome
                .draw_window_title_bar(
                    &mut super::surface::Surface::new(
                        &mut backbuffer[..],
                        screen_width,
                        screen_height,
                    ),
                    win,
                    Some(win.id) == focused_id,
                    hovered_light,
                    controls_hovered,
                );
        }

        // ====================================================
        // 4. Desktop chrome
        // ====================================================
        //
        // The dock and the menu bar float above every window.

        let focused_title =
            self.windows
                .values()
                .filter(|window| {
                    window.is_visible()
                })
                .max_by_key(|window| window.z_index)
                .map(|window| window.title())
                .unwrap_or("");

        // Dock indicators: one entry per open (or minimized) window.
        let dock_windows: Vec<DockWindowState> =
            self.windows
                .values()
                .map(|window| {
                    DockWindowState {
                        title: window.title(),
                        minimized: window.minimized,
                    }
                })
                .collect();

        self.chrome
            .draw_dock(
                &mut super::surface::Surface::new(
                    &mut backbuffer[..],
                    screen_width,
                    screen_height,
                ),
                mouse_x,
                mouse_y,
                &dock_windows,
            );

        self.chrome
            .draw_menu_bar(
                &mut super::surface::Surface::new(
                    &mut backbuffer[..],
                    screen_width,
                    screen_height,
                ),
                focused_title,
            );

        // ====================================================
        // 5. Present clean backbuffer
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
        // 6. Draw cursor directly to hardware framebuffer
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
        // Classic arrow silhouette: wide at the head, notched into
        // a slim tail.
        const ROW_WIDTHS: &[usize] = &[
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 7, 6, 5, 5, 4, 4,
            3,
        ];

        let x = x as i64;
        let y = y as i64;

        let filled =
            |row: i64, col: i64| -> bool {
                if row < 0
                    || col < 0
                {
                    return false;
                }

                match ROW_WIDTHS.get(row as usize) {
                    Some(width) => (col as usize) < *width,

                    None => false,
                }
            };

        let rows =
            ROW_WIDTHS.len() as i64;

        // 1. Drop shadow, offset down-right.
        for row in -1..=rows + 2 {
            for col in -1..=(CURSOR_WIDTH as i64 + 1) {
                if !filled(row - 2, col - 2) {
                    continue;
                }

                Self::blend_pixel_clamped_impl(
                    framebuffer,
                    screen_width,
                    screen_height,
                    x + col,
                    y + row,
                    0x000000,
                    CURSOR_SHADOW_ALPHA,
                );
            }
        }

        // 2. Dark outline, then the white body, so the pointer keeps
        // its shape over both dark and bright backgrounds.
        for row in -1..=rows {
            for col in -1..=(CURSOR_WIDTH as i64) {
                let inside =
                    filled(row, col);

                let outline =
                    !inside
                        && (filled(row - 1, col - 1)
                            || filled(row - 1, col)
                            || filled(row - 1, col + 1)
                            || filled(row, col - 1)
                            || filled(row, col + 1)
                            || filled(row + 1, col - 1)
                            || filled(row + 1, col)
                            || filled(row + 1, col + 1));

                if !inside && !outline {
                    continue;
                }

                Self::set_pixel_clamped_impl(
                    framebuffer,
                    screen_width,
                    screen_height,
                    x + col,
                    y + row,
                    if inside {
                        CURSOR_COLOR
                    } else {
                        CURSOR_OUTLINE_COLOR
                    },
                );
            }
        }
    }

    /// Blend one pixel with a fixed source alpha, clamped to screen.
    fn blend_pixel_clamped_impl(
        framebuffer: &mut [u32],
        screen_width: usize,
        screen_height: usize,
        x: i64,
        y: i64,
        color: u32,
        alpha: u8,
    ) {
        if x < 0
            || y < 0
            || x >= screen_width as i64
            || y >= screen_height as i64
        {
            return;
        }

        if alpha == 0 {
            return;
        }

        let index =
            (y as usize) * screen_width
                + x as usize;

        if alpha >= 255 {
            framebuffer[index] =
                0xFF00_0000 | (color & 0x00FF_FFFF);

            return;
        }

        let destination =
            framebuffer[index];

        let source =
            alpha as u32;

        let inverse =
            255 - source;

        let blend =
            |dst: u32, src: u32| -> u32 {
                (src * source
                    + dst * inverse)
                    / 255
            };

        let red =
            blend(
                (destination >> 16) & 0xFF,
                (color >> 16) & 0xFF,
            );

        let green =
            blend(
                (destination >> 8) & 0xFF,
                (color >> 8) & 0xFF,
            );

        let blue =
            blend(
                destination & 0xFF,
                color & 0xFF,
            );

        framebuffer[index] =
            0xFF00_0000 | (red << 16) | (green << 8) | blue;
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