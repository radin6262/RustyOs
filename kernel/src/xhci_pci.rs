use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

use crate::{interrupts, memory, serial};

// ============================================================
// PCI configuration mechanism #1
// ============================================================

const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

// ============================================================
// PCI configuration-space offsets
// ============================================================

const PCI_VENDOR_ID: u8 = 0x00;
const PCI_DEVICE_ID: u8 = 0x02;
const PCI_COMMAND: u8 = 0x04;
const PCI_STATUS: u8 = 0x06;
const PCI_REVISION_ID: u8 = 0x08;
const PCI_PROG_IF: u8 = 0x09;
const PCI_SUBCLASS: u8 = 0x0A;
const PCI_CLASS: u8 = 0x0B;
const PCI_HEADER_TYPE: u8 = 0x0E;
const PCI_CAP_PTR: u8 = 0x34;

// ============================================================
// xHCI PCI class codes
// ============================================================

const PCI_CLASS_SERIAL_BUS: u8 = 0x0C;
const PCI_SUBCLASS_USB: u8 = 0x03;
const PCI_PROGIF_XHCI: u8 = 0x30;

// ============================================================
// Preferred xHCI controllers
// ============================================================
//
// These are controllers we explicitly prefer when present.
//
// IMPORTANT:
//
// This is NOT an allow-list.
//
// If none of these controllers exists, find_xhci() automatically
// falls back to the first xHCI controller discovered on PCI.
//
// Add more preferred controllers here when desired.
//
// Example:
//     (0x8086, 0x51ED),
//     (0x8086, 0x461E),
//
// The order matters: the first matching preferred entry is
// considered the preferred choice when multiple matching
// controllers are present.
//
// ============================================================

const PREFERRED_XHCI_CONTROLLERS: &[(u16, u16)] = &[(0x8086, 0x51ED)];

// ============================================================
// PCI command/status bits
// ============================================================

const PCI_COMMAND_MEMORY_SPACE: u16 = 1 << 1;
const PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;
const PCI_COMMAND_INTERRUPT_DISABLE: u16 = 1 << 10;

const PCI_STATUS_CAPABILITIES: u16 = 1 << 4;

// ============================================================
// PCI BAR
// ============================================================

const PCI_BAR0: u8 = 0x10;
const PCI_BAR_IO_SPACE: u32 = 1 << 0;
const PCI_BAR_MEMORY_TYPE_MASK: u32 = 0x06;
const PCI_BAR_MEMORY_TYPE_64: u32 = 0x04;
const PCI_BAR_MEMORY_ADDRESS_MASK: u32 = 0xFFFF_FFF0;

const PCI_MAX_BARS: usize = 6;

// ============================================================
// PCI capabilities
// ============================================================

const PCI_CAP_ID_MSI: u8 = 0x05;
const PCI_CAP_ID_MSIX: u8 = 0x11;

// MSI message control
const MSI_CONTROL_ENABLE: u16 = 1 << 0;
const MSI_CONTROL_MME_MASK: u16 = 0x000E;
const MSI_CONTROL_64BIT: u16 = 1 << 7;
const MSI_CONTROL_MASK_CAPABLE: u16 = 1 << 8;

// MSI-X message control
const MSIX_CONTROL_TABLE_SIZE_MASK: u16 = 0x07FF;
const MSIX_CONTROL_FUNCTION_MASK: u16 = 1 << 14;
const MSIX_CONTROL_ENABLE: u16 = 1 << 15;

// MSI-X capability layout
const MSIX_TABLE_OFFSET_MASK: u32 = 0xFFFF_FFF8;
const MSIX_BIR_MASK: u32 = 0x0000_0007;

// MSI-X table entry layout
const MSIX_TABLE_ENTRY_SIZE: usize = 16;
const MSIX_TABLE_VECTOR_CONTROL_MASK: u32 = 1;

// ============================================================
// Interrupt mode
// ============================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XhciInterruptMode {
    Disabled,
    Msi,
    Msix,
}

// ============================================================
// xHCI controller information
// ============================================================

#[derive(Clone, Copy)]
pub struct XhciPciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,

    pub vendor_id: u16,
    pub device_id: u16,
    pub revision_id: u8,

    pub bar0: u64,
    pub bar0_size: u64,

    pub msi_offset: Option<u8>,
    pub msix_offset: Option<u8>,

    pub interrupt_mode: XhciInterruptMode,
    pub interrupt_vector: u8,

    pub msix_table_bar: Option<u8>,
    pub msix_table_offset: Option<u32>,
    pub msix_table_size: Option<u16>,
}

// ============================================================
// Low-level PCI I/O
// ============================================================

unsafe fn outl(port: u16, value: u32) {
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") port,
            in("eax") value,
            options(nomem, nostack, preserves_flags),
        );
    }
}

unsafe fn inl(port: u16) -> u32 {
    let value: u32;

    unsafe {
        asm!(
            "in eax, dx",
            out("eax") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags),
        );
    }

    value
}

