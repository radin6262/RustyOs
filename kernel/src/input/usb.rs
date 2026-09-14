use core::{
    arch::asm,
    cell::UnsafeCell,
    sync::atomic::{
        AtomicU8,
        Ordering,
    },
};

use xhci_nostd::XhciController;

use super::Key;

// ============================================================
// Controller storage
// ============================================================

struct ControllerStorage {
    controller: UnsafeCell<Option<XhciController>>,
}

unsafe impl Sync for ControllerStorage {}

impl ControllerStorage {
    const fn new() -> Self {
        Self {
            controller: UnsafeCell::new(None),
        }
    }
}

static CONTROLLER: ControllerStorage =
    ControllerStorage::new();

// ============================================================
// State
// ============================================================

static INITIALIZED: AtomicU8 =
    AtomicU8::new(0);

static KEYBOARD_PRESENT: AtomicU8 =
    AtomicU8::new(0);

static LAST_USAGE: AtomicU8 =
    AtomicU8::new(0);

static LAST_PRESSED: AtomicU8 =
    AtomicU8::new(0);

// ============================================================
// PCI I/O
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
        preserves_flags
        )
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
        preserves_flags
        )
        );
    }

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

    unsafe {
        outl(
            0xCF8,
            address,
        );

        inl(0xCFC)
    }
}

unsafe fn pci_write_u32(
    bus: u8,
    device: u8,
    function: u8,
    offset: u8,
    value: u32,
) {
    let address =
        0x8000_0000u32
            | ((bus as u32) << 16)
            | ((device as u32) << 11)
            | ((function as u32) << 8)
            | ((offset as u32) & 0xFC);

    unsafe {
        outl(
            0xCF8,
            address,
        );

        outl(
            0xCFC,
            value,
        );
    }
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

    let shift =
        ((offset & 2) * 8) as u32;

    ((value >> shift) & 0xFFFF)
        as u16
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

    let shift =
        ((offset & 3) * 8) as u32;

    ((value >> shift) & 0xFF)
        as u8
}

// ============================================================
// Find xHCI controller
// ============================================================

/// Find the first USB xHCI controller and return BAR0.
///
/// PCI class:
///     0x0C = Serial Bus Controller
///
/// PCI subclass:
///     0x03 = USB Controller
///
/// Programming interface:
///     0x30 = xHCI
pub fn find_xhci_bar0()
    -> Option<usize>
{
    for bus in 0..=255u16 {
        for device in 0..32u8 {
            let vendor =
                pci_read_u16(
                    bus as u8,
                    device,
                    0,
                    0x00,
                );

            if vendor == 0xFFFF {
                continue;
            }

            let header_type =
                pci_read_u8(
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
                let class =
                    pci_read_u8(
                        bus as u8,
                        device,
                        function,
                        0x0B,
                    );

                let subclass =
                    pci_read_u8(
                        bus as u8,
                        device,
                        function,
                        0x0A,
                    );

                let prog_if =
                    pci_read_u8(
                        bus as u8,
                        device,
                        function,
                        0x09,
                    );

                if class != 0x0C
                    || subclass != 0x03
                    || prog_if != 0x30
                {
                    continue;
                }

                //
                // Enable:
                //
                // bit 1 = Memory Space
                // bit 2 = Bus Master
                //
                let command =
                    pci_read_u16(
                        bus as u8,
                        device,
                        function,
                        0x04,
                    );

                let new_command =
                    command
                        | (1 << 1)
                        | (1 << 2);

                let command_dword =
                    unsafe {
                        pci_read_u32(
                            bus as u8,
                            device,
                            function,
                            0x04,
                        )
                    };

                let updated =
                    (command_dword
                        & 0xFFFF_0000)
                        | new_command as u32;

                unsafe {
                    pci_write_u32(
                        bus as u8,
                        device,
                        function,
                        0x04,
                        updated,
                    );
                }

                //
                // BAR0
                //
                let bar0 =
                    unsafe {
                        pci_read_u32(
                            bus as u8,
                            device,
                            function,
                            0x10,
                        )
                    };

                //
                // Must be a memory BAR.
                //
                if bar0 & 1 != 0 {
                    continue;
                }

                let memory_type =
                    (bar0 >> 1) & 0x03;

                //
                // 64-bit memory BAR.
                //
                if memory_type == 0x02 {
                    let bar1 =
                        unsafe {
                            pci_read_u32(
                                bus as u8,
                                device,
                                function,
                                0x14,
                            )
                        };

                    let address =
                        ((bar1 as u64) << 32)
                            | ((bar0 as u64)
                            & 0xFFFF_FFF0);

                    if address != 0 {
                        return Some(
                            address as usize
                        );
                    }
                } else {
                    //
                    // 32-bit memory BAR.
                    //
                    let address =
                        (bar0
                            & 0xFFFF_FFF0)
                            as usize;

                    if address != 0 {
                        return Some(
                            address
                        );
                    }
                }
            }
        }
    }

    None
}

// ============================================================
// xHCI initialization
// ============================================================

