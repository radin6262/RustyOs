pub mod key;
pub mod mouse;
pub mod usb;

pub use key::Key;
pub use mouse::MouseEvent;

pub fn init(
    bar0: usize,
) {
    unsafe {
        usb::init(
            bar0,
        );
    }
}

pub fn read_key()
    -> Option<Key>
{
    usb::read_key()
}

pub fn poll_mouse()
    -> Option<MouseEvent>
{
    usb::poll_mouse()
}

pub fn has_keyboard()
    -> bool
{
    usb::has_keyboard()
}

pub fn has_mouse()
    -> bool
{
    usb::has_mouse()
}