// ============================================================
// PCI configuration address
// ============================================================

#[inline(always)]
fn make_config_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC)
}

// ============================================================
// PCI config reads/writes
// ============================================================

#[inline(always)]
fn read_u32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = make_config_address(bus, device, function, offset);

    unsafe {
        outl(PCI_CONFIG_ADDRESS, address);

        inl(PCI_CONFIG_DATA)
    }
}

#[inline(always)]
fn write_u32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let address = make_config_address(bus, device, function, offset);

    unsafe {
        outl(PCI_CONFIG_ADDRESS, address);

        outl(PCI_CONFIG_DATA, value);
    }
}

#[inline(always)]
fn read_u16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let value = read_u32(bus, device, function, offset & !3);

    let shift = ((offset & 2) * 8) as u32;

    ((value >> shift) & 0xFFFF) as u16
}

#[inline(always)]
fn write_u16(bus: u8, device: u8, function: u8, offset: u8, value: u16) {
    let aligned = offset & !3;

    let mut current = read_u32(bus, device, function, aligned);

    let shift = ((offset & 2) * 8) as u32;

    let mask = 0xFFFFu32 << shift;

    current = (current & !mask) | ((value as u32) << shift);

    write_u32(bus, device, function, aligned, current);
}

#[inline(always)]
fn read_u8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let value = read_u32(bus, device, function, offset & !3);

    let shift = ((offset & 3) * 8) as u32;

    ((value >> shift) & 0xFF) as u8
}

// ============================================================
// PCI register helpers
// ============================================================

#[inline(always)]
fn pci_command(bus: u8, device: u8, function: u8) -> u16 {
    read_u16(bus, device, function, PCI_COMMAND)
}

#[inline(always)]
fn pci_status(bus: u8, device: u8, function: u8) -> u16 {
    read_u16(bus, device, function, PCI_STATUS)
}

// ============================================================
// Preferred-controller check
// ============================================================

#[inline]
fn is_preferred_xhci(vendor_id: u16, device_id: u16) -> bool {
    PREFERRED_XHCI_CONTROLLERS
        .iter()
        .any(|&(preferred_vendor, preferred_device)| {
            preferred_vendor == vendor_id && preferred_device == device_id
        })
}

// ============================================================
// Print controller identification
// ============================================================

fn log_xhci_identity(
    bus: u8,
    device: u8,
    function: u8,
    vendor_id: u16,
    device_id: u16,
    revision_id: u8,
) {
    serial::write_str("PCI: xHCI controller found during scan\n");

    serial::write_str("PCI: BDF=");

    serial::write_usize(bus as usize);

    serial::write_str(":");

    serial::write_usize(device as usize);

    serial::write_str(".");

    serial::write_usize(function as usize);

    serial::write_str("\n");

    serial::write_str("PCI: vendor=");

    serial::write_hex(vendor_id as u64);

    serial::write_str(" device=");

    serial::write_hex(device_id as u64);

    serial::write_str(" revision=");

    serial::write_hex(revision_id as u64);

    serial::write_str("\n");
}

// ============================================================
// PCI capability search
// ============================================================

fn find_capability(bus: u8, device: u8, function: u8, cap_id: u8) -> Option<u8> {
    let status = pci_status(bus, device, function);

    if status & PCI_STATUS_CAPABILITIES == 0 {
        return None;
    }

    let mut ptr = read_u8(bus, device, function, PCI_CAP_PTR);

    // The conventional PCI capability list lives in 0x40..0xFC.
    // Limit traversal so malformed hardware cannot loop forever.
    for _ in 0..48 {
        if ptr == 0 {
            return None;
        }

        if ptr < 0x40 || ptr & 0x03 != 0 {
            return None;
        }

        let id = read_u8(bus, device, function, ptr);

        if id == cap_id {
            return Some(ptr);
        }

        ptr = read_u8(bus, device, function, ptr + 1);
    }

    None
}

// ============================================================
// BAR helpers
// ============================================================

fn read_bar(bus: u8, device: u8, function: u8, bar_index: u8) -> Option<u64> {
    if bar_index as usize >= PCI_MAX_BARS {
        return None;
    }

    let offset = PCI_BAR0.checked_add(bar_index.checked_mul(4)?)?;

    let low = read_u32(bus, device, function, offset);

    if low == 0 || low == 0xFFFF_FFFF {
        return None;
    }

    if low & PCI_BAR_IO_SPACE != 0 {
        return None;
    }

    let memory_type = low & PCI_BAR_MEMORY_TYPE_MASK;

    if memory_type == PCI_BAR_MEMORY_TYPE_64 {
        if bar_index as usize + 1 >= PCI_MAX_BARS {
            return None;
        }

        let high = read_u32(bus, device, function, offset + 4);

        let address = ((high as u64) << 32) | ((low as u64) & PCI_BAR_MEMORY_ADDRESS_MASK as u64);

        if address == 0 { None } else { Some(address) }
    } else {
        let address = (low & PCI_BAR_MEMORY_ADDRESS_MASK) as u64;

        if address == 0 { None } else { Some(address) }
    }
}

