// ============================================================
// PCI discovery for the xHCI controller
// ============================================================
//
// The driver locates the first PCI function with class 0x0C (serial bus),
// subclass 0x03 (USB) and programming interface 0x30 (xHCI). The controller
// is driven through BAR0, which the PCI specification requires to be a
// memory BAR (typically a 64-bit one).
//
// Configuration space is accessed through the legacy 0xCF8/0xCFC pair. That
// mechanism is available on every x86 machine with PCI, including those that
// also provide ECAM, and it is enough to read the standard header.
// ============================================================

use x86_64::instructions::port::Port;

use alloc::vec::Vec;

use crate::memory;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

const PCI_COMMAND: u8 = 0x04;
const PCI_CLASS_REVISION: u8 = 0x08;
const PCI_HEADER_TYPE: u8 = 0x0E;
const PCI_BAR0: u8 = 0x10;
const PCI_BAR1: u8 = 0x14;

const COMMAND_MEMORY_SPACE: u16 = 1 << 1;
const COMMAND_BUS_MASTER: u16 = 1 << 2;

/// Upper bound for a BAR that the driver is willing to map. xHCI register
/// sets are small (16-64 KiB); a larger value most likely means the BAR was
/// misread.
const MAX_MMIO_SIZE: u64 = 16 * 1024 * 1024;

/// A discovered xHCI controller.
#[derive(Clone, Copy)]
pub struct XhciPci {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub mmio_base: usize,
    pub mmio_size: usize,
}

// ============================================================
// Raw configuration space access
// ============================================================

fn config_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC)
}

fn read_u32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
    let mut data_port = Port::<u32>::new(CONFIG_DATA);

    unsafe {
        address_port.write(config_address(bus, device, function, offset));
        data_port.read()
    }
}

fn write_u32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
    let mut data_port = Port::<u32>::new(CONFIG_DATA);

    unsafe {
        address_port.write(config_address(bus, device, function, offset));
        data_port.write(value);
    }
}

fn read_u16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let value = read_u32(bus, device, function, offset & !3);
    let shift = ((offset & 2) * 8) as u32;

    ((value >> shift) & 0xFFFF) as u16
}

fn read_u8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let value = read_u32(bus, device, function, offset & !3);
    let shift = ((offset & 3) * 8) as u32;

    ((value >> shift) & 0xFF) as u8
}

fn write_u16(bus: u8, device: u8, function: u8, offset: u8, value: u16) {
    let aligned = offset & !3;
    let shift = ((offset & 2) * 8) as u32;

    let old = read_u32(bus, device, function, aligned);
    let mask = !(0xFFFFu32 << shift);
    let new = (old & mask) | ((value as u32) << shift);

    write_u32(bus, device, function, aligned, new);
}

// ============================================================
// BAR sizing
// ============================================================

/// Read and size the memory BAR at `offset`.
///
/// Returns (address, size). The BAR is restored before returning.
fn size_memory_bar(
    bus: u8,
    device: u8,
    function: u8,
    offset: u8,
) -> Option<(u64, u64)> {
    let original_low = read_u32(bus, device, function, offset);

    // Bit 0: 0 = memory BAR, 1 = I/O BAR.
    if original_low & 1 != 0 {
        return None;
    }

    // Bits 2:1: 0 = 32-bit, 2 = 64-bit. Type 1 is reserved.
    let bar_type = (original_low >> 1) & 0x3;

    if bar_type == 1 {
        return None;
    }

    let is_64_bit = bar_type == 2;

    // Save and disable memory decoding while probing.
    let command = read_u16(bus, device, function, PCI_COMMAND);
    write_u16(
        bus,
        device,
        function,
        PCI_COMMAND,
        command & !COMMAND_MEMORY_SPACE,
    );

    let original_high = if is_64_bit {
        read_u32(bus, device, function, offset + 4)
    } else {
        0
    };

    // Probe: write all ones and read back which bits are hard-wired to zero.
    write_u32(bus, device, function, offset, 0xFFFF_FFFF);

    let mask_low = read_u32(bus, device, function, offset);

    let mask_high = if is_64_bit {
        write_u32(bus, device, function, offset + 4, 0xFFFF_FFFF);
        read_u32(bus, device, function, offset + 4)
    } else {
        0
    };

    // Restore the original address.
    write_u32(bus, device, function, offset, original_low);

    if is_64_bit {
        write_u32(bus, device, function, offset + 4, original_high);
    }

    write_u16(bus, device, function, PCI_COMMAND, command);

    let address_mask = if is_64_bit {
        (((mask_high as u64) << 32) | (mask_low as u64)) & !0xF
    } else {
        (mask_low as u64) & !0xF
    };

    if address_mask == 0 {
        return None;
    }

    let size = (!address_mask).wrapping_add(1);

    if !size.is_power_of_two() || size < 0x1000 || size > MAX_MMIO_SIZE {
        return None;
    }

    let address = if is_64_bit {
        ((original_high as u64) << 32) | ((original_low as u64) & !0xF)
    } else {
        (original_low as u64) & !0xF
    };

    if address == 0 {
        return None;
    }

    Some((address, size))
}

// ============================================================
// Discovery
// ============================================================

/// Upper bound on controllers the subsystem will drive. Eight is far above
/// any real machine while keeping the fixed per-controller allocations
/// bounded.
const MAX_CONTROLLERS: usize = 8;

/// Find every xHCI controller in the system.
pub fn find_all_xhci() -> Vec<XhciPci> {
    let mut controllers = Vec::new();

    'scan: for bus in 0..=255u8 {
        for device in 0..32u8 {
            let vendor = read_u16(bus, device, 0, 0x00);

            if vendor == 0xFFFF {
                continue;
            }

            let header_type = read_u8(bus, device, 0, PCI_HEADER_TYPE);
            let function_count = if header_type & 0x80 != 0 { 8 } else { 1 };

            for function in 0..function_count {
                let vendor = read_u16(bus, device, function, 0x00);

                if vendor == 0xFFFF {
                    continue;
                }

                let class_revision = read_u32(bus, device, function, PCI_CLASS_REVISION);

                let programming_interface = ((class_revision >> 8) & 0xFF) as u8;
                let subclass = ((class_revision >> 16) & 0xFF) as u8;
                let class = ((class_revision >> 24) & 0xFF) as u8;

                if class != 0x0C || subclass != 0x03 || programming_interface != 0x30 {
                    continue;
                }

                let Some((address, size)) =
                    size_memory_bar(bus, device, function, PCI_BAR0)
                else {
                    continue;
                };

                let address = match usize::try_from(address) {
                    Ok(value) => value,
                    Err(_) => continue,
                };

                let size = size as usize;

                // Map the register window uncached and identity mapped.
                unsafe {
                    memory::identity_map_mmio(address, size);
                }

                // Enable MMIO decoding and bus mastering.
                let command = read_u16(bus, device, function, PCI_COMMAND);
                write_u16(
                    bus,
                    device,
                    function,
                    PCI_COMMAND,
                    command | COMMAND_MEMORY_SPACE | COMMAND_BUS_MASTER,
                );

                controllers.push(XhciPci {
                    bus,
                    device,
                    function,
                    mmio_base: address,
                    mmio_size: size,
                });

                if controllers.len() >= MAX_CONTROLLERS {
                    break 'scan;
                }
            }
        }
    }

    controllers
}
