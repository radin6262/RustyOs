#![allow(dead_code)]

// ============================================================
// PS/2 (i8042) input controller
// ============================================================
//
// Rusty's input stack is the legacy PS/2 path: the kernel talks to the
// i8042 keyboard controller that every PC provides instead of enumerating
// USB HID devices over xHCI.
//
// Layout:
//
//     i8042 -- IRQ1  --> keyboard::handle_scancode()
//           -- IRQ12 --> mouse::handle_byte()
//
// Scancodes and mouse packets are decoded into the same InputEvent queue
// that the compositor and the syscall layer already consume, so nothing
// above this module depends on how the bytes arrived.
//
// All hardware access here uses bounded polling. A missing or wedged
// controller must never hang the boot sequence.
// ============================================================

pub mod keyboard;
pub mod mouse;

use core::arch::asm;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::{input, serial};

// ============================================================
// i8042 ports
// ============================================================

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;

/// Classic "I/O delay" port. Writing to it costs a bus cycle, which the
/// i8042 needs between commands on older machines.
const DELAY_PORT: u16 = 0x80;

// ============================================================
// Status register bits
// ============================================================

const STATUS_OUTPUT_FULL: u8 = 1 << 0;
const STATUS_INPUT_FULL: u8 = 1 << 1;
const STATUS_AUX_OUTPUT: u8 = 1 << 5;

// ============================================================
// Controller commands (written to STATUS_PORT)
// ============================================================

const CMD_READ_CONFIG: u8 = 0x20;
const CMD_WRITE_CONFIG: u8 = 0x60;
const CMD_DISABLE_FIRST_PORT: u8 = 0xAD;
const CMD_ENABLE_FIRST_PORT: u8 = 0xAE;
const CMD_DISABLE_SECOND_PORT: u8 = 0xA7;
const CMD_ENABLE_SECOND_PORT: u8 = 0xA8;
const CMD_SELF_TEST: u8 = 0xAA;
const CMD_TEST_SECOND_PORT: u8 = 0xA9;
const CMD_WRITE_SECOND_PORT: u8 = 0xD4;

// ============================================================
// Device commands / responses (written to DATA_PORT)
// ============================================================

const DEVICE_ACK: u8 = 0xFA;
const DEVICE_SET_DEFAULTS: u8 = 0xF6;
const DEVICE_SET_SAMPLE_RATE: u8 = 0xF3;
const DEVICE_SET_RESOLUTION: u8 = 0xE8;
const DEVICE_GET_ID: u8 = 0xF2;
const DEVICE_ENABLE_REPORTING: u8 = 0xF4;

const CONTROLLER_SELF_TEST_OK: u8 = 0x55;
const PORT_TEST_OK: u8 = 0x00;

/// Device ID reported by an IntelliMouse-compatible pointing device. Such
/// devices send a four byte packet whose last byte is the wheel delta.
const MOUSE_ID_INTELLIMOUSE: u8 = 0x03;

/// Default sample rate: 100 reports per second.
const MOUSE_SAMPLE_RATE: u8 = 100;

/// Resolution: 4 counts per millimetre.
const MOUSE_RESOLUTION: u8 = 0x02;

// ============================================================
// Configuration byte bits
// ============================================================

const CONFIG_FIRST_PORT_IRQ: u8 = 1 << 0;
const CONFIG_SECOND_PORT_IRQ: u8 = 1 << 1;
const CONFIG_FIRST_PORT_CLOCK_DISABLE: u8 = 1 << 4;
const CONFIG_SECOND_PORT_CLOCK_DISABLE: u8 = 1 << 5;
const CONFIG_TRANSLATION: u8 = 1 << 6;

// ============================================================
// Bounded I/O timeouts
// ============================================================

const IO_TIMEOUT: u32 = 100_000;
const DEVICE_RETRIES: usize = 4;
const FLUSH_LIMIT: usize = 64;
const DRAIN_LIMIT: usize = 16;

// ============================================================
// State
// ============================================================

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static KEYBOARD_PRESENT: AtomicBool = AtomicBool::new(false);
static MOUSE_PRESENT: AtomicBool = AtomicBool::new(false);