fn read_bar_size(bus: u8, device: u8, function: u8, bar_index: u8) -> Option<u64> {
    if bar_index as usize >= PCI_MAX_BARS {
        return None;
    }

    let offset = PCI_BAR0.checked_add(bar_index.checked_mul(4)?)?;

    let original_low = read_u32(bus, device, function, offset);

    if original_low == 0xFFFF_FFFF || original_low & PCI_BAR_IO_SPACE != 0 {
        return None;
    }

    let is_64_bit = (original_low & PCI_BAR_MEMORY_TYPE_MASK) == PCI_BAR_MEMORY_TYPE_64;

    let original_high = if is_64_bit {
        if bar_index as usize + 1 >= PCI_MAX_BARS {
            return None;
        }

        read_u32(bus, device, function, offset + 4)
    } else {
        0
    };

    let original_command = pci_command(bus, device, function);

    let disabled_command = original_command & !(PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER);

    if disabled_command != original_command {
        write_u16(bus, device, function, PCI_COMMAND, disabled_command);
    }

    write_u32(bus, device, function, offset, 0xFFFF_FFFF);

    let size_low = read_u32(bus, device, function, offset);

    let size_high = if is_64_bit {
        write_u32(bus, device, function, offset + 4, 0xFFFF_FFFF);

        let value = read_u32(bus, device, function, offset + 4);

        write_u32(bus, device, function, offset + 4, original_high);

        value
    } else {
        0
    };

    write_u32(bus, device, function, offset, original_low);

    write_u16(bus, device, function, PCI_COMMAND, original_command);

    let mask = if is_64_bit {
        ((size_high as u64) << 32) | ((size_low as u64) & PCI_BAR_MEMORY_ADDRESS_MASK as u64)
    } else {
        (size_low & PCI_BAR_MEMORY_ADDRESS_MASK) as u64
    };

    if mask == 0 || mask == u64::MAX {
        return None;
    }

    let size = (!mask).wrapping_add(1);

    if size == 0 || !size.is_power_of_two() {
        return None;
    }

    Some(size)
}

fn read_bar0(bus: u8, device: u8, function: u8) -> Option<u64> {
    read_bar(bus, device, function, 0)
}

fn read_bar0_size(bus: u8, device: u8, function: u8) -> Option<u64> {
    read_bar_size(bus, device, function, 0)
}

// ============================================================
// PCI controller enable
// ============================================================

fn enable_controller(bus: u8, device: u8, function: u8) {
    let command = pci_command(bus, device, function);

    serial::write_str("PCI: COMMAND before=");

    serial::write_hex(command as u64);

    serial::write_str("\n");

    let new_command = command | PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER;

    if new_command != command {
        write_u16(bus, device, function, PCI_COMMAND, new_command);
    }

    let verified = pci_command(bus, device, function);

    serial::write_str("PCI: COMMAND after=");

    serial::write_hex(verified as u64);

    serial::write_str("\n");

    if verified & PCI_COMMAND_MEMORY_SPACE == 0 {
        panic!("Rusty: failed to enable PCI memory space for xHCI");
    }

    if verified & PCI_COMMAND_BUS_MASTER == 0 {
        panic!("Rusty: failed to enable PCI bus mastering for xHCI");
    }
}

fn disable_legacy_intx(device: &XhciPciDevice) {
    let command = pci_command(device.bus, device.device, device.function);

    let new_command = command | PCI_COMMAND_INTERRUPT_DISABLE;

    if new_command != command {
        write_u16(
            device.bus,
            device.device,
            device.function,
            PCI_COMMAND,
            new_command,
        );
    }
}

// ============================================================
// PCI capability diagnostics
// ============================================================

fn debug_dump_capabilities(device: &XhciPciDevice) {
    serial::write_str("PCI: capabilities:");

    if device.msi_offset.is_some() {
        serial::write_str(" MSI");
    }

    if device.msix_offset.is_some() {
        serial::write_str(" MSI-X");
    }

    if device.msi_offset.is_none() && device.msix_offset.is_none() {
        serial::write_str(" none");
    }

    serial::write_str("\n");

    if let Some(offset) = device.msi_offset {
        serial::write_str("PCI: MSI capability offset=");

        serial::write_hex(offset as u64);

        serial::write_str("\n");
    }

    if let Some(offset) = device.msix_offset {
        serial::write_str("PCI: MSI-X capability offset=");

        serial::write_hex(offset as u64);

        serial::write_str("\n");

        let control = read_u16(device.bus, device.device, device.function, offset + 0x02);

        let table = read_u32(device.bus, device.device, device.function, offset + 0x04);

        let pba = read_u32(device.bus, device.device, device.function, offset + 0x08);

        serial::write_str("PCI: MSI-X control=");

        serial::write_hex(control as u64);

        serial::write_str(" table_size=");

        serial::write_usize(((control & MSIX_CONTROL_TABLE_SIZE_MASK) as usize) + 1);

        serial::write_str(" table_bir=");

        serial::write_hex((table & MSIX_BIR_MASK) as u64);

        serial::write_str(" table_offset=");

        serial::write_hex((table & MSIX_TABLE_OFFSET_MASK) as u64);

        serial::write_str(" pba_bir=");

        serial::write_hex((pba & MSIX_BIR_MASK) as u64);

        serial::write_str(" pba_offset=");

        serial::write_hex((pba & MSIX_TABLE_OFFSET_MASK) as u64);

        serial::write_str("\n");
    }
}

