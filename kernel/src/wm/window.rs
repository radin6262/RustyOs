#![no_std]

extern crate alloc;

use alloc::alloc::{
    Layout,
    alloc,
};
use alloc::vec::Vec;

use crate::graphics::Color;
use crate::memory::PAGE_SIZE;

// ============================================================
// Window background color
// ============================================================

const DEFAULT_WINDOW_COLOR: u32 =
    0xFF181825;

// ============================================================
// Window limits
// ============================================================

/// Maximum width of a single window.
const MAX_WINDOW_WIDTH: usize = 4096;

/// Maximum height of a single window.
const MAX_WINDOW_HEIGHT: usize = 4096;

/// Maximum number of pixels a single window may contain.
///
/// 16 million pixels × 4 bytes = 64 MiB.
const MAX_WINDOW_PIXELS: usize = 16 * 1024 * 1024;

// ============================================================
// Pixel buffer allocation
// ============================================================
//
// A window pixel buffer is page aligned and physically contiguous
// so that it can be mapped into the owning userspace process as
// shared memory. That mapping is what lets an application draw
// straight into its window instead of issuing a syscall for every
// rectangle.
//
// The kernel heap is identity mapped (see `memory::init`), so the
// pointer handed out by the global allocator is also the physical
// address of the buffer. `allocate_frame` in the memory allocator
// relies on the same property.
//
// Whole pages can be reclaimed: `Window::drop` returns the buffer to
// the allocator's page free list, which a later window or process
// reuses. Non-page allocations keep the monotonic bump behaviour.
//

fn allocate_pixel_buffer(
    pixel_count: usize,
) -> (&'static mut [u32], u64, usize) {
    let byte_size =
        pixel_count
            .checked_mul(
                core::mem::size_of::<u32>(),
            )
            .expect(
                "Rusty: window pixel buffer overflow",
            );

    // --------------------------------------------------------
    // Round the allocation up to whole pages.
    //
    // The mapping handed to userspace covers whole pages, so the
    // last page of the buffer must belong to the buffer. Otherwise
    // the tail of that page would be neighbouring kernel heap
    // memory exposed as writable userspace memory.
    // --------------------------------------------------------

    let aligned_size =
        crate::memory::align_up(
            byte_size as u64,
            PAGE_SIZE,
        )
            .expect(
                "Rusty: window pixel buffer alignment overflow",
            ) as usize;

    let layout =
        Layout::from_size_align(
            aligned_size,
            PAGE_SIZE as usize,
        )
            .expect(
                "Rusty: invalid window pixel buffer layout",
            );

    let pointer =
        unsafe {
            alloc(layout)
        } as *mut u32;

    assert!(
        !pointer.is_null(),
        "Rusty: failed to allocate window pixel buffer",
    );

    assert_eq!(
        (pointer as u64) & (PAGE_SIZE - 1),
        0,
        "Rusty: window pixel buffer is not page aligned",
    );

    // --------------------------------------------------------
    // Confirm the identity mapping before the address is handed
    // to the page tables: userspace must write into these very
    // physical pages, because the compositor reads the buffer
    // through this pointer.
    // --------------------------------------------------------

    let translated =
        crate::memory::virtual_to_physical(
            pointer as u64,
        )
            .expect(
                "Rusty: window pixel buffer is not mapped",
            );

    assert_eq!(
        translated, pointer as u64,
        "Rusty: window pixel buffer is not identity mapped",
    );

    let pixels =
        unsafe {
            core::slice::from_raw_parts_mut(
                pointer,
                pixel_count,
            )
        };

    pixels.fill(
        DEFAULT_WINDOW_COLOR,
    );

    (
        pixels,
        pointer as u64,
        aligned_size,
    )
}

// ============================================================
// Window
// ============================================================

pub struct Window {
    pub id: u64,

    pub x: i32,
    pub y: i32,

    pub width: usize,
    pub height: usize,

    /// Window-owned ARGB/XRGB pixel buffer.
    ///
    /// Page aligned and physically contiguous, so it can be mapped
    /// into the owning userspace process.
    pub pixels: &'static mut [u32],

    /// Physical address of `pixels[0]`.
    pub buffer_physical: u64,

    /// Size of the page aligned allocation backing `pixels`.
    ///
    /// Kept so the buffer can be returned to the allocator exactly.
    pub buffer_alloc_bytes: usize,

    /// Process that created the window, or 0 for a kernel window.
    /// Closing the window terminates this process.
    pub owner_pid: u64,

    /// Virtual address at which `pixels` is mapped inside the owning
    /// userspace process.
    ///
    /// Zero while the window has no shared mapping. Kernel-owned
    /// windows (for example the serial log window) stay at zero.
    pub buffer_va: u64,

    pub z_index: usize,

    pub title: Vec<u8>,

    /// Hidden behind the minimize button. The window keeps its
    /// buffer and geometry so it can be restored from the dock.
    pub minimized: bool,
}