/// Bytes per mouse packet: 3 for a plain PS/2 mouse, 4 when the wheel
/// extension is active.
static MOUSE_PACKET_SIZE: AtomicU8 = AtomicU8::new(3);

// ============================================================
// Raw port I/O
// ============================================================

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;

    unsafe {
        asm!(
            "in al, dx",
            out("al") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags),
        );
    }

    value
}

#[inline]
unsafe fn outb(port: u16, value: u8) {
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags),
        );
    }
}

#[inline]
unsafe fn io_wait() {
    unsafe {
        outb(DELAY_PORT, 0);
    }
}

#[inline]
unsafe fn status() -> u8 {
    unsafe { inb(STATUS_PORT) }
}

// ============================================================
// Controller primitives
// ============================================================

unsafe fn wait_input_clear() -> bool {
    for _ in 0..IO_TIMEOUT {
        if unsafe { status() } & STATUS_INPUT_FULL == 0 {
            return true;
        }

        core::hint::spin_loop();
    }

    false
}

unsafe fn wait_output_full() -> bool {
    for _ in 0..IO_TIMEOUT {
        if unsafe { status() } & STATUS_OUTPUT_FULL != 0 {
            return true;
        }

        core::hint::spin_loop();
    }

    false
}

unsafe fn write_command(command: u8) -> bool {
    if !unsafe { wait_input_clear() } {
        return false;
    }

    unsafe {
        outb(STATUS_PORT, command);
        io_wait();
    }

    true
}

unsafe fn write_data(value: u8) -> bool {
    if !unsafe { wait_input_clear() } {
        return false;
    }

    unsafe {
        outb(DATA_PORT, value);
        io_wait();
    }

    true
}

/// Reads one byte, waiting for the output buffer to fill. Returns `None`
/// on timeout.
unsafe fn read_data() -> Option<u8> {
    if !unsafe { wait_output_full() } {
        return None;
    }

    Some(unsafe { inb(DATA_PORT) })
}

/// Reads one byte that must have come from the requested port. `aux`
/// selects the second (mouse) port. Bytes belonging to the other port are
/// left in the buffer for the other port's handler.
unsafe fn read_data_for(aux: bool) -> Option<u8> {
    if !unsafe { wait_output_full() } {
        return None;
    }

    if (unsafe { status() } & STATUS_AUX_OUTPUT != 0) != aux {
        return None;
    }

    Some(unsafe { inb(DATA_PORT) })
}

unsafe fn flush_output() {
    for _ in 0..FLUSH_LIMIT {
        if unsafe { status() } & STATUS_OUTPUT_FULL == 0 {
            break;
        }

        let _ = unsafe { inb(DATA_PORT) };

        unsafe {
            io_wait();
        }
    }
}

// ============================================================
// Device commands
// ============================================================

unsafe fn send_keyboard_command(command: u8) -> bool {
    for _ in 0..DEVICE_RETRIES {
        if !unsafe { write_data(command) } {
            return false;
        }

        if unsafe { read_data_for(false) } == Some(DEVICE_ACK) {
            return true;
        }
    }

    false
}

unsafe fn send_mouse_command(command: u8) -> bool {
    for _ in 0..DEVICE_RETRIES {
        if !unsafe { write_command(CMD_WRITE_SECOND_PORT) }
            || !unsafe { write_data(command) }
        {
            return false;
        }

        if unsafe { read_data_for(true) } == Some(DEVICE_ACK) {
            return true;
        }
    }

    false
}

unsafe fn send_mouse_argument(command: u8, argument: u8) -> bool {
    if !unsafe { send_mouse_command(command) } {
        return false;
    }

    for _ in 0..DEVICE_RETRIES {
        if !unsafe { write_command(CMD_WRITE_SECOND_PORT) }
            || !unsafe { write_data(argument) }
        {
            return false;
        }

        if unsafe { read_data_for(true) } == Some(DEVICE_ACK) {
            return true;
        }
    }

    false
}