// ============================================================
// MSI-X capability decoding
// ============================================================

fn msix_table_info(device: &XhciPciDevice) -> Option<(u8, u64, usize, u16)> {
    let cap = device.msix_offset?;

    let control = read_u16(device.bus, device.device, device.function, cap + 0x02);

    let entry_count = (control & MSIX_CONTROL_TABLE_SIZE_MASK) + 1;

    if entry_count == 0 {
        return None;
    }

    let table = read_u32(device.bus, device.device, device.function, cap + 0x04);

    let bir = (table & MSIX_BIR_MASK) as u8;

    let offset = (table & MSIX_TABLE_OFFSET_MASK) as u64;

    if bir as usize >= PCI_MAX_BARS {
        return None;
    }

    let bar = read_bar(device.bus, device.device, device.function, bir)?;

    /*
     * The Table Offset is relative to the BAR selected by BIR.
     *
     * Validate the complete table against the BAR aperture before mapping or
     * touching it. The controller has not been started yet, so a temporary
     * BAR-size probe is safe here and prevents an MMIO access outside the BAR
     * from turning into a page fault or bus fault.
     */
    if (offset & 0x7) != 0 {
        serial::write_str("PCI: ERROR: MSI-X table offset is not 8-byte aligned\n");

        return None;
    }

    let table_bytes = (entry_count as usize).checked_mul(MSIX_TABLE_ENTRY_SIZE)?;

    let table_end = offset.checked_add(table_bytes as u64)?;

    let bar_size = read_bar_size(device.bus, device.device, device.function, bir)?;

    if table_end > bar_size {
        serial::write_str("PCI: ERROR: MSI-X table extends beyond its BAR\n");

        return None;
    }

    let table_address = bar.checked_add(offset)?;

    Some((bir, table_address, table_bytes, entry_count))
}

// ============================================================
// MSI-X setup
// ============================================================

fn setup_msix(device: &XhciPciDevice, vector: u8) -> bool {
    let Some(cap) = device.msix_offset else {
        return false;
    };

    let Some((table_bar, table_address, table_bytes, entry_count)) = msix_table_info(device) else {
        serial::write_str("PCI: ERROR: unable to decode MSI-X table\n");

        return false;
    };

    if entry_count == 0 {
        return false;
    }

    serial::write_str("PCI: MSI-X table BAR=");

    serial::write_hex(table_bar as u64);

    serial::write_str(" address=");

    serial::write_hex(table_address);

    serial::write_str(" bytes=");

    serial::write_hex(table_bytes as u64);

    serial::write_str(" entries=");

    serial::write_usize(entry_count as usize);

    serial::write_str("\n");

    // The table is ordinary MMIO memory inside the BAR identified by BIR.
    // Identity-map only the MSI-X table region; xHCI BAR0 is mapped separately
    // by usb::init().
    unsafe {
        memory::identity_map_mmio(table_address as usize, table_bytes);
    }

    let table_virtual = table_address as usize;

    let entry = table_virtual;

    let (message_address, message_data) = match interrupts::msi_message(vector) {
        Some(value) => value,

        None => {
            serial::write_str("PCI: ERROR: unable to compose x86 MSI message\n");

            return false;
        }
    };

    // Disable MSI-X and assert Function Mask while programming the table.
    let mut control = read_u16(device.bus, device.device, device.function, cap + 0x02);

    control &= !MSIX_CONTROL_ENABLE;

    control |= MSIX_CONTROL_FUNCTION_MASK;

    write_u16(
        device.bus,
        device.device,
        device.function,
        cap + 0x02,
        control,
    );

    unsafe {
        /*
         * Mask every MSI-X entry first. Rusty currently uses interrupter 0
         * only, so entry 0 is the sole vector that will be unmasked. This
         * prevents stale firmware table entries from becoming live vectors.
         */
        for index in 0..entry_count as usize {
            let entry_address = entry
                .checked_add(
                    index
                        .checked_mul(MSIX_TABLE_ENTRY_SIZE)
                        .expect("PCI: MSI-X table entry offset overflow"),
                )
                .expect("PCI: MSI-X table entry address overflow");

            write_volatile(
                (entry_address + 0x0C) as *mut u32,
                MSIX_TABLE_VECTOR_CONTROL_MASK,
            );
        }

        /*
         * Program MSI-X vector 0. The xHCI driver uses interrupter 0, which
         * corresponds to MSI-X table entry 0.
         */
        write_volatile((entry + 0x00) as *mut u32, message_address as u32);

        write_volatile((entry + 0x04) as *mut u32, (message_address >> 32) as u32);

        write_volatile((entry + 0x08) as *mut u32, message_data as u32);

        /*
         * Vector Control bit 0 = Mask.
         * Clear it only after the message fields have been written.
         */
        write_volatile((entry + 0x0C) as *mut u32, 0);
    }

    // Make the table entry visible before enabling MSI-X.
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

    // Clear Function Mask and enable MSI-X.
    control &= !MSIX_CONTROL_FUNCTION_MASK;

    control |= MSIX_CONTROL_ENABLE;

    write_u16(
        device.bus,
        device.device,
        device.function,
        cap + 0x02,
        control,
    );

    let verified = read_u16(device.bus, device.device, device.function, cap + 0x02);

    if verified & MSIX_CONTROL_ENABLE == 0 {
        serial::write_str("PCI: ERROR: xHCI MSI-X failed to enable\n");

        return false;
    }

    if verified & MSIX_CONTROL_FUNCTION_MASK != 0 {
        serial::write_str("PCI: ERROR: xHCI MSI-X Function Mask remained set\n");

        return false;
    }

    serial::write_str("PCI: MSI-X enabled for xHCI, vector=");

    serial::write_usize(vector as usize);

    serial::write_str(" table_entry=0\n");

    true
}

