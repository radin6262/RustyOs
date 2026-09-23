use core::arch::asm;

use crate::serial;

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
// PCI command register bits
// ============================================================

const PCI_COMMAND_IO_SPACE: u16 = 1 << 0;
const PCI_COMMAND_MEMORY_SPACE: u16 = 1 << 1;
const PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;

// ============================================================
// PCI status register bits
// ============================================================

const PCI_STATUS_CAPABILITIES: u16 = 1 << 4;

// ============================================================
// PCI BAR
// ============================================================

const PCI_BAR0: u8 = 0x10;

const PCI_BAR_IO_SPACE: u32 = 1 << 0;
const PCI_BAR_MEMORY_TYPE_MASK: u32 = 0x06;
const PCI_BAR_MEMORY_TYPE_64: u32 = 0x04;
const PCI_BAR_MEMORY_ADDRESS_MASK: u32 = 0xFFFF_FFF0;

// ============================================================
// PCI capabilities
// ============================================================

const PCI_CAP_ID_MSI: u8 = 0x05;
const PCI_CAP_ID_MSIX: u8 = 0x11;

// MSI capability
const MSI_CONTROL_64BIT: u16 = 1 << 7;
const MSI_CONTROL_ENABLE: u16 = 1 << 0;

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

    pub bar0: u64,
    pub bar0_size: u64,

    pub msi_offset: Option<u8>,
    pub msix_offset: Option<u8>,
}

// ============================================================
// Low-level PCI I/O
// ============================================================

unsafe fn outl(
    port: u16,
    value: u32,
) {
    asm!(
    "out dx, eax",
    in("dx") port,
    in("eax") value,
    options(
    nomem,
    nostack,
    preserves_flags,
    ),
    );
}

unsafe fn inl(
    port: u16,
) -> u32 {
    let value: u32;

    asm!(
    "in eax, dx",
    out("eax") value,
    in("dx") port,
    options(
    nomem,
    nostack,
    preserves_flags,
    ),
    );

    value
}

// ============================================================
// PCI configuration address
// ============================================================

fn make_config_address(
    bus: u8,
    device: u8,
    function: u8,
    offset: u8,
) -> u32 {
    0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC)
}

// ============================================================
// PCI config read/write
// ============================================================

fn read_u32(
    bus: u8,
    device: u8,
    function: u8,
    offset: u8,
) -> u32 {
    let address = make_config_address(
        bus,
        device,
        function,
        offset,
    );

    unsafe {
        outl(
            PCI_CONFIG_ADDRESS,
            address,
        );

        inl(
            PCI_CONFIG_DATA,
        )
    }
}

fn write_u32(
    bus: u8,
    device: u8,
    function: u8,
    offset: u8,
    value: u32,
) {
    let address = make_config_address(
        bus,
        device,
        function,
        offset,
    );

    unsafe {
        outl(
            PCI_CONFIG_ADDRESS,
            address,
        );

        outl(
            PCI_CONFIG_DATA,
            value,
        );
    }
}

fn read_u16(
    bus: u8,
    device: u8,
    function: u8,
    offset: u8,
) -> u16 {
    let value = read_u32(
        bus,
        device,
        function,
        offset & !3,
    );

    let shift = ((offset & 2) * 8) as u32;

    ((value >> shift) & 0xFFFF) as u16
}

fn write_u16(
    bus: u8,
    device: u8,
    function: u8,
    offset: u8,
    value: u16,
) {
    let aligned = offset & !3;

    let mut current = read_u32(
        bus,
        device,
        function,
        aligned,
    );

    let shift = ((offset & 2) * 8) as u32;

    let mask = 0xFFFFu32 << shift;

    current =
        (current & !mask)
            | ((value as u32) << shift);

    write_u32(
        bus,
        device,
        function,
        aligned,
        current,
    );
}

fn read_u8(
    bus: u8,
    device: u8,
    function: u8,
    offset: u8,
) -> u8 {
    let value = read_u32(
        bus,
        device,
        function,
        offset & !3,
    );

    let shift = ((offset & 3) * 8) as u32;

    ((value >> shift) & 0xFF) as u8
}

