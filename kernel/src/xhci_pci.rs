use core::arch::asm;

use crate::serial;
// ============================================================
// PCI configuration mechanism #1
// ============================================================

const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

// ============================================================
// xHCI PCI class codes
// ============================================================

const PCI_CLASS_SERIAL_BUS: u8 = 0x0C;
const PCI_SUBCLASS_USB: u8 = 0x03;
const PCI_PROGIF_XHCI: u8 = 0x30;

// ============================================================
// PCI command register bits
// ============================================================

const PCI_COMMAND_MEMORY_SPACE: u16 =
    1 << 1;

const PCI_COMMAND_BUS_MASTER: u16 =
    1 << 2;

// ============================================================
// PCI BAR
// ============================================================

const PCI_BAR0: u8 = 0x10;

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
}

// ============================================================
// Low-level PCI I/O
// ============================================================

unsafe fn outl(
    port: u16,
    value: u32,
) {
    unsafe {
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
}

unsafe fn inl(
    port: u16,
) -> u32 {
    let value: u32;

    unsafe {
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
    }

    value
}

// ============================================================
// PCI config access
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

fn read_u32(
    bus: u8,
    device: u8,
    function: u8,
    offset: u8,
) -> u32 {
    let address =
        make_config_address(
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
    let address =
        make_config_address(
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
    let value =
        read_u32(
            bus,
            device,
            function,
            offset & !3,
        );

    let shift =
        ((offset & 2) * 8) as u32;

    ((value >> shift) & 0xFFFF)
        as u16
}

fn write_u16(
    bus: u8,
    device: u8,
    function: u8,
    offset: u8,
    value: u16,
) {
    let aligned =
        offset & !3;

    let mut current =
        read_u32(
            bus,
            device,
            function,
            aligned,
        );

    let shift =
        ((offset & 2) * 8) as u32;

    let mask =
        0xFFFFu32 << shift;

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
    let value =
        read_u32(
            bus,
            device,
            function,
            offset & !3,
        );

    let shift =
        ((offset & 3) * 8) as u32;

    ((value >> shift) & 0xFF)
        as u8
}

// ============================================================
// Read PCI command register
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
        0x04,
    )
}

// ============================================================
// Enable controller
// ============================================================
//
// Memory Space + Bus Mastering are both required for a normal
// PCI xHCI controller operation.
//
// ============================================================

fn enable_controller(
    bus: u8,
    device: u8,
    function: u8,
) {
    let command =
        pci_command(
            bus,
            device,
            function,
        );

    let new_command =
        command
            | PCI_COMMAND_MEMORY_SPACE
            | PCI_COMMAND_BUS_MASTER;

    if new_command
        != command
    {
        write_u16(
            bus,
            device,
            function,
            0x04,
            new_command,
        );
    }

    let verified =
        pci_command(
            bus,
            device,
            function,
        );

    if verified
        & PCI_COMMAND_MEMORY_SPACE
        == 0
    {
        panic!(
            "Rusty: failed to enable PCI memory space for xHCI"
        );
    }

    if verified
        & PCI_COMMAND_BUS_MASTER
        == 0
    {
        panic!(
            "Rusty: failed to enable PCI bus mastering for xHCI"
        );
    }
}

// ============================================================
// Read BAR0 address
// ============================================================

fn read_bar0(
    bus: u8,
    device: u8,
    function: u8,
) -> Option<u64> {
    let low =
        read_u32(
            bus,
            device,
            function,
            PCI_BAR0,
        );

    // --------------------------------------------------------
    // Bit 0 = I/O BAR
    // --------------------------------------------------------

    if low & 1 != 0 {
        return None;
    }

    let memory_type =
        (low >> 1) & 0x03;

    // --------------------------------------------------------
    // 64-bit BAR
    // --------------------------------------------------------

    if memory_type == 0x02 {
        let high =
            read_u32(
                bus,
                device,
                function,
                PCI_BAR0 + 4,
            );

        let address =
            ((high as u64) << 32)
                | ((low as u64)
                & 0xFFFF_FFF0);

        if address == 0 {
            None
        } else {
            Some(address)
        }
    } else {
        // ----------------------------------------------------
        // 32-bit BAR
        // ----------------------------------------------------

        let address =
            (low
                & 0xFFFF_FFF0)
                as u64;

        if address == 0 {
            None
        } else {
            Some(address)
        }
    }
}

// ============================================================
// BAR0 size
// ============================================================
//
// Probe the BAR by writing all ones.
//
// The original value is restored immediately.
//
// ============================================================

fn read_bar0_size(
    bus: u8,
    device: u8,
    function: u8,
) -> Option<u64> {
    let original_low =
        read_u32(
            bus,
            device,
            function,
            PCI_BAR0,
        );

    if original_low & 1 != 0 {
        return None;
    }

    let is_64_bit =
        ((original_low >> 1) & 0x03)
            == 0x02;

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
    // Probe low dword
    // --------------------------------------------------------

    write_u32(
        bus,
        device,
        function,
        PCI_BAR0,
        0xFFFF_FFFF,
    );

    let size_low =
        read_u32(
            bus,
            device,
            function,
            PCI_BAR0,
        );

    // --------------------------------------------------------
    // Probe high dword for 64-bit BAR
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

            let value =
                read_u32(
                    bus,
                    device,
                    function,
                    PCI_BAR0 + 4,
                );

            // Restore high half.

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

    // Restore low half.

    write_u32(
        bus,
        device,
        function,
        PCI_BAR0,
        original_low,
    );

    // --------------------------------------------------------
    // Build mask
    // --------------------------------------------------------

    let mask =
        if is_64_bit {
            ((size_high as u64) << 32)
                | ((size_low as u64)
                & 0xFFFF_FFF0)
        } else {
            (size_low
                & 0xFFFF_FFF0)
                as u64
        };

    if mask == 0 {
        return None;
    }

    // BAR size = two's complement of mask.

    let size =
        (!mask)
            .wrapping_add(1);

    if size == 0 {
        return None;
    }

    Some(size)
}

// ============================================================
// Scan PCI bus
// ============================================================

pub fn find_xhci()
    -> Option<XhciPciDevice>
{
    for bus in 0..=255u16 {
        for device in 0..32u8 {
            // ------------------------------------------------
            // Read function 0 vendor.
            // ------------------------------------------------

            let vendor0 =
                read_u16(
                    bus as u8,
                    device,
                    0,
                    0x00,
                );

            if vendor0 == 0xFFFF {
                continue;
            }

            // ------------------------------------------------
            // Header type.
            // ------------------------------------------------

            let header_type =
                read_u8(
                    bus as u8,
                    device,
                    0,
                    0x0E,
                );

            let function_count =
                if header_type & 0x80 != 0 {
                    8
                } else {
                    1
                };

            for function
            in 0..function_count
            {
                let function =
                    function as u8;

                let vendor_id =
                    read_u16(
                        bus as u8,
                        device,
                        function,
                        0x00,
                    );

                if vendor_id == 0xFFFF {
                    continue;
                }

                let device_id =
                    read_u16(
                        bus as u8,
                        device,
                        function,
                        0x02,
                    );

                let prog_if =
                    read_u8(
                        bus as u8,
                        device,
                        function,
                        0x09,
                    );

                let subclass =
                    read_u8(
                        bus as u8,
                        device,
                        function,
                        0x0A,
                    );

                let class =
                    read_u8(
                        bus as u8,
                        device,
                        function,
                        0x0B,
                    );

                if class
                    != PCI_CLASS_SERIAL_BUS
                {
                    continue;
                }

                if subclass
                    != PCI_SUBCLASS_USB
                {
                    continue;
                }

                if prog_if
                    != PCI_PROGIF_XHCI
                {
                    continue;
                }

                // ------------------------------------------------
                // We found xHCI.
                // ------------------------------------------------

                crate::serial::write_str(
                    "PCI: xHCI controller found\n",
                );

                crate::serial::write_str(
                    "PCI: bus=",
                );

                crate::serial::write_usize(
                    bus as usize,
                );

                crate::serial::write_str(
                    " device=",
                );

                crate::serial::write_usize(
                    device as usize,
                );

                crate::serial::write_str(
                    " function=",
                );

                crate::serial::write_usize(
                    function as usize,
                );

                crate::serial::write_str(
                    "\n",
                );

                crate::serial::write_str(
                    "PCI: vendor=",
                );

                crate::serial::write_hex(
                    vendor_id as u64,
                );

                crate::serial::write_str(
                    " device=",
                );

                crate::serial::write_hex(
                    device_id as u64,
                );

                crate::serial::write_str(
                    "\n",
                );

                // ------------------------------------------------
                // Enable PCI controller.
                // ------------------------------------------------

                enable_controller(
                    bus as u8,
                    device,
                    function,
                );

                // ------------------------------------------------
                // Read BAR0.
                // ------------------------------------------------

                let bar0 =
                    read_bar0(
                        bus as u8,
                        device,
                        function,
                    )?;

                let bar0_size =
                    read_bar0_size(
                        bus as u8,
                        device,
                        function,
                    )?;

                crate::serial::write_str(
                    "PCI: xHCI BAR0=",
                );

                crate::serial::write_hex(
                    bar0,
                );

                crate::serial::write_str(
                    " size=",
                );

                crate::serial::write_hex(
                    bar0_size,
                );

                crate::serial::write_str(
                    "\n",
                );

                return Some(
                    XhciPciDevice {
                        bus:
                        bus as u8,

                        device,

                        function,

                        vendor_id,

                        device_id,

                        bar0,

                        bar0_size,
                    },
                );
            }
        }
    }

    None
}

pub fn debug_dump_ports(
    mmio_base: usize,
) {
    serial::write_str(
        "xHCI: PORT DUMP BEGIN\n",
    );

    let cap_length =
        unsafe {
            core::ptr::read_volatile(
                (mmio_base + 0x00)
                    as *const u8,
            )
        } as usize;

    let op_base =
        mmio_base + cap_length;

    let hcsparams1 =
        unsafe {
            core::ptr::read_volatile(
                (mmio_base + 0x04)
                    as *const u32,
            )
        };

    let max_ports =
        ((hcsparams1 >> 24) & 0xFF)
            as usize;

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
        " PORTS=",
    );
    serial::write_usize(
        max_ports,
    );
    serial::write_str(
        "\n",
    );

    for port in 0..max_ports {
        let address =
            op_base
                + 0x400
                + (port * 0x10);

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

        serial::write_str(
            "xHCI: P",
        );
        serial::write_usize(
            port + 1,
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