pub unsafe fn init(
    bar0: usize,
) {
    if bar0 == 0 {
        return;
    }

    INITIALIZED.store(
        0,
        Ordering::Release,
    );

    KEYBOARD_PRESENT.store(
        0,
        Ordering::Release,
    );

    LAST_USAGE.store(
        0,
        Ordering::Relaxed,
    );

    LAST_PRESSED.store(
        0,
        Ordering::Relaxed,
    );

    //
    // xhci-nostd expects the controller MMIO address
    // to be directly accessible at the address passed to it.
    //
    // Therefore identity-map the xHCI BAR first.
    //
    crate::memory::identity_map_mmio(
        bar0,
        0x10_0000,
    );

    //
    // NOW initialize xHCI.
    //
    let mut controller =
        unsafe {
            XhciController::init(
                bar0,
            )
        };

    //
    // Enumerate USB devices.
    //
    controller.enumerate_ports();

    //
    // Check for HID keyboard.
    //
    if controller.has_keyboard() {
        KEYBOARD_PRESENT.store(
            1,
            Ordering::Release,
        );
    }

    //
    // Store controller.
    //
    unsafe {
        *CONTROLLER
            .controller
            .get() =
            Some(controller);
    }

    INITIALIZED.store(
        1,
        Ordering::Release,
    );
}

// ============================================================
// Keyboard input
// ============================================================

pub fn read_key() -> Option<Key> {
    if INITIALIZED.load(
        Ordering::Acquire,
    ) == 0 {
        return None;
    }

    let controller = unsafe {
        (*CONTROLLER
            .controller
            .get())
            .as_mut()?
    };

    //
    // xhci-nostd processes the USB HID
    // interrupt transfer and gives us
    // a KeyEvent.
    //
    let event =
        controller.poll_keyboard()?;

    LAST_USAGE.store(
        event.usage_id,
        Ordering::Relaxed,
    );

    LAST_PRESSED.store(
        if event.pressed {
            1
        } else {
            0
        },
        Ordering::Relaxed,
    );

    //
    // Rusty's UI currently wants key-down
    // events, not key-up events.
    //
    if !event.pressed {
        return None;
    }

    usage_to_key(
        event.usage_id,
    )
}

// ============================================================
// HID Usage → Rusty Key
// ============================================================

fn usage_to_key(
    usage: u8,
) -> Option<Key> {
    Some(match usage {
        // ----------------------------------------------------
        // Letters
        // ----------------------------------------------------

        0x04 => Key::Character('a'),
        0x05 => Key::Character('b'),
        0x06 => Key::Character('c'),
        0x07 => Key::Character('d'),
        0x08 => Key::Character('e'),
        0x09 => Key::Character('f'),
        0x0A => Key::Character('g'),
        0x0B => Key::Character('h'),
        0x0C => Key::Character('i'),
        0x0D => Key::Character('j'),
        0x0E => Key::Character('k'),
        0x0F => Key::Character('l'),
        0x10 => Key::Character('m'),
        0x11 => Key::Character('n'),
        0x12 => Key::Character('o'),
        0x13 => Key::Character('p'),
        0x14 => Key::Character('q'),
        0x15 => Key::Character('r'),
        0x16 => Key::Character('s'),
        0x17 => Key::Character('t'),
        0x18 => Key::Character('u'),
        0x19 => Key::Character('v'),
        0x1A => Key::Character('w'),
        0x1B => Key::Character('x'),
        0x1C => Key::Character('y'),
        0x1D => Key::Character('z'),

        // ----------------------------------------------------
        // Numbers
        // ----------------------------------------------------

        0x1E => Key::Character('1'),
        0x1F => Key::Character('2'),
        0x20 => Key::Character('3'),
        0x21 => Key::Character('4'),
        0x22 => Key::Character('5'),
        0x23 => Key::Character('6'),
        0x24 => Key::Character('7'),
        0x25 => Key::Character('8'),
        0x26 => Key::Character('9'),
        0x27 => Key::Character('0'),

        // ----------------------------------------------------
        // Controls
        // ----------------------------------------------------

        0x28 => Key::Enter,
        0x29 => Key::Escape,
        0x2A => Key::Backspace,
        0x2B => Key::Tab,
        0x2C => Key::Space,

        // ----------------------------------------------------
        // Punctuation
        // ----------------------------------------------------

        0x2D => Key::Character('-'),
        0x2E => Key::Character('='),
        0x2F => Key::Character('['),
        0x30 => Key::Character(']'),
        0x31 => Key::Character('\\'),
        0x33 => Key::Character(';'),
        0x34 => Key::Character('\''),
        0x35 => Key::Character('`'),
        0x36 => Key::Character(','),
        0x37 => Key::Character('.'),
        0x38 => Key::Character('/'),

        // ----------------------------------------------------
        // Navigation
        // ----------------------------------------------------

        0x4F => Key::Right,
        0x50 => Key::Left,
        0x51 => Key::Down,
        0x52 => Key::Up,

        _ => return None,
    })
}

// ============================================================
// Diagnostics
// ============================================================

pub fn is_initialized() -> bool {
    INITIALIZED.load(
        Ordering::Acquire,
    ) != 0
}

pub fn has_keyboard() -> bool {
    KEYBOARD_PRESENT.load(
        Ordering::Acquire,
    ) != 0
}

pub fn last_usage() -> u8 {
    LAST_USAGE.load(
        Ordering::Relaxed,
    )
}

pub fn last_pressed() -> bool {
    LAST_PRESSED.load(
        Ordering::Relaxed,
    ) != 0
}