// ============================================================
// PCI register helpers
// ============================================================

fn pci_command(
    bus: u8,
    device: u8,
    function: u8,
) -> u16 {
    read_u16(
        bus,
        device,
        function,
        PCI_COMMAND,
    )
}

fn pci_status(
    bus: u8,
    device: u8,
    function: u8,
) -> u16 {
    read_u16(
        bus,
        device,
        function,
        PCI_STATUS,
    )
}

// ============================================================
// PCI capability search
// ============================================================

fn find_capability(
    bus: u8,
    device: u8,
    function: u8,
    cap_id: u8,
) -> Option<u8> {
    let status = pci_status(
        bus,
        device,
        function,
    );

    if status & PCI_STATUS_CAPABILITIES == 0 {
        return None;
    }

    let mut ptr = read_u8(
        bus,
        device,
        function,
        PCI_CAP_PTR,
    );

    // PCI capability pointers are byte offsets aligned to 4 bytes.
    // Put a hard limit on traversal so malformed hardware cannot
    // trap us in an infinite loop.
    for _ in 0..48 {
        if ptr == 0 {
            return None;
        }

        if ptr < 0x40 || ptr & 0x03 != 0 {
            return None;
        }

        let id = read_u8(
            bus,
            device,
            function,
            ptr,
        );

        if id == cap_id {
            return Some(ptr);
        }

        ptr = read_u8(
            bus,
            device,
            function,
            ptr + 1,
        );
    }

    None
}

// ============================================================
// Print PCI capability information
// ============================================================

fn debug_dump_capabilities(
    device: &XhciPciDevice,
) {
    serial::write_str(
        "PCI: capabilities:",
    );

    if device.msi_offset.is_some() {
        serial::write_str(
            " MSI",
        );
    }

    if device.msix_offset.is_some() {
        serial::write_str(
            " MSI-X",
        );
    }

    if device.msi_offset.is_none()
        && device.msix_offset.is_none()
    {
        serial::write_str(
            " none",
        );
    }

    serial::write_str(
        "\n",
    );

    if let Some(offset) = device.msi_offset {
        serial::write_str(
            "PCI: MSI capability offset=",
        );

        serial::write_hex(
            offset as u64,
        );

        serial::write_str(
            "\n",
        );
    }

    if let Some(offset) = device.msix_offset {
        serial::write_str(
            "PCI: MSI-X capability offset=",
        );

        serial::write_hex(
            offset as u64,
        );

        serial::write_str(
            "\n",
        );
    }
}

// ============================================================
// Enable PCI xHCI controller
// ============================================================
//
// We intentionally enable Memory Space and Bus Mastering only
// after BAR probing has completed.
//
// BAR probing writes temporary values into BAR registers, so doing
// it while the controller is already actively decoding MMIO/DMA
// transactions is unnecessary and potentially unsafe.
//
// ============================================================

fn enable_controller(
    bus: u8,
    device: u8,
    function: u8,
) {
    let command = pci_command(
        bus,
        device,
        function,
    );

    serial::write_str(
        "PCI: COMMAND before=",
    );

    serial::write_hex(
        command as u64,
    );

    serial::write_str(
        "\n",
    );

    let new_command =
        command
            | PCI_COMMAND_MEMORY_SPACE
            | PCI_COMMAND_BUS_MASTER;

    if new_command != command {
        write_u16(
            bus,
            device,
            function,
            PCI_COMMAND,
            new_command,
        );
    }

    let verified = pci_command(
        bus,
        device,
        function,
    );

    serial::write_str(
        "PCI: COMMAND after=",
    );

    serial::write_hex(
        verified as u64,
    );

    serial::write_str(
        "\n",
    );

    if verified & PCI_COMMAND_MEMORY_SPACE == 0 {
        panic!(
            "Rusty: failed to enable PCI memory space for xHCI"
        );
    }

    if verified & PCI_COMMAND_BUS_MASTER == 0 {
        panic!(
            "Rusty: failed to enable PCI bus mastering for xHCI"
        );
    }

    // I/O space is not required for an MMIO xHCI controller.
    //
    // We deliberately do not force PCI_COMMAND_IO_SPACE on.
}

