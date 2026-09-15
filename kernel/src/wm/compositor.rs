#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use spin::Mutex;

use super::window::Window;

pub static WM: Mutex<Option<Compositor>> = Mutex::new(None);

pub struct Compositor {
    pub windows: BTreeMap<u64, Window>,
    pub screen_width: usize,
    pub screen_height: usize,
    pub hardware_framebuffer: &'static mut [u32],
    pub backbuffer: Vec<u32>,

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
            .expect("Rusty: framebuffer size overflow");

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

        *WM.lock() = Some(Compositor {
            windows: BTreeMap::new(),

            screen_width: width,
            screen_height: height,

            hardware_framebuffer: hw_fb,

            backbuffer,

            next_window_id: 1,
            next_z_index: 1,
        });
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
            self.next_window_id.saturating_add(1);

        let z = self.next_z_index;
        self.next_z_index =
            self.next_z_index.saturating_add(1);

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
                .copy_from_slice(user_buffer);

            return true;
        }

        false
    }

    // ========================================================
    // Draw compositor
    // ========================================================

    pub fn draw(&mut self) {
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
                (15 + ratio / 15) as u32;

            let g =
                (23 + ratio / 10) as u32;

            let b =
                (42 + ratio / 6) as u32;

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

        sorted_windows
            .sort_by_key(|w| w.z_index);

        let backbuffer =
            &mut self.backbuffer;

        // ====================================================
        // 3. Composite windows
        // ====================================================

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
                ).max(0) as usize;

            let end_y =
                core::cmp::min(
                    screen_height as i64,
                    end_y_i64,
                ).max(0) as usize;

            if start_x >= end_x
                || start_y >= end_y
            {
                continue;
            }

            for py in start_y..end_y {
                for px in start_x..end_x {
                    let win_x =
                        (px as i64
                            - win.x as i64)
                            as usize;

                    let win_y =
                        (py as i64
                            - win.y as i64)
                            as usize;

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
                        py * screen_width
                            + px;

                    let pixel =
                        win.pixels[win_idx];

                    let alpha =
                        (pixel >> 24) & 0xFF;

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
        // 4. Present backbuffer
        // ====================================================

        self.hardware_framebuffer
            .copy_from_slice(
                &self.backbuffer,
            );
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
        backbuffer: &mut [u32],
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
                    + (x as usize);

            backbuffer[idx] = color;
        }
    }
}