// ============================================================
// Disable MSI-X
// ============================================================

fn disable_msix(device: &XhciPciDevice) {
    let Some(cap) = device.msix_offset else {
        return;
    };

    let mut control = read_u16(device.bus, device.device, device.function, cap + 0x02);

    control &= !MSIX_CONTROL_ENABLE;

    control |= MSIX_CONTROL_FUNCTION_MASK;

    write_u16(
        device.bus,
        device.device,
        device.function,
        cap + 0x02,
        control,
    );
}

// ============================================================
// MSI setup
// ============================================================

pub fn setup_msi(device: &XhciPciDevice, vector: u8) -> bool {
    let Some(cap) = device.msi_offset else {
        serial::write_str("PCI: xHCI has no MSI capability\n");

        return false;
    };

    disable_msix(device);

    let mut control = read_u16(device.bus, device.device, device.function, cap + 0x02);

    // Program one MSI message. MME=000 requests exactly one message.
    control &= !MSI_CONTROL_ENABLE;

    control &= !MSI_CONTROL_MME_MASK;

    write_u16(
        device.bus,
        device.device,
        device.function,
        cap + 0x02,
        control,
    );

    let Some((message_address, message_data)) = interrupts::msi_message(vector) else {
        serial::write_str("PCI: ERROR: unable to compose x86 MSI message\n");

        return false;
    };

    write_u32(
        device.bus,
        device.device,
        device.function,
        cap + 0x04,
        message_address as u32,
    );

    let data_offset = if control & MSI_CONTROL_64BIT != 0 {
        write_u32(
            device.bus,
            device.device,
            device.function,
            cap + 0x08,
            (message_address >> 32) as u32,
        );

        cap + 0x0C
    } else {
        // 32-bit MSI has no upper-address dword.
        cap + 0x08
    };

    write_u16(
        device.bus,
        device.device,
        device.function,
        data_offset,
        message_data,
    );

    // If per-vector masking is implemented, unmask message 0.
    //
    // The optional MSI Mask Bits register has a fixed capability-relative
    // location:
    //
    //   32-bit MSI -> +0x0C
    //   64-bit MSI -> +0x10
    //
    if control & MSI_CONTROL_MASK_CAPABLE != 0 {
        let mask_offset = if control & MSI_CONTROL_64BIT != 0 {
            cap + 0x10
        } else {
            cap + 0x0C
        };

        write_u32(device.bus, device.device, device.function, mask_offset, 0);
    }

    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

    control |= MSI_CONTROL_ENABLE;

    write_u16(
        device.bus,
        device.device,
        device.function,
        cap + 0x02,
        control,
    );

    let verified = read_u16(device.bus, device.device, device.function, cap + 0x02);

    if verified & MSI_CONTROL_ENABLE == 0 {
        serial::write_str("PCI: ERROR: xHCI MSI failed to enable\n");

        return false;
    }

    serial::write_str("PCI: MSI enabled for xHCI, vector=");

    serial::write_usize(vector as usize);

    serial::write_str("\n");

    true
}

// ============================================================
// Disable MSI
// ============================================================

fn disable_msi(device: &XhciPciDevice) {
    let Some(cap) = device.msi_offset else {
        return;
    };

    let mut control = read_u16(device.bus, device.device, device.function, cap + 0x02);

    control &= !MSI_CONTROL_ENABLE;

    write_u16(
        device.bus,
        device.device,
        device.function,
        cap + 0x02,
        control,
    );
}

// ============================================================
// Select and configure interrupt mode
// ============================================================