// ============================================================
// Read BAR0 address
// ============================================================

fn read_bar0(
    bus: u8,
    device: u8,
    function: u8,
) -> Option<u64> {
    let low = read_u32(
        bus,
        device,
        function,
        PCI_BAR0,
    );

    // BAR0 must be a memory BAR.
    if low & PCI_BAR_IO_SPACE != 0 {
        return None;
    }

    let memory_type =
        low & PCI_BAR_MEMORY_TYPE_MASK;

    // --------------------------------------------------------
    // 64-bit memory BAR
    // --------------------------------------------------------

    if memory_type == PCI_BAR_MEMORY_TYPE_64 {
        let high = read_u32(
            bus,
            device,
            function,
            PCI_BAR0 + 4,
        );

        let address =
            ((high as u64) << 32)
                | ((low as u64)
                & PCI_BAR_MEMORY_ADDRESS_MASK as u64);

        if address == 0 {
            None
        } else {
            Some(address)
        }
    } else {
        // ----------------------------------------------------
        // 32-bit memory BAR
        // ----------------------------------------------------

        let address =
            (low & PCI_BAR_MEMORY_ADDRESS_MASK)
                as u64;

        if address == 0 {
            None
        } else {
            Some(address)
        }
    }
}

// ============================================================
// Read BAR0 size
// ============================================================
//
// IMPORTANT:
//
// This function is called BEFORE PCI memory decoding and bus
// mastering are enabled.
//
// We temporarily write all ones into the BAR, read the hardware
// mask, then restore the original value.
//
// ============================================================

fn read_bar0_size(
    bus: u8,
    device: u8,
    function: u8,
) -> Option<u64> {
    let original_low = read_u32(
        bus,
        device,
        function,
        PCI_BAR0,
    );

    // BAR must be memory space.
    if original_low & PCI_BAR_IO_SPACE != 0 {
        return None;
    }

    let is_64_bit =
        (original_low & PCI_BAR_MEMORY_TYPE_MASK)
            == PCI_BAR_MEMORY_TYPE_64;

    let original_high =
        if is_64_bit {
            read_u32(
                bus,
                device,
                function,
                PCI_BAR0 + 4,
            )
        } else {
            0
        };

    // --------------------------------------------------------
    // Disable memory decoding temporarily.
    //
    // We also make sure bus mastering is disabled while probing.
    // --------------------------------------------------------

    let original_command = pci_command(
        bus,
        device,
        function,
    );

    let disabled_command =
        original_command
            & !(PCI_COMMAND_MEMORY_SPACE
            | PCI_COMMAND_BUS_MASTER);

    if disabled_command != original_command {
        write_u16(
            bus,
            device,
            function,
            PCI_COMMAND,
            disabled_command,
        );
    }

    // --------------------------------------------------------
    // Probe low dword.
    // --------------------------------------------------------

    write_u32(
        bus,
        device,
        function,
        PCI_BAR0,
        0xFFFF_FFFF,
    );

    let size_low = read_u32(
        bus,
        device,
        function,
        PCI_BAR0,
    );

    // --------------------------------------------------------
    // Probe high dword for 64-bit BAR.
    // --------------------------------------------------------

    let size_high =
        if is_64_bit {
            write_u32(
                bus,
                device,
                function,
                PCI_BAR0 + 4,
                0xFFFF_FFFF,
            );

            let value = read_u32(
                bus,
                device,
                function,
                PCI_BAR0 + 4,
            );

            // Restore high half immediately.
            write_u32(
                bus,
                device,
                function,
                PCI_BAR0 + 4,
                original_high,
            );

            value
        } else {
            0
        };

    // --------------------------------------------------------
    // Restore low half.
    // --------------------------------------------------------

    write_u32(
        bus,
        device,
        function,
        PCI_BAR0,
        original_low,
    );

    // --------------------------------------------------------
    // Restore PCI command register.
    // --------------------------------------------------------

    write_u16(
        bus,
        device,
        function,
        PCI_COMMAND,
        original_command,
    );

    // --------------------------------------------------------
    // Build BAR mask.
    // --------------------------------------------------------

    let mask =
        if is_64_bit {
            ((size_high as u64) << 32)
                | ((size_low as u64)
                & PCI_BAR_MEMORY_ADDRESS_MASK as u64)
        } else {
            (size_low
                & PCI_BAR_MEMORY_ADDRESS_MASK)
                as u64
        };

    if mask == 0
        || mask == u64::MAX
    {
        return None;
    }

    // --------------------------------------------------------
    // Memory BAR size is two's complement of the mask.
    // --------------------------------------------------------

    let size =
        (!mask).wrapping_add(1);

    if size == 0 {
        return None;
    }

    // BAR sizes should be powers of two.
    if !size.is_power_of_two() {
        return None;
    }

    Some(size)
}