// ============================================================
// Mouse configuration
// ============================================================
//
// The IntelliMouse extension is unlocked with the magic sample-rate
// sequence 200, 100, 80; afterwards GET ID reports 0x03 and every packet
// carries a wheel byte.
// ============================================================

unsafe fn configure_mouse() -> bool {
    let wheel_unlocked = unsafe {
        send_mouse_argument(DEVICE_SET_SAMPLE_RATE, 200)
            && send_mouse_argument(DEVICE_SET_SAMPLE_RATE, 100)
            && send_mouse_argument(DEVICE_SET_SAMPLE_RATE, 80)
    };

    let device_id = if wheel_unlocked {
        if unsafe { send_mouse_command(DEVICE_GET_ID) } {
            unsafe { read_data_for(true) }
        } else {
            None
        }
    } else {
        None
    };

    let packet_size = if device_id == Some(MOUSE_ID_INTELLIMOUSE) {
        4
    } else {
        3
    };

    MOUSE_PACKET_SIZE.store(packet_size, Ordering::Release);

    serial::write_str("PS/2: mouse device id=");

    match device_id {
        Some(id) => serial::write_hex(id as u64),
        None => serial::write_str("unavailable"),
    }

    serial::write_str(" packet_size=");
    serial::write_usize(packet_size as usize);
    serial::write_str("\n");

    unsafe {
        send_mouse_command(DEVICE_SET_DEFAULTS)
            && send_mouse_argument(DEVICE_SET_SAMPLE_RATE, MOUSE_SAMPLE_RATE)
            && send_mouse_argument(DEVICE_SET_RESOLUTION, MOUSE_RESOLUTION)
            && send_mouse_command(DEVICE_ENABLE_REPORTING)
    }
}

// ============================================================
// Initialization
// ============================================================
//
// Sequence:
//
//     1. disable both ports
//     2. flush stale bytes
//     3. controller self-test
//     4. read/modify/write the configuration byte
//     5. test the second (mouse) port
//     6. enable the ports
//     7. configure the devices
//     8. flush, then enable port interrupts last
//
// The controller self-test resets the configuration byte, so it must run
// before the configuration byte is written.
// ============================================================

pub fn init() {
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        serial::write_str("PS/2: already initialized\n");
        return;
    }

    serial::write_str("PS/2: initializing i8042 controller\n");

    let (keyboard_present, mouse_present) = unsafe { probe() };

    KEYBOARD_PRESENT.store(keyboard_present, Ordering::Release);
    MOUSE_PRESENT.store(mouse_present, Ordering::Release);

    input::set_keyboard_present(keyboard_present);
    input::set_mouse_present(mouse_present);

    serial::write_str("PS/2: keyboard=");
    serial::write_str(if keyboard_present { "detected" } else { "not found" });
    serial::write_str(" mouse=");
    serial::write_str(if mouse_present { "detected" } else { "not found" });
    serial::write_str("\n");

    serial::write_str("PS/2: initialization complete\n");
}