impl Window {
    // ========================================================
    // Construction
    // ========================================================

    pub fn new(
        id: u64,
        x: i32,
        y: i32,
        width: usize,
        height: usize,
        z_index: usize,
        owner_pid: u64,
    ) -> Self {
        assert!(width > 0, "Rusty: window width cannot be zero");
        assert!(height > 0, "Rusty: window height cannot be zero");
        assert!(width <= MAX_WINDOW_WIDTH, "Rusty: window width is too large");
        assert!(height <= MAX_WINDOW_HEIGHT, "Rusty: window height is too large");

        let pixel_count = width
            .checked_mul(height)
            .expect("Rusty: window pixel count overflow");

        assert!(pixel_count <= MAX_WINDOW_PIXELS, "Rusty: window is too large");

        let (
            pixels,
            buffer_physical,
            buffer_alloc_bytes,
        ) =
            allocate_pixel_buffer(
                pixel_count,
            );

        Self {
            id,
            x,
            y,
            width,
            height,
            pixels,
            buffer_physical,
            buffer_alloc_bytes,
            owner_pid,
            buffer_va: 0,
            z_index,
            title: Vec::new(),
            minimized: false,
        }
    }

    // ========================================================
    // Visibility
    // ========================================================

    /// Whether the compositor should draw this window right now.
    pub fn is_visible(&self) -> bool {
        !self.minimized
    }

    /// Bring the window back after minimizing it.
    pub fn restore(&mut self) {
        self.minimized = false;
    }

    // ========================================================
    // Clear
    // ========================================================

    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }

    // ========================================================
    // Pixel
    // ========================================================

    pub fn set_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }

        let index = y * self.width + x;
        self.pixels[index] = color;
    }

    // ========================================================
    // Rectangle
    // ========================================================

    pub fn fill_rect(
        &mut self,
        rx: usize,
        ry: usize,
        rw: usize,
        rh: usize,
        color: u32,
    ) {
        if rx >= self.width || ry >= self.height {
            return;
        }

        let end_x = rx.saturating_add(rw).min(self.width);
        let end_y = ry.saturating_add(rh).min(self.height);

        for y in ry..end_y {
            let row_start = y * self.width;
            for x in rx..end_x {
                self.pixels[row_start + x] = color;
            }
        }
    }

    // ========================================================
    // Text
    // ========================================================

    pub fn draw_string(
        &mut self,
        x: usize,
        y: usize,
        text: &str,
        color: Color,
        scale: usize,
    ) {
        crate::wm::surface::Surface::new(
            &mut self.pixels,
            self.width,
            self.height,
        )
            .text(
                x,
                y,
                text,
                color,
                scale,
            );
    }

    // ========================================================
    // Shared buffer
    // ========================================================

    /// Size of the pixel buffer in bytes.
    pub fn buffer_byte_len(
        &self,
    ) -> usize {
        self.pixels
            .len()
            .saturating_mul(
                core::mem::size_of::<u32>(),
            )
    }

    /// Size of the shared mapping created for this window.
    ///
    /// The pixel bytes rounded up to whole pages: the buffer is
    /// allocated that way so the final mapped page never contains
    /// neighbouring kernel memory.
    pub fn buffer_map_byte_len(
        &self,
    ) -> usize {
        crate::memory::align_up(
            self.buffer_byte_len() as u64,
            PAGE_SIZE,
        )
            .unwrap_or(
                self.buffer_byte_len()
                    as u64,
            ) as usize
    }

    /// Record the virtual address this window's pixel buffer is mapped
    /// at inside the owning process.
    pub fn set_buffer_va(
        &mut self,
        virtual_address: u64,
    ) {
        self.buffer_va =
            virtual_address;
    }

    /// Virtual address of the shared pixel buffer, or `None` when the
    /// window has no userspace mapping.
    pub fn shared_buffer_address(
        &self,
    ) -> Option<u64> {
        if self.buffer_va == 0 {
            None
        } else {
            Some(self.buffer_va)
        }
    }

    // ========================================================
    // Title
    // ========================================================

    pub fn set_title(&mut self, title: &str) {
        self.title.clear();
        self.title.extend_from_slice(title.as_bytes());
    }

    pub fn title(&self) -> &str {
        core::str::from_utf8(&self.title).unwrap_or("")
    }
}

// ============================================================
// Release
// ============================================================

impl Drop for Window {
    fn drop(&mut self) {
        crate::serial::write_str(
            "WM: release window ",
        );

        crate::serial::write_hex(self.id);

        crate::serial::write_str(" buffer-bytes=");

        crate::serial::write_hex(
            self.buffer_alloc_bytes as u64,
        );

        crate::serial::write_str("\n");

        crate::memory::release_page_block(
            self.buffer_physical,
            self.buffer_alloc_bytes,
        );

        // Drop the slice before the backing pages can be reused, so
        // the compiler cannot keep a pointer into freed memory alive.
        self.pixels = &mut [];
    }
}
