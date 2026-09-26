use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU8, Ordering},
};

use crate::{memory, serial, xhci_pci};

use super::hid::HidManager;
use super::xhci::XhciDriver;

// ============================================================
// USB state
// ============================================================
//
// Rusty's USB subsystem currently contains the custom xHCI driver
// plus the boot-protocol HID layer.
//
// The xHCI driver owns:
//   - controller reset/start state
//   - command/event rings
//   - root-port reset state machines
//   - device/input contexts
//   - synchronous control transfers
//
// The HID layer owns only USB HID enumeration and input decoding.
// ============================================================

pub(crate) struct UsbState {
    pub(crate) xhci: XhciDriver,
    pub(crate) hid: HidManager,
}

// ============================================================
// Global USB storage
// ============================================================

struct UsbStorage {
    state: UnsafeCell<MaybeUninit<UsbState>>,
}

unsafe impl Sync for UsbStorage {}

impl UsbStorage {
    const fn new() -> Self {
        Self {
            state: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
}

static USB_STORAGE: UsbStorage = UsbStorage::new();
static USB_INITIALIZED: AtomicU8 = AtomicU8::new(0);

// ============================================================
// Access USB state
// ============================================================

pub(crate) unsafe fn state_mut() -> &'static mut UsbState {
    if USB_INITIALIZED.load(Ordering::Acquire) == 0 {
        panic!("Rusty: USB state accessed before initialization");
    }

    unsafe { &mut *((*USB_STORAGE.state.get()).as_mut_ptr()) }
}

// ============================================================
// Initialization status
// ============================================================

pub fn is_initialized() -> bool {
    USB_INITIALIZED.load(Ordering::Acquire) != 0
}

// ============================================================
// Root-port constants used only by the initialization layer
// ============================================================

const PORTSC_PLS_MASK: u32 = 0xF << 5;
const PORTSC_PR: u32 = 1 << 4;
const PORTSC_CAS: u32 = 1 << 24;

const PLS_U0: u8 = 0;
const PLS_INACTIVE: u8 = 6;
const PLS_COMPLIANCE: u8 = 10;

// The xHCI driver owns the actual reset deadline. These are supervisory
// deadlines for this higher-level enumeration path, not PORTSC reset timers.
const USB2_PORT_READY_TIMEOUT_US: u64 = 1_000_000;
const USB3_PORT_READY_TIMEOUT_US: u64 = 2_000_000;
const PORT_POLL_INTERVAL_US: u64 = 1_000;

// ============================================================
// Wait for a USB2 root port to become enumeration-ready
// ============================================================
//
// XhciDriver::run() already starts the driver's non-blocking reset state
// machine for newly connected USB2 ports. This helper never writes PORTSC
// directly and never acknowledges PRC itself; it only drives XhciDriver's
// state machine through poll_events().
// ============================================================

unsafe fn wait_for_usb2_port_ready(xhci: &mut XhciDriver, port: usize) -> bool {
    if !xhci.is_usb2_port(port) {
        return false;
    }

    let start_us = crate::delay::now_us();
    let deadline_us = start_us.saturating_add(USB2_PORT_READY_TIMEOUT_US);

    // If run() did not already start a reset, request one here only when the
    // port is connected + disabled + not actively resetting.
    let initial = xhci.port_status(port);
    let initial_connected = (initial & (1 << 0)) != 0;
    let initial_enabled = (initial & (1 << 1)) != 0;
    let initial_resetting = (initial & PORTSC_PR) != 0;

    if !initial_connected {
        return false;
    }

    if !xhci.port_reset_pending(port) && !initial_enabled && !initial_resetting {
        let _ = xhci.request_usb2_port_reset(port);
    }

    loop {
        xhci.poll_events();

        let portsc = xhci.port_status(port);
        let connected = (portsc & (1 << 0)) != 0;
        let enabled = (portsc & (1 << 1)) != 0;
        let resetting = (portsc & PORTSC_PR) != 0;
        let pls = ((portsc & PORTSC_PLS_MASK) >> 5) as u8;

        if !connected {
            serial::write_str(
                "USB: USB2 device disconnected while waiting for port readiness\n",
            );
            return false;
        }

        if !xhci.port_reset_pending(port)
            && enabled
            && !resetting
            && pls == PLS_U0
        {
            serial::write_str("USB: USB2 root port ready on port ");
            serial::write_usize(port);
            serial::write_str(" speed=0x");
            serial::write_hex(xhci.port_speed(port) as u64);
            serial::write_str(" PORTSC=0x");
            serial::write_hex(portsc as u64);
            serial::write_str("\n");
            return true;
        }

        if crate::delay::now_us() >= deadline_us {
            serial::write_str(
                "USB: timed out waiting for USB2 root port readiness port ",
            );
            serial::write_usize(port);
            serial::write_str(" PORTSC=0x");
            serial::write_hex(portsc as u64);
            serial::write_str(" pending=");
            serial::write_usize(xhci.port_reset_pending(port) as usize);
            serial::write_str("\n");
            return false;
        }

        crate::delay::delay_us(PORT_POLL_INTERVAL_US);
    }
}

// ============================================================
// Wait for a USB3 root port to become enumeration-ready
// ============================================================
//
// A healthy USB3 port should reach U0 without a USB2 PORT_RESET. Warm reset is
// requested only for the xHCI/USB3 recovery states exposed by the driver.
// ============================================================

unsafe fn wait_for_usb3_port_ready(xhci: &mut XhciDriver, port: usize) -> bool {
    if !xhci.is_usb3_port(port) {
        return false;
    }

    let start_us = crate::delay::now_us();
    let deadline_us = start_us.saturating_add(USB3_PORT_READY_TIMEOUT_US);

    loop {
        xhci.poll_events();

        let portsc = xhci.port_status(port);
        let connected = (portsc & (1 << 0)) != 0;
        let enabled = (portsc & (1 << 1)) != 0;
        let pls = ((portsc & PORTSC_PLS_MASK) >> 5) as u8;
        let cas = (portsc & PORTSC_CAS) != 0;
        let warm_pending = xhci.usb3_warm_reset_pending(port);

        if !connected && !warm_pending {
            return false;
        }

        if connected && enabled && pls == PLS_U0 && !warm_pending {
            serial::write_str("USB: USB3 root port ready on port ");
            serial::write_usize(port);
            serial::write_str(" speed=0x");
            serial::write_hex(xhci.port_speed(port) as u64);
            serial::write_str(" PORTSC=0x");
            serial::write_hex(portsc as u64);
            serial::write_str("\n");
            return true;
        }

        // Let the driver own USB3 warm-reset semantics. Never issue the USB2
        // PORTSC.PR operation on a USB3 protocol port.
        let recovery_state = cas || pls == PLS_INACTIVE || pls == PLS_COMPLIANCE;

        if recovery_state && !warm_pending {
            serial::write_str("USB: requesting USB3 warm-reset recovery on port ");
            serial::write_usize(port);
            serial::write_str(" PORTSC=0x");
            serial::write_hex(portsc as u64);
            serial::write_str("\n");

            if !xhci.warm_reset_usb3_port(port) {
                serial::write_str(
                    "USB: USB3 warm-reset request rejected by xHCI driver\n",
                );
                return false;
            }

            continue;
        }

        if crate::delay::now_us() >= deadline_us {
            serial::write_str(
                "USB: timed out waiting for USB3 root port readiness port ",
            );
            serial::write_usize(port);
            serial::write_str(" PORTSC=0x");
            serial::write_hex(portsc as u64);
            serial::write_str(" warm_pending=");
            serial::write_usize(warm_pending as usize);
            serial::write_str("\n");
            return false;
        }

        crate::delay::delay_us(PORT_POLL_INTERVAL_US);
    }
}

// ============================================================
// Find one enumeration-ready root port
// ============================================================
//
// Prefer USB2 because the current HID/xHCI endpoint implementation is tested
// primarily against the USB2 boot-HID path. USB3 is still supported when no
// ready USB2 device is present and the controller reports a healthy U0 port.
// ============================================================

unsafe fn find_enumeration_port(xhci: &mut XhciDriver) -> Option<usize> {
    for index in 0..xhci.usb2_ports_count() {
        let port = xhci.usb2_ports[index] as usize;
        if !xhci.port_connected(port) {
            continue;
        }

        if wait_for_usb2_port_ready(xhci, port) {
            return Some(port);
        }
    }

    for index in 0..xhci.usb3_ports_count() {
        let port = xhci.usb3_ports[index] as usize;
        if !xhci.port_connected(port) {
            continue;
        }

        if wait_for_usb3_port_ready(xhci, port) {
            return Some(port);
        }
    }

    None
}

// ============================================================
// Initialize USB
// ============================================================
//
// Lifecycle is intentionally delegated to XhciDriver:
//
//     PCI
//       |
//       v
//     MMIO mapping
//       |
//       v
//     XhciDriver::new()
//       |
//       +--> BIOS/OS legacy handoff
//       |
//       v
//     halt()
//       |
//       v
//     reset()
//       |
//       v
//     init()
//       |
//       v
//     run()
//       |
//       v
//     wait for a ready root port
//       |
//       v
//     Enable Slot
//       |
//       v
//     HID Address Device / control transfers / endpoints
//
// The init layer does not manipulate CRCR, DCBAAP, ERST, ERDP, IMAN, doorbells,
// command TRBs, or event TRBs directly.
// ============================================================

pub fn init() {
    // --------------------------------------------------------
    // Prevent double initialization.
    // --------------------------------------------------------

    if USB_INITIALIZED.load(Ordering::Acquire) != 0 {
        serial::write_str("USB: already initialized\n");
        return;
    }

    crate::delay::init();

    serial::write_str("USB: custom xHCI subsystem initialization started\n");

    // ========================================================
    // Find xHCI controller
    // ========================================================

    serial::write_str("USB: searching for xHCI controller...\n");

    let controller = match xhci_pci::find_xhci() {
        Some(controller) => controller,
        None => {
            serial::write_str("USB: no xHCI controller found\n");
            return;
        }
    };

    serial::write_str("USB: xHCI controller found\n");

    // ========================================================
    // Validate BAR0
    // ========================================================

    serial::write_str("USB: BAR0=");
    serial::write_hex(controller.bar0);
    serial::write_str(" size=");
    serial::write_hex(controller.bar0_size);
    serial::write_str("\n");

    let bar0 = controller.bar0 as usize;
    let bar0_size = controller.bar0_size as usize;

    if bar0 == 0 {
        panic!("Rusty: xHCI BAR0 is zero");
    }

    if bar0_size == 0 {
        panic!("Rusty: xHCI BAR0 size is zero");
    }

    if (bar0 & 0xF) != 0 {
        panic!("Rusty: xHCI BAR0 is not 16-byte aligned");
    }

    if bar0.checked_add(bar0_size).is_none() {
        panic!("Rusty: xHCI BAR0 range overflows usize");
    }

    // ========================================================
    // Map xHCI MMIO
    // ========================================================

    serial::write_str("USB: mapping xHCI MMIO...\n");

    unsafe {
        memory::identity_map_mmio(bar0, bar0_size);
    }

    serial::write_str("USB: xHCI MMIO mapped\n");

    // ========================================================
    // Create xHCI driver
    // ========================================================

    serial::write_str("USB: creating custom xHCI driver...\n");

    let mut xhci = unsafe { XhciDriver::new(bar0) };

    serial::write_str("USB: custom xHCI driver created\n");

    // ========================================================
    // Halt controller
    // ========================================================

    serial::write_str("USB: halting custom xHCI controller...\n");

    unsafe {
        xhci.halt();
    }

    serial::write_str("USB: custom xHCI controller halted\n");

    // ========================================================
    // Reset controller
    // ========================================================

    serial::write_str("USB: resetting custom xHCI controller...\n");

    unsafe {
        xhci.reset();
    }

    serial::write_str("USB: custom xHCI controller reset complete\n");

    // ========================================================
    // Initialize controller structures
    // ========================================================

    serial::write_str("USB: initializing custom xHCI controller...\n");

    unsafe {
        xhci.init();
    }

    serial::write_str("USB: custom xHCI controller initialization complete\n");

    // ========================================================
    // Start controller
    // ========================================================

    serial::write_str("USB: starting custom xHCI controller...\n");

    unsafe {
        xhci.run();
    }

    serial::write_str("USB: custom xHCI controller started\n");

    // ========================================================
    // Verify controller state
    // ========================================================

    let command = xhci.regs.usbcmd();
    let status = xhci.regs.usbsts();

    serial::write_str("USB: post-start USBCMD=0x");
    serial::write_hex(command as u64);
    serial::write_str(" USBSTS=0x");
    serial::write_hex(status as u64);
    serial::write_str("\n");

    if (command & 1) == 0 {
        panic!("Rusty: xHCI RUN bit is not set after start");
    }

    if (status & 1) != 0 {
        panic!("Rusty: xHCI HCH bit is still set after start");
    }

    if (status & (1 << 11)) != 0 {
        panic!("Rusty: xHCI CNR is still set after start");
    }

    // ========================================================
    // Diagnostic state dump
    // ========================================================

    serial::write_str("USB: dumping xHCI controller state...\n");
    xhci.debug_state();
    serial::write_str("USB: xHCI controller state dump complete\n");

    serial::write_str("USB: dumping xHCI PORTSC registers...\n");
    xhci.dump_ports();
    serial::write_str("USB: xHCI PORTSC dump complete\n");

    // ========================================================
    // Give the event consumer one opportunity before port scan.
    // ========================================================

    unsafe {
        xhci.poll_events();
    }

    // ========================================================
    // Prepare HID manager
    // ========================================================

    let mut hid = HidManager::new();

    // ========================================================
    // Find a ready root port
    // ========================================================

    let test_port = unsafe { find_enumeration_port(&mut xhci) };

    let Some(test_port) = test_port else {
        serial::write_str("USB: no enumeration-ready USB root port found\n");

        let hid_has_keyboard = hid.has_keyboard();
        let hid_has_mouse = hid.has_mouse();

        unsafe {
            (*USB_STORAGE.state.get()).write(UsbState { xhci, hid });
        }

        crate::input::set_keyboard_present(hid_has_keyboard);
        crate::input::set_mouse_present(hid_has_mouse);

        USB_INITIALIZED.store(1, Ordering::Release);

        serial::write_str("USB: custom xHCI subsystem initialized\n");
        return;
    };

    serial::write_str("USB: enumeration test using port ");
    serial::write_usize(test_port);
    serial::write_str(" protocol=");
    serial::write_str(if xhci.is_usb2_port(test_port) {
        "USB2"
    } else if xhci.is_usb3_port(test_port) {
        "USB3"
    } else {
        "UNKNOWN"
    });
    serial::write_str(" speed=0x");
    serial::write_hex(xhci.port_speed(test_port) as u64);
    serial::write_str("\n");

    // At this point the xHCI driver has completed its port state machine. Do
    // not clear PRC/WRC or write PORTSC here; those change bits are owned by
    // the driver's root-port state machine.
    let portsc = xhci.port_status(test_port);
    serial::write_str("USB: enumeration-ready PORTSC=0x");
    serial::write_hex(portsc as u64);
    serial::write_str("\n");

    // ========================================================
    // Enable Slot + HID enumeration
    // ========================================================

    serial::write_str("USB: submitting Enable Slot command...\n");

    let slot_id = unsafe { xhci.enable_slot() };

    match slot_id {
        Some(slot_id) => {
            serial::write_str("USB: Enable Slot succeeded, slot=");
            serial::write_hex(slot_id as u64);
            serial::write_str("\n");

            serial::write_str("USB: starting USB HID device enumeration...\n");

            if unsafe { hid.enumerate_device(&mut xhci, test_port, slot_id) } {
                serial::write_str("USB: USB HID enumeration completed\n");
            } else {
                serial::write_str("USB: USB HID enumeration failed or device is not boot HID\n");
            }
        }

        None => {
            // enable_slot() already prints the command/event diagnostics from
            // inside XhciDriver. Do not attempt to manufacture a second
            // command-ring timeout here.
            serial::write_str("USB: Enable Slot failed\n");
        }
    }

    // ========================================================
    // Final diagnostic state
    // ========================================================

    serial::write_str("USB: final xHCI controller state:\n");
    xhci.debug_state();

    serial::write_str("USB: final xHCI port state:\n");
    xhci.dump_ports();

    // ========================================================
    // Publish USB state
    // ========================================================

    let hid_has_keyboard = hid.has_keyboard();
    let hid_has_mouse = hid.has_mouse();

    unsafe {
        (*USB_STORAGE.state.get()).write(UsbState { xhci, hid });
    }

    crate::input::set_keyboard_present(hid_has_keyboard);
    crate::input::set_mouse_present(hid_has_mouse);

    USB_INITIALIZED.store(1, Ordering::Release);

    serial::write_str("USB: custom xHCI subsystem initialized\n");
}