pub fn setup_interrupts(device: &XhciPciDevice, vector: u8) -> XhciInterruptMode {
    serial::write_str("PCI: configuring xHCI interrupts, vector=");

    serial::write_usize(vector as usize);

    serial::write_str("\n");

    /*
     * MSI and MSI-X are mutually exclusive mechanisms for this PCI function.
     * Make sure firmware cannot leave MSI enabled while the MSI-X table is
     * being programmed. setup_msi() re-enables MSI later if required.
     */
    disable_msi(device);

    // MSI-X is preferred because it gives the controller an independent vector
    // per interrupter. Rusty currently uses only interrupter 0, so one vector is
    // enough for now while retaining a path to multiple vectors later.
    if device.msix_offset.is_some() {
        serial::write_str("PCI: trying MSI-X first\n");

        if setup_msix(device, vector) {
            disable_msi(device);

            disable_legacy_intx(device);

            return XhciInterruptMode::Msix;
        }

        serial::write_str("PCI: MSI-X setup failed; falling back to MSI\n");

        disable_msix(device);
    }

    if device.msi_offset.is_some() {
        serial::write_str("PCI: trying MSI\n");

        if setup_msi(device, vector) {
            disable_legacy_intx(device);

            return XhciInterruptMode::Msi;
        }
    }

    serial::write_str("PCI: ERROR: xHCI has no usable MSI/MSI-X interrupt mode\n");

    XhciInterruptMode::Disabled
}

// ============================================================
// Build xHCI PCI device information
// ============================================================
//
// This performs all non-destructive PCI inspection necessary to create
// XhciPciDevice.
//
// The controller is NOT enabled here.
//
// That is important because find_xhci() may discover several xHCI
// controllers and must not initialize every controller while deciding
// which one to use.
//
// ============================================================

fn inspect_xhci(
    bus: u8,
    device: u8,
    function: u8,
    vendor_id: u16,
    device_id: u16,
    revision_id: u8,
) -> Option<XhciPciDevice> {
    let bar0 = match read_bar0(bus, device, function) {
        Some(value) => value,

        None => {
            serial::write_str("PCI: ERROR: xHCI BAR0 is invalid\n");

            return None;
        }
    };

    let bar0_size = match read_bar0_size(bus, device, function) {
        Some(value) => value,

        None => {
            serial::write_str("PCI: ERROR: unable to determine xHCI BAR0 size\n");

            return None;
        }
    };

    serial::write_str("PCI: BAR0=");

    serial::write_hex(bar0);

    serial::write_str(" size=");

    serial::write_hex(bar0_size);

    serial::write_str("\n");

    let msi_offset = find_capability(bus, device, function, PCI_CAP_ID_MSI);

    let msix_offset = find_capability(bus, device, function, PCI_CAP_ID_MSIX);

    Some(XhciPciDevice {
        bus,
        device,
        function,

        vendor_id,
        device_id,
        revision_id,

        bar0,
        bar0_size,

        msi_offset,
        msix_offset,

        interrupt_mode: XhciInterruptMode::Disabled,

        interrupt_vector: interrupts::XHCI_INTERRUPT_VECTOR,

        msix_table_bar: None,

        msix_table_offset: None,

        msix_table_size: None,
    })
}

// ============================================================
// Configure selected xHCI controller
// ============================================================
//
// At this point the controller has already been selected.
//
// Only the selected controller gets:
//
//   - capability diagnostics
//   - PCI Memory Space enable
//   - PCI Bus Master enable
//   - MSI-X/MSI setup
//
// ============================================================

fn configure_selected_xhci(mut xhci: XhciPciDevice) -> XhciPciDevice {
    serial::write_str("PCI: configuring selected xHCI controller\n");

    serial::write_str("PCI: selected BDF=");

    serial::write_usize(xhci.bus as usize);

    serial::write_str(":");

    serial::write_usize(xhci.device as usize);

    serial::write_str(".");

    serial::write_usize(xhci.function as usize);

    serial::write_str("\n");

    serial::write_str("PCI: selected vendor=");

    serial::write_hex(xhci.vendor_id as u64);

    serial::write_str(" device=");

    serial::write_hex(xhci.device_id as u64);

    serial::write_str("\n");

    debug_dump_capabilities(&xhci);

    // Enable MMIO decoding + bus mastering before controller use.
    enable_controller(xhci.bus, xhci.device, xhci.function);

    // Configure MSI-X/MSI before xHCI RUN is asserted. The xHCI driver itself
    // enables USBCMD.EIE + IMAN.IE later in run().
    xhci.interrupt_mode = setup_interrupts(&xhci, interrupts::XHCI_INTERRUPT_VECTOR);

    if let Some(cap) = xhci.msix_offset {
        let control = read_u16(xhci.bus, xhci.device, xhci.function, cap + 0x02);

        if control & MSIX_CONTROL_ENABLE != 0 {
            let table = read_u32(xhci.bus, xhci.device, xhci.function, cap + 0x04);

            let bir = (table & MSIX_BIR_MASK) as u8;

            let offset = (table & MSIX_TABLE_OFFSET_MASK) as u32;

            let count = (control & MSIX_CONTROL_TABLE_SIZE_MASK) + 1;

            xhci.msix_table_bar = Some(bir);

            xhci.msix_table_offset = Some(offset);

            xhci.msix_table_size = Some(count);
        }
    }

    serial::write_str("PCI: xHCI interrupt mode=");

    match xhci.interrupt_mode {
        XhciInterruptMode::Msix => serial::write_str("MSI-X"),

        XhciInterruptMode::Msi => serial::write_str("MSI"),

        XhciInterruptMode::Disabled => serial::write_str("DISABLED"),
    }

    serial::write_str(" vector=");

    serial::write_usize(xhci.interrupt_vector as usize);

    serial::write_str("\n");

    let command = pci_command(xhci.bus, xhci.device, xhci.function);

    let status = pci_status(xhci.bus, xhci.device, xhci.function);

    serial::write_str("PCI: final COMMAND=");

    serial::write_hex(command as u64);

    serial::write_str(" STATUS=");

    serial::write_hex(status as u64);

    serial::write_str("\n");

    xhci
}

