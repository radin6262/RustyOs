use core::arch::asm;

// ============================================================
// PCI configuration-space helpers
// ============================================================

unsafe fn outl(port: u16, value: u32) {
    asm!(
        "out dx, eax",
        in("dx") port,
        in("eax") value,
        options(nomem, nostack, preserves_flags)
    );
}

unsafe fn inl(port: u16) -> u32 {
    let value: u32;

    asm!(
        "in eax, dx",
        out("eax") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );

    value
}

unsafe fn pci_read_u32(
    bus: u8,
    device: u8,
    function: u8,
    offset: u8,
) -> u32 {
    let address =
        0x8000_0000u32
            | ((bus as u32) << 16)
            | ((device as u32) << 11)
            | ((function as u32) << 8)
            | ((offset as u32) & 0xFC);

    outl(0xCF8, address);
    inl(0xCFC)
}

fn pci_read_u16(
    bus: u8,
    device: u8,
    function: u8,
    offset: u8,
) -> u16 {
    let value = unsafe {
        pci_read_u32(
            bus,
            device,
            function,
            offset & !3,
        )
    };

    let shift = ((offset & 2) * 8) as u32;

    ((value >> shift) & 0xFFFF) as u16
}

fn pci_read_u8(
    bus: u8,
    device: u8,
    function: u8,
    offset: u8,
) -> u8 {
    let value = unsafe {
        pci_read_u32(
            bus,
            device,
            function,
            offset & !3,
        )
    };

    let shift = ((offset & 3) * 8) as u32;

    ((value >> shift) & 0xFF) as u8
}

// ============================================================
// Legacy compatibility API
// ============================================================
//
// kernel_main() still asks the input layer for xHCI BAR0 before it starts
// the new USB subsystem. Keep that API, but make it read-only: the actual
// xHCI initialization, interrupt setup, BAR sizing, reset, and run sequence
// live in crate::usb::init.
//
// ============================================================

pub fn find_xhci_bar0() -> Option<usize> {
    for bus in 0..=255u16 {
        for device in 0..32u8 {
            let vendor = pci_read_u16(
                bus as u8,
                device,
                0,
                0x00,
            );

            if vendor == 0xFFFF {
                continue;
            }

            let header_type = pci_read_u8(
                bus as u8,
                device,
                0,
                0x0E,
            );

            let function_count =
                if header_type & 0x80 != 0 { 8 } else { 1 };

            for function in 0..function_count {
                let class = pci_read_u8(
                    bus as u8,
                    device,
                    function,
                    0x0B,
                );

                let subclass = pci_read_u8(
                    bus as u8,
                    device,
                    function,
                    0x0A,
                );

                let prog_if = pci_read_u8(
                    bus as u8,
                    device,
                    function,
                    0x09,
                );

                if class != 0x0C || subclass != 0x03 || prog_if != 0x30 {
                    continue;
                }

                let bar0 = unsafe {
                    pci_read_u32(
                        bus as u8,
                        device,
                        function,
                        0x10,
                    )
                };

                if bar0 == 0 || (bar0 & 1) != 0 {
                    continue;
                }

                let memory_type = (bar0 >> 1) & 0x03;

                if memory_type == 0x02 {
                    let bar1 = unsafe {
                        pci_read_u32(
                            bus as u8,
                            device,
                            function,
                            0x14,
                        )
                    };

                    let address =
                        ((bar1 as u64) << 32)
                            | ((bar0 as u64) & 0xFFFF_FFF0);

                    if address != 0 {
                        return Some(address as usize);
                    }
                } else {
                    let address = (bar0 & 0xFFFF_FFF0) as usize;

                    if address != 0 {
                        return Some(address);
                    }
                }
            }
        }
    }

    None
}

pub unsafe fn init(_bar0: usize) {
    crate::usb::init::init();
}

pub fn has_keyboard() -> bool {
    super::has_keyboard()
}

pub fn read_key() -> Option<super::Key> {
    super::read_key()
}
