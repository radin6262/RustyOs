#![no_std]

pub mod window;
pub mod compositor;
pub mod chrome;
pub mod surface;

pub use window::Window;
pub use compositor::{Compositor, WM};

pub fn init(width: usize, height: usize, fb_ptr: *mut u32) {
    Compositor::init(width, height, fb_ptr);
}

/// Drop all windows owned by a quitting process.
///
/// Each removed window returns its pixel buffer to the allocator.
pub fn release_windows_owned_by(pid: u64) {
    if let Some(compositor) = WM.lock().as_mut() {
        compositor.release_windows_owned_by(pid);
    }
}