// ============================================================
// Scan PCI bus for xHCI
// ============================================================
//
// Selection policy:
//
//   1. Scan all PCI buses/devices/functions.
//
//   2. Every PCI function matching:
//
//        Class    = 0x0C
//        Subclass = 0x03
//        ProgIF   = 0x30
//
//      is an xHCI controller.
//
//   3. The first valid xHCI controller is saved as a fallback.
//
//   4. If a controller matches PREFERRED_XHCI_CONTROLLERS,
//      it is selected immediately.
//
//   5. If no preferred controller was found after the complete scan,
//      the first valid xHCI controller is selected.
//
// This means a vendor/device ID not present in the preferred list
// is still fully supported.
//
// ============================================================

pub fn find_xhci() -> Option<XhciPciDevice> {
    serial::write_str("PCI: scanning for xHCI controllers\n");

    serial::write_str("PCI: preferred xHCI controller list:\n");

    for &(vendor_id, device_id) in PREFERRED_XHCI_CONTROLLERS.iter() {
        serial::write_str("PCI:   ");

        serial::write_hex(vendor_id as u64);

        serial::write_str(":");

        serial::write_hex(device_id as u64);

        serial::write_str("\n");
    }

    let mut first_xhci: Option<XhciPciDevice> = None;

    for bus in 0..=255u16 {
        for device in 0..32u8 {
            let bus8 = bus as u8;

            let vendor0 = read_u16(bus8, device, 0, PCI_VENDOR_ID);

            if vendor0 == 0xFFFF {
                continue;
            }

            let header_type = read_u8(bus8, device, 0, PCI_HEADER_TYPE);

            let function_count = if header_type & 0x80 != 0 { 8 } else { 1 };

            for function_index in 0..function_count {
                let function = function_index as u8;

                let vendor_id = read_u16(bus8, device, function, PCI_VENDOR_ID);

                if vendor_id == 0xFFFF {
                    continue;
                }

                let device_id = read_u16(bus8, device, function, PCI_DEVICE_ID);

                let revision_id = read_u8(bus8, device, function, PCI_REVISION_ID);

                let prog_if = read_u8(bus8, device, function, PCI_PROG_IF);

                let subclass = read_u8(bus8, device, function, PCI_SUBCLASS);

                let class = read_u8(bus8, device, function, PCI_CLASS);

                // Not an xHCI controller.
                if class != PCI_CLASS_SERIAL_BUS
                    || subclass != PCI_SUBCLASS_USB
                    || prog_if != PCI_PROGIF_XHCI
                {
                    continue;
                }

                log_xhci_identity(bus8, device, function, vendor_id, device_id, revision_id);

                let is_preferred = is_preferred_xhci(vendor_id, device_id);

                if is_preferred {
                    serial::write_str("PCI: xHCI matches preferred controller list\n");
                } else {
                    serial::write_str("PCI: xHCI is not on preferred list\n");

                    serial::write_str(
                        "PCI: xHCI will be used as fallback if no preferred controller is found\n",
                    );
                }

                let Some(xhci) =
                    inspect_xhci(bus8, device, function, vendor_id, device_id, revision_id)
                else {
                    serial::write_str(
                        "PCI: xHCI controller is not usable as a fallback because PCI resources are invalid\n",
                    );

                    continue;
                };

                // Remember the FIRST valid xHCI controller.
                //
                // This is deliberately only assigned once so scan order
                // determines the fallback controller.
                if first_xhci.is_none() {
                    serial::write_str("PCI: saving this controller as first xHCI fallback\n");

                    first_xhci = Some(xhci);
                }

                // Preferred controller wins immediately.
                if is_preferred {
                    serial::write_str("PCI: selecting preferred xHCI controller\n");

                    return Some(configure_selected_xhci(xhci));
                }
            }
        }
    }

    // No preferred controller was found.
    //
    // Fall back to the first valid xHCI discovered during the scan.
    if let Some(xhci) = first_xhci {
        serial::write_str("PCI: no preferred xHCI controller found\n");

        serial::write_str("PCI: selecting FIRST discovered xHCI controller as fallback\n");

        return Some(configure_selected_xhci(xhci));
    }

    serial::write_str("PCI: no xHCI controller found\n");

    None
}