// ============================================================
// Configure MSI
// ============================================================
//
// MSI layout:
//
// 32-bit MSI:
//   +0x00 capability ID / next
//   +0x02 message control
//   +0x04 message address low
//   +0x08 message data
//
// 64-bit MSI:
//   +0x00 capability ID / next
//   +0x02 message control
//   +0x04 message address low
//   +0x08 message address high
//   +0x0C message data
//
// The old implementation always wrote message data at +0x08.
// That is wrong for a 64-bit MSI capability.
//
// ============================================================

pub fn setup_msi(
    device: &XhciPciDevice,
    vector: u8,
) {
    let Some(offset) = device.msi_offset else {
        if device.msix_offset.is_some() {
            serial::write_str(
                "PCI: xHCI has MSI-X but MSI is unavailable\n",
            );
        } else {
            serial::write_str(
                "PCI: xHCI has no MSI capability\n",
            );
        }

        return;
    };

    let mut control = read_u16(
        device.bus,
        device.device,
        device.function,
        offset + 0x02,
    );

    // Disable MSI while programming it.
    control &= !MSI_CONTROL_ENABLE;

    write_u16(
        device.bus,
        device.device,
        device.function,
        offset + 0x02,
        control,
    );

    // --------------------------------------------------------
    // MSI message address.
    //
    // x86 Local APIC MSI address:
    //
    //   0xFEE00000
    //
    // CPU destination is zero here.
    // --------------------------------------------------------

    write_u32(
        device.bus,
        device.device,
        device.function,
        offset + 0x04,
        0xFEE0_0000,
    );

    let data_offset =
        if control & MSI_CONTROL_64BIT != 0 {
            // 64-bit MSI:
            // +0x08 is address high.
            write_u32(
                device.bus,
                device.device,
                device.function,
                offset + 0x08,
                0,
            );

            offset + 0x0C
        } else {
            // 32-bit MSI:
            // +0x08 is message data.
            offset + 0x08
        };

    // --------------------------------------------------------
    // MSI message data.
    // --------------------------------------------------------

    write_u16(
        device.bus,
        device.device,
        device.function,
        data_offset,
        vector as u16,
    );

    // --------------------------------------------------------
    // Enable MSI.
    // --------------------------------------------------------

    control |= MSI_CONTROL_ENABLE;

    write_u16(
        device.bus,
        device.device,
        device.function,
        offset + 0x02,
        control,
    );

    // --------------------------------------------------------
    // Verify.
    // --------------------------------------------------------

    let verified = read_u16(
        device.bus,
        device.device,
        device.function,
        offset + 0x02,
    );

    if verified & MSI_CONTROL_ENABLE == 0 {
        serial::write_str(
            "PCI: ERROR: xHCI MSI failed to enable\n",
        );
        return;
    }

    serial::write_str(
        "PCI: MSI enabled for xHCI, vector=",
    );

    serial::write_usize(
        vector as usize,
    );

    serial::write_str(
        "\n",
    );
}

// ============================================================
// Scan PCI bus for xHCI
// ============================================================

