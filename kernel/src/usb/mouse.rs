pub const USB_HID_CLASS: u8 = 0x03;
pub const USB_HID_BOOT_SUBCLASS: u8 = 0x01;
pub const USB_HID_MOUSE_PROTOCOL: u8 = 0x02;

pub fn announce(interface: u8) {
    crate::serial::write_str(
        "USB: =============================\n",
    );

    crate::serial::write_str(
        "USB: HID MOUSE DETECTED\n",
    );

    crate::serial::write_str(
        "USB: interface=",
    );

    crate::serial::write_hex(
        interface as u64,
    );

    crate::serial::write_str("\n");

    crate::serial::write_str(
        "USB: =============================\n",
    );
}