// ============================================================
// Debug dump xHCI root ports
// ============================================================

pub fn debug_dump_ports(mmio_base: usize) {
    serial::write_str("xHCI: PORT DUMP BEGIN\n");

    let cap_length = unsafe { read_volatile((mmio_base + 0x00) as *const u8) } as usize;

    let op_base = mmio_base + cap_length;

    let hcsparams1 = unsafe { read_volatile((mmio_base + 0x04) as *const u32) };

    let hccparams1 = unsafe { read_volatile((mmio_base + 0x10) as *const u32) };

    let usbcmd = unsafe { read_volatile((op_base + 0x00) as *const u32) };

    let usbsts = unsafe { read_volatile((op_base + 0x04) as *const u32) };

    let pagesize = unsafe { read_volatile((op_base + 0x08) as *const u32) };

    let max_slots = (hcsparams1 & 0xFF) as usize;

    let max_interrupters = ((hcsparams1 >> 8) & 0x7FF) as usize;

    let max_ports = ((hcsparams1 >> 24) & 0xFF) as usize;

    let supports_64bit = (hccparams1 & 1) != 0;

    let context_size_64 = (hccparams1 & (1 << 2)) != 0;

    let xecp = (hccparams1 >> 16) as u16;

    serial::write_str("xHCI: BASE=");

    serial::write_hex(mmio_base as u64);

    serial::write_str(" CAP=");

    serial::write_hex(cap_length as u64);

    serial::write_str(" OP=");

    serial::write_hex(op_base as u64);

    serial::write_str("\n");

    serial::write_str("xHCI: SLOTS=");

    serial::write_usize(max_slots);

    serial::write_str(" INTERRUPTERS=");

    serial::write_usize(max_interrupters);

    serial::write_str(" PORTS=");

    serial::write_usize(max_ports);

    serial::write_str("\n");

    serial::write_str("xHCI: HCCPARAMS1=");

    serial::write_hex(hccparams1 as u64);

    serial::write_str(" AC64=");

    serial::write_usize(supports_64bit as usize);

    serial::write_str(" CSZ64=");

    serial::write_usize(context_size_64 as usize);

    serial::write_str(" XECP=");

    serial::write_hex(xecp as u64);

    serial::write_str("\n");

    serial::write_str("xHCI: USBCMD=");

    serial::write_hex(usbcmd as u64);

    serial::write_str(" USBSTS=");

    serial::write_hex(usbsts as u64);

    serial::write_str(" PAGESIZE=");

    serial::write_hex(pagesize as u64);

    serial::write_str("\n");

    for port_index in 0..max_ports {
        let address = op_base + 0x400 + (port_index * 0x10);

        let portsc = unsafe { read_volatile(address as *const u32) };

        let portpmsc = unsafe { read_volatile((address + 0x04) as *const u32) };

        let portli = unsafe { read_volatile((address + 0x08) as *const u32) };

        let porthlpmc = unsafe { read_volatile((address + 0x0C) as *const u32) };

        serial::write_str("xHCI: P");

        serial::write_usize(port_index + 1);

        serial::write_str(" SC=");

        serial::write_hex(portsc as u64);

        serial::write_str(" PMSC=");

        serial::write_hex(portpmsc as u64);

        serial::write_str(" LI=");

        serial::write_hex(portli as u64);

        serial::write_str(" HLPMC=");

        serial::write_hex(porthlpmc as u64);

        serial::write_str(" CCS=");

        serial::write_usize((portsc & 1) as usize);

        serial::write_str(" PED=");

        serial::write_usize(((portsc >> 1) & 1) as usize);

        serial::write_str(" PR=");

        serial::write_usize(((portsc >> 4) & 1) as usize);

        serial::write_str(" PLS=");

        serial::write_hex(((portsc >> 5) & 0xF) as u64);

        serial::write_str(" PP=");

        serial::write_usize(((portsc >> 9) & 1) as usize);

        serial::write_str(" SPEED=");

        serial::write_hex(((portsc >> 10) & 0xF) as u64);

        serial::write_str(" CSC=");

        serial::write_usize(((portsc >> 17) & 1) as usize);

        serial::write_str(" PEC=");

        serial::write_usize(((portsc >> 18) & 1) as usize);

        serial::write_str(" PRC=");

        serial::write_usize(((portsc >> 21) & 1) as usize);

        serial::write_str(" PLC=");

        serial::write_usize(((portsc >> 22) & 1) as usize);

        serial::write_str("\n");
    }

    serial::write_str("xHCI: PORT DUMP END\n");
}