pub fn find_xhci()
    -> Option<XhciPciDevice>
{
    for bus in 0..=255u16 {
        for device in 0..32u8 {
            let bus8 = bus as u8;

            // ------------------------------------------------
            // Function 0 vendor.
            // ------------------------------------------------

            let vendor0 = read_u16(
                bus8,
                device,
                0,
                PCI_VENDOR_ID,
            );

            if vendor0 == 0xFFFF {
                continue;
            }

            // ------------------------------------------------
            // Header type.
            // ------------------------------------------------

            let header_type = read_u8(
                bus8,
                device,
                0,
                PCI_HEADER_TYPE,
            );

            let function_count =
                if header_type & 0x80 != 0 {
                    8
                } else {
                    1
                };

            // ------------------------------------------------
            // Enumerate functions.
            // ------------------------------------------------

            for function_index
            in 0..function_count
            {
                let function =
                    function_index as u8;

                let vendor_id = read_u16(
                    bus8,
                    device,
                    function,
                    PCI_VENDOR_ID,
                );

                if vendor_id == 0xFFFF {
                    continue;
                }

                let device_id = read_u16(
                    bus8,
                    device,
                    function,
                    PCI_DEVICE_ID,
                );

                let revision_id = read_u8(
                    bus8,
                    device,
                    function,
                    PCI_REVISION_ID,
                );

                let prog_if = read_u8(
                    bus8,
                    device,
                    function,
                    PCI_PROG_IF,
                );

                let subclass = read_u8(
                    bus8,
                    device,
                    function,
                    PCI_SUBCLASS,
                );

                let class = read_u8(
                    bus8,
                    device,
                    function,
                    PCI_CLASS,
                );

                if class != PCI_CLASS_SERIAL_BUS {
                    continue;
                }

                if subclass != PCI_SUBCLASS_USB {
                    continue;
                }

                if prog_if != PCI_PROGIF_XHCI {
                    continue;
                }

                // ====================================================
                // xHCI found
                // ====================================================

                serial::write_str(
                    "PCI: xHCI controller found\n",
                );

                serial::write_str(
                    "PCI: BDF=",
                );

                serial::write_usize(
                    bus as usize,
                );

                serial::write_str(
                    ":",
                );

                serial::write_usize(
                    device as usize,
                );

                serial::write_str(
                    ".",
                );

                serial::write_usize(
                    function as usize,
                );

                serial::write_str(
                    "\n",
                );

                serial::write_str(
                    "PCI: vendor=",
                );

                serial::write_hex(
                    vendor_id as u64,
                );

                serial::write_str(
                    " device=",
                );

                serial::write_hex(
                    device_id as u64,
                );

                serial::write_str(
                    " revision=",
                );

                serial::write_hex(
                    revision_id as u64,
                );

                serial::write_str(
                    "\n",
                );

                // ------------------------------------------------
                // Read BAR BEFORE enabling PCI memory/DMA.
                // ------------------------------------------------

                let bar0 =
                    match read_bar0(
                        bus8,
                        device,
                        function,
                    ) {
                        Some(value) => value,

                        None => {
                            serial::write_str(
                                "PCI: ERROR: xHCI BAR0 is invalid\n",
                            );

                            continue;
                        }
                    };

                let bar0_size =
                    match read_bar0_size(
                        bus8,
                        device,
                        function,
                    ) {
                        Some(value) => value,

                        None => {
                            serial::write_str(
                                "PCI: ERROR: unable to determine xHCI BAR0 size\n",
                            );

                            continue;
                        }
                    };

                serial::write_str(
                    "PCI: BAR0=",
                );

                serial::write_hex(
                    bar0,
                );

                serial::write_str(
                    " size=",
                );

                serial::write_hex(
                    bar0_size,
                );

                serial::write_str(
                    "\n",
                );

                // ------------------------------------------------
                // Discover capabilities BEFORE enabling MSI.
                // ------------------------------------------------

                let msi_offset =
                    find_capability(
                        bus8,
                        device,
                        function,
                        PCI_CAP_ID_MSI,
                    );

                let msix_offset =
                    find_capability(
                        bus8,
                        device,
                        function,
                        PCI_CAP_ID_MSIX,
                    );

                let xhci = XhciPciDevice {
                    bus: bus8,
                    device,
                    function,

                    vendor_id,
                    device_id,

                    bar0,
                    bar0_size,

                    msi_offset,
                    msix_offset,
                };

                debug_dump_capabilities(
                    &xhci,
                );

                // ------------------------------------------------
                // Now enable MMIO + DMA.
                // ------------------------------------------------

                enable_controller(
                    bus8,
                    device,
                    function,
                );

                // ------------------------------------------------
                // Final PCI state diagnostics.
                // ------------------------------------------------

                let command =
                    pci_command(
                        bus8,
                        device,
                        function,
                    );

                let status =
                    pci_status(
                        bus8,
                        device,
                        function,
                    );

                serial::write_str(
                    "PCI: final COMMAND=",
                );

                serial::write_hex(
                    command as u64,
                );

                serial::write_str(
                    " STATUS=",
                );

                serial::write_hex(
                    status as u64,
                );

                serial::write_str(
                    "\n",
                );

                return Some(xhci);
            }
        }
    }

    serial::write_str(
        "PCI: no xHCI controller found\n",
    );

    None
}