/// Runs the hardware bring-up sequence and reports which devices answered.
unsafe fn probe() -> (bool, bool) {
    let keyboard_present;
    let mut mouse_present = false;

    unsafe {
        // 1. Both ports off while the controller is reconfigured.
        write_command(CMD_DISABLE_FIRST_PORT);
        write_command(CMD_DISABLE_SECOND_PORT);

        // 2. Drop anything the firmware left behind.
        flush_output();

        // 3. Controller self-test.
        if !write_command(CMD_SELF_TEST) {
            serial::write_str("PS/2: controller did not accept self-test\n");
        } else {
            match read_data() {
                Some(CONTROLLER_SELF_TEST_OK) => {
                    serial::write_str("PS/2: controller self-test OK\n");
                }

                other => {
                    serial::write_str("PS/2: WARNING: controller self-test returned ");

                    match other {
                        Some(value) => serial::write_hex(value as u64),
                        None => serial::write_str("no response"),
                    }

                    serial::write_str("\n");
                }
            }
        }

        // 4. Interrupts stay off until both devices are configured, clocks
        //    stay on, and device output is translated to scancode set 1
        //    (the set that ps2::keyboard decodes).
        if !write_command(CMD_READ_CONFIG) {
            serial::write_str("PS/2: controller did not accept READ CONFIG\n");
            return (false, false);
        }

        let mut config = match read_data() {
            Some(value) => value,

            None => {
                serial::write_str("PS/2: controller did not return a config byte\n");
                return (false, false);
            }
        };

        serial::write_str("PS/2: config byte=");
        serial::write_hex(config as u64);
        serial::write_str("\n");

        config &= !(CONFIG_FIRST_PORT_IRQ | CONFIG_SECOND_PORT_IRQ);
        config &= !(CONFIG_FIRST_PORT_CLOCK_DISABLE | CONFIG_SECOND_PORT_CLOCK_DISABLE);
        config |= CONFIG_TRANSLATION;

        if !write_command(CMD_WRITE_CONFIG) || !write_data(config) {
            serial::write_str("PS/2: failed to write the config byte\n");
            return (false, false);
        }

        // 5. Second port test (0x00 means the port passed).
        let second_port_ok = write_command(CMD_TEST_SECOND_PORT)
            && read_data() == Some(PORT_TEST_OK);

        // 6. Bring both ports back up.
        write_command(CMD_ENABLE_FIRST_PORT);

        if second_port_ok {
            write_command(CMD_ENABLE_SECOND_PORT);
        }

        // 7. Devices.
        keyboard_present = send_keyboard_command(DEVICE_SET_DEFAULTS);

        if keyboard_present {
            if !send_keyboard_command(DEVICE_ENABLE_REPORTING) {
                serial::write_str("PS/2: WARNING: keyboard did not accept ENABLE REPORTING\n");
            }
        } else {
            serial::write_str("PS/2: no keyboard responding on the first port\n");
        }

        if second_port_ok {
            mouse_present = send_mouse_command(DEVICE_SET_DEFAULTS);

            if mouse_present {
                if !configure_mouse() {
                    serial::write_str("PS/2: WARNING: mouse configuration incomplete\n");
                }
            } else {
                serial::write_str("PS/2: no mouse responding on the second port\n");
            }
        } else {
            serial::write_str("PS/2: second port test failed\n");
        }

        // 8. Flush any remaining device responses before enabling the
        //    interrupt lines, so they cannot be decoded as input.
        flush_output();

        config |= CONFIG_FIRST_PORT_IRQ;

        if mouse_present {
            config |= CONFIG_SECOND_PORT_IRQ;
        } else {
            config &= !CONFIG_SECOND_PORT_IRQ;
        }

        if !write_command(CMD_WRITE_CONFIG) || !write_data(config) {
            serial::write_str("PS/2: failed to enable port interrupts\n");
        }
    }

    (keyboard_present, mouse_present)
}

// ============================================================
// Interrupt entry points
// ============================================================
//
// Called from the IRQ1 / IRQ12 IDT handlers. Only the i8042 status
// register and data port are touched here; the decoders live in the
// keyboard/mouse submodules.
// ============================================================

pub fn handle_keyboard_interrupt() {
    drain_output();
}

pub fn handle_mouse_interrupt() {
    drain_output();
}

/// Routes every byte the controller is holding to the matching decoder.
/// Both IRQ handlers share this: the status register reports which port a
/// byte belongs to, which keeps a misrouted interrupt harmless.
fn drain_output() {
    for _ in 0..DRAIN_LIMIT {
        let status = unsafe { status() };

        if status & STATUS_OUTPUT_FULL == 0 {
            break;
        }

        let byte = unsafe { inb(DATA_PORT) };

        if status & STATUS_AUX_OUTPUT != 0 {
            mouse::handle_byte(byte);
        } else {
            keyboard::handle_scancode(byte);
        }
    }
}

// ============================================================
// Status
// ============================================================

pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

pub fn has_keyboard() -> bool {
    KEYBOARD_PRESENT.load(Ordering::Acquire)
}

pub fn has_mouse() -> bool {
    MOUSE_PRESENT.load(Ordering::Acquire)
}

pub(crate) fn mouse_packet_size() -> usize {
    MOUSE_PACKET_SIZE.load(Ordering::Acquire) as usize
}
