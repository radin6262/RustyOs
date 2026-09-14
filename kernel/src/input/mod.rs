pub mod key;
pub mod usb;

pub use key::Key;

pub fn init(bar0: usize) {
    unsafe {
        usb::init(bar0);
    }
}

pub fn read_key() -> Option<Key> {
    usb::read_key()
}

pub fn has_keyboard() -> bool {
    usb::has_keyboard()
}