// ============================================================
// Debug dump xHCI root ports
// ============================================================

pub fn debug_dump_ports(
    mmio_base: usize,
) {
    serial::write_str(
        "xHCI: PORT DUMP BEGIN\n",
    );

    // --------------------------------------------------------
    // Capability length
    // --------------------------------------------------------

    let cap_length =
        unsafe {
            core::ptr::read_volatile(
                (mmio_base + 0x00)
                    as *const u8,
            )
        } as usize;

    let op_base =
        mmio_base + cap_length;

    // --------------------------------------------------------
    // HCSPARAMS1
    // --------------------------------------------------------

    let hcsparams1 =
        unsafe {
            core::ptr::read_volatile(
                (mmio_base + 0x04)
                    as *const u32,
            )
        };

    let max_slots =
        (hcsparams1 & 0xFF)
            as usize;

    let max_interrupters =
        ((hcsparams1 >> 8) & 0x7FF)
            as usize;

    let max_ports =
        ((hcsparams1 >> 24) & 0xFF)
            as usize;

    // --------------------------------------------------------
    // HCCPARAMS1
    // --------------------------------------------------------

    let hccparams1 =
        unsafe {
            core::ptr::read_volatile(
                (mmio_base + 0x10)
                    as *const u32,
            )
        };

    let supports_64bit =
        (hccparams1 & (1 << 0)) != 0;

    let context_size_64 =
        (hccparams1 & (1 << 2)) != 0;

    let xecp =
        ((hccparams1 >> 16) as usize)
            << 2;

    // --------------------------------------------------------
    // Operational registers
    // --------------------------------------------------------

    let usbcmd =
        unsafe {
            core::ptr::read_volatile(
                (op_base + 0x00)
                    as *const u32,
            )
        };

    let usbsts =
        unsafe {
            core::ptr::read_volatile(
                (op_base + 0x04)
                    as *const u32,
            )
        };

    let pagesize =
        unsafe {
            core::ptr::read_volatile(
                (op_base + 0x08)
                    as *const u32,
            )
        };

    // --------------------------------------------------------
    // Output controller information
    // --------------------------------------------------------

    serial::write_str(
        "xHCI: BASE=",
    );

    serial::write_hex(
        mmio_base as u64,
    );

    serial::write_str(
        " CAP=",
    );

    serial::write_hex(
        cap_length as u64,
    );

    serial::write_str(
        " OP=",
    );

    serial::write_hex(
        op_base as u64,
    );

    serial::write_str(
        "\n",
    );

    serial::write_str(
        "xHCI: SLOTS=",
    );

    serial::write_usize(
        max_slots,
    );

    serial::write_str(
        " INTERRUPTERS=",
    );

    serial::write_usize(
        max_interrupters,
    );

    serial::write_str(
        " PORTS=",
    );

    serial::write_usize(
        max_ports,
    );

    serial::write_str(
        "\n",
    );

    serial::write_str(
        "xHCI: HCCPARAMS1=",
    );

    serial::write_hex(
        hccparams1 as u64,
    );

    serial::write_str(
        " AC64=",
    );

    serial::write_usize(
        supports_64bit as usize,
    );

    serial::write_str(
        " CSZ64=",
    );

    serial::write_usize(
        context_size_64 as usize,
    );

    serial::write_str(
        " XECP=",
    );

    serial::write_hex(
        xecp as u64,
    );

    serial::write_str(
        "\n",
    );

    serial::write_str(
        "xHCI: USBCMD=",
    );

    serial::write_hex(
        usbcmd as u64,
    );

    serial::write_str(
        " USBSTS=",
    );

    serial::write_hex(
        usbsts as u64,
    );

    serial::write_str(
        " PAGESIZE=",
    );

    serial::write_hex(
        pagesize as u64,
    );

    serial::write_str(
        "\n",
    );

    // ========================================================
    // PORTS
    // ========================================================

    for port_index in 0..max_ports {
        let address =
            op_base
                + 0x400
                + (port_index * 0x10);

        let portsc =
            unsafe {
                core::ptr::read_volatile(
                    address as *const u32,
                )
            };

        let portpmsc =
            unsafe {
                core::ptr::read_volatile(
                    (address + 0x04)
                        as *const u32,
                )
            };

        let portli =
            unsafe {
                core::ptr::read_volatile(
                    (address + 0x08)
                        as *const u32,
                )
            };

        let porthlpmc =
            unsafe {
                core::ptr::read_volatile(
                    (address + 0x0C)
                        as *const u32,
                )
            };

        // ----------------------------------------------------
        // PORTSC fields
        // ----------------------------------------------------

        let ccs =
            (portsc & (1 << 0)) != 0;

        let ped =
            (portsc & (1 << 1)) != 0;

        let pr =
            (portsc & (1 << 4)) != 0;

        let pls =
            (portsc >> 5) & 0xF;

        let pp =
            (portsc & (1 << 9)) != 0;

        let speed =
            (portsc >> 10) & 0xF;

        let csc =
            (portsc & (1 << 17)) != 0;

        let pec =
            (portsc & (1 << 18)) != 0;

        let prc =
            (portsc & (1 << 21)) != 0;

        // ----------------------------------------------------
        // Port output
        // ----------------------------------------------------

        serial::write_str(
            "xHCI: P",
        );

        serial::write_usize(
            port_index + 1,
        );

        serial::write_str(
            " SC=",
        );

        serial::write_hex(
            portsc as u64,
        );

        serial::write_str(
            " PMSC=",
        );

        serial::write_hex(
            portpmsc as u64,
        );

        serial::write_str(
            " LI=",
        );

        serial::write_hex(
            portli as u64,
        );

        serial::write_str(
            " HLPMC=",
        );

        serial::write_hex(
            porthlpmc as u64,
        );

        serial::write_str(
            " | CCS=",
        );

        serial::write_usize(
            ccs as usize,
        );

        serial::write_str(
            " PED=",
        );

        serial::write_usize(
            ped as usize,
        );

        serial::write_str(
            " PR=",
        );

        serial::write_usize(
            pr as usize,
        );

        serial::write_str(
            " PP=",
        );

        serial::write_usize(
            pp as usize,
        );

        serial::write_str(
            " PLS=",
        );

        serial::write_usize(
            pls as usize,
        );

        serial::write_str(
            " SPEED=",
        );

        serial::write_usize(
            speed as usize,
        );

        serial::write_str(
            " CSC=",
        );

        serial::write_usize(
            csc as usize,
        );

        serial::write_str(
            " PEC=",
        );

        serial::write_usize(
            pec as usize,
        );

        serial::write_str(
            " PRC=",
        );

        serial::write_usize(
            prc as usize,
        );

        serial::write_str(
            "\n",
        );
    }

    serial::write_str(
        "xHCI: PORT DUMP END\n",
    );
}