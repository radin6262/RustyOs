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
