use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU8, Ordering},
};

use crate::{memory, serial, xhci_pci};

use super::xhci::XhciDriver;

// ============================================================
// USB state
// ============================================================
//
// Rusty's USB subsystem currently contains only our custom
// xHCI driver.
//
// CrabUSB is not used here.
//
// ============================================================

pub(crate) struct UsbState {
    pub(crate) xhci: XhciDriver,
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
// Wait for one USB2 root-port reset
// ============================================================
//
// reset_port() is deliberately non-blocking.  The controller owns the actual
// reset duration; this function simply drives the driver's state machine from
// the normal USB initialization path until the port becomes ready.
//
// It is NEVER called from the xHCI event-consumer path.
//
// ============================================================

fn wait_for_usb2_port_ready(xhci: &mut XhciDriver, port: usize) -> bool {
    const RESET_TIMEOUT_US: u64 = 500_000;
    const POLL_INTERVAL_US: u64 = 1_000;

    let start_us = crate::delay::now_us();
    let deadline_us = start_us.saturating_add(RESET_TIMEOUT_US);

    loop {
        unsafe {
            xhci.poll_events();
        }

        let portsc = xhci.regs.portsc(port);
        let connected = (portsc & (1 << 0)) != 0;
        let enabled = (portsc & (1 << 1)) != 0;
        let reset_active = (portsc & (1 << 4)) != 0;
        let reset_change = (portsc & (1 << 21)) != 0;
        let pls = ((portsc >> 5) & 0xF) as u8;

        if !connected {
            serial::write_str("USB: device disconnected while waiting for port reset\n");
            return false;
        }

        /*
         * The driver clears its pending state only after PR has cleared and
         * the port has reached the expected enabled/U0 state.
         */
        if !xhci.port_reset_pending(port) {
            serial::write_str("USB: USB2 port reset state machine completed on port ");
            serial::write_usize(port);
            serial::write_str("\n");

            serial::write_str("USB: port reset result CCS=");
            serial::write_usize(connected as usize);
            serial::write_str(" PED=");
            serial::write_usize(enabled as usize);
            serial::write_str(" PR=");
            serial::write_usize(reset_active as usize);
            serial::write_str(" PRC=");
            serial::write_usize(reset_change as usize);
            serial::write_str(" PLS=0x");
            serial::write_hex(pls as u64);
            serial::write_str("\n");

            return enabled && !reset_active && pls == 0;
        }

        if crate::delay::now_us() >= deadline_us {
            serial::write_str("USB: timed out waiting for USB2 port reset on port ");
            serial::write_usize(port);
            serial::write_str(" PORTSC=0x");
            serial::write_hex(portsc as u64);
            serial::write_str("\n");
            return false;
        }

        crate::delay::delay_us(POLL_INTERVAL_US);
    }
}

// ============================================================
// Initialize USB
// ============================================================
//
// Initialization sequence:
//
//     PCI
//       |
//       v
//     Find xHCI
//       |
//       v
//     Validate BAR0 + enable/mapped PCI MMIO
//       |
//       v
//     XhciDriver::new()
//       |
//       v
//     XhciDriver::halt()
//       |
//       v
//     XhciDriver::reset()
//       |
//       v
//     XhciDriver::init()
//       |
//       v
//     XhciDriver::run()
//       |
//       v
//     Controller: RS=1 and HCH=0
//       |
//       v
//     Dump PORTSC / capabilities
//       |
//       v
//     Observe connected USB2 root port
//       |
//       v
//     Drive non-blocking port reset state machine
//       |
//       v
//     Clear PRC after interpreting completion
//       |
//       v
//     Enable Slot command
//
// Device-context/address/configuration is NOT implemented yet.
//
// ============================================================

pub fn init() {
    // --------------------------------------------------------
    // Prevent double initialization.
    // --------------------------------------------------------

    if USB_INITIALIZED.load(Ordering::Acquire) != 0 {
        serial::write_str("USB: already initialized\n");
        return;
    }

    /*
     * The custom xHCI driver uses the kernel's calibrated TSC clock for
     * controller and root-port deadlines.  Its initializer is idempotent.
     */
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
    // BAR0
    // ========================================================

    serial::write_str("USB: BAR0=");
    serial::write_hex(controller.bar0);
    serial::write_str(" size=");
    serial::write_hex(controller.bar0_size);
    serial::write_str("\n");

    let bar0 = controller.bar0 as usize;
    let bar0_size = controller.bar0_size as usize;

    // ========================================================
    // Validate BAR0
    // ========================================================

    if bar0 == 0 {
        panic!("Rusty: xHCI BAR0 is zero");
    }

    if bar0_size == 0 {
        panic!("Rusty: xHCI BAR0 size is zero");
    }

    if (bar0 & 0xF) != 0 {
        panic!("Rusty: xHCI BAR0 is not 16-byte aligned");
    }

    /*
     * Sanity-check the BAR range before passing it to the MMIO mapper.
     */
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
    // Create custom xHCI driver
    // ========================================================

    serial::write_str("USB: creating custom xHCI driver...\n");

    let mut xhci = unsafe { XhciDriver::new(bar0) };

    serial::write_str("USB: custom xHCI driver created\n");

    // ========================================================
    // HALT CONTROLLER
    // ========================================================
    //
    // new() allocates/derives the driver's persistent state.  It does NOT
    // replace the explicit Linux-style halt/reset/init/run phases.
    //
    // ========================================================

    serial::write_str("USB: halting custom xHCI controller...\n");

    unsafe {
        xhci.halt();
    }

    serial::write_str("USB: custom xHCI controller halted\n");

    // ========================================================
    // RESET CONTROLLER
    // ========================================================
    //
    // This asserts HCRST, waits for it to self-clear, and then waits for CNR
    // to clear before any operational/runtime programming continues.
    //
    // ========================================================

    serial::write_str("USB: resetting custom xHCI controller...\n");

    unsafe {
        xhci.reset();
    }

    serial::write_str("USB: custom xHCI controller reset complete\n");

    // ========================================================
    // INITIALIZE CONTROLLER
    // ========================================================
    //
    // This is the actual xhci_init() phase:
    //
    //     MAXSLOTSEN
    //     CRCR
    //     DCBAAP
    //     DBOFF software pointer
    //     DNCTRL
    //     Event Ring / ERST / ERDP
    //
    // The controller is still halted during this phase.
    //
    // ========================================================

    serial::write_str("USB: initializing custom xHCI controller...\n");

    unsafe {
        xhci.init();
    }

    serial::write_str("USB: custom xHCI controller initialization complete\n");

    // ========================================================
    // START CONTROLLER
    // ========================================================
    //
    // run() performs the run-finished phase:
    //
    //     CMD_EIE = 1
    //     IMAN.IE = 1
    //     IMAN.IP = 1 (RW1C acknowledge)
    //     USBCMD.RS = 1
    //     wait for USBSTS.HCH = 0
    //
    // This is the point at which the controller actually becomes operational.
    //
    // ========================================================

    serial::write_str("USB: starting custom xHCI controller...\n");

    unsafe {
        xhci.run();
    }

    serial::write_str("USB: custom xHCI controller started\n");

    // ========================================================
    // Verify RUN/HCH state explicitly
    // ========================================================

    let command = xhci.regs.usbcmd();
    let status = xhci.regs.usbsts();

    serial::write_str("USB: post-start USBCMD=0x");
    serial::write_hex(command as u64);
    serial::write_str(" USBSTS=0x");
    serial::write_hex(status as u64);
    serial::write_str("\n");

    if (command & (1 << 0)) == 0 {
        panic!("Rusty: xHCI RUN bit is not set after start");
    }

    if (status & (1 << 0)) != 0 {
        panic!("Rusty: xHCI HCH bit is still set after start");
    }

    if (status & (1 << 11)) != 0 {
        panic!("Rusty: xHCI CNR is still set after start");
    }

    // ========================================================
    // Dump controller state
    // ========================================================

    serial::write_str("USB: dumping xHCI controller state...\n");

    xhci.debug_state();

    serial::write_str("USB: xHCI controller state dump complete\n");

    // ========================================================
    // Dump root ports
    // ========================================================

    serial::write_str("USB: dumping xHCI PORTSC registers...\n");

    xhci.dump_ports();

    serial::write_str("USB: xHCI PORTSC dump complete\n");

    // ========================================================
    // Extended capabilities
    // ========================================================

    serial::write_str("USB: checking xHCI extended capabilities...\n");

    xhci.regs
        .walk_extended_capabilities(|offset, capability_id, next, header| {
            serial::write_str("xHCI: EXT CAP offset=");
            serial::write_hex(offset as u64);

            serial::write_str(" id=");
            serial::write_hex(capability_id as u64);

            serial::write_str(" next=");
            serial::write_hex(next as u64);

            serial::write_str(" header=");
            serial::write_hex(header as u64);

            serial::write_str("\n");

            // ------------------------------------------------
            // USB Legacy Support
            // ------------------------------------------------

            if capability_id == 1 {
                let cap = xhci.regs.extended_capability(offset);

                let bios_owned = (cap & (1 << 16)) != 0;
                let os_owned = (cap & (1 << 24)) != 0;

                serial::write_str("xHCI: USB Legacy Support found\n");

                serial::write_str("xHCI: LEGACY CAP=0x");
                serial::write_hex(cap as u64);
                serial::write_str("\n");

                serial::write_str("xHCI: BIOS Owned=");
                serial::write_usize(bios_owned as usize);

                serial::write_str(" OS Owned=");
                serial::write_usize(os_owned as usize);

                serial::write_str("\n");
            }

            // ------------------------------------------------
            // Supported Protocol
            // ------------------------------------------------

            if capability_id == 2 {
                let revision = xhci.regs.extended_capability(offset);
                let name_string = xhci.regs.extended_capability(offset + 0x04);
                let port_info = xhci.regs.extended_capability(offset + 0x08);

                let major = ((revision >> 24) & 0xFF) as u8;
                let minor = ((revision >> 16) & 0xFF) as u8;
                let port_offset = (port_info & 0xFF) as u8;
                let port_count = ((port_info >> 8) & 0xFF) as u8;

                serial::write_str("xHCI: Supported Protocol\n");

                serial::write_str("xHCI:   revision=0x");
                serial::write_hex(((major as u64) << 8) | minor as u64);
                serial::write_str("\n");

                serial::write_str("xHCI:   name=0x");
                serial::write_hex(name_string as u64);
                serial::write_str("\n");

                serial::write_str("xHCI:   port offset=0x");
                serial::write_hex(port_offset as u64);
                serial::write_str("\n");

                serial::write_str("xHCI:   port count=0x");
                serial::write_hex(port_count as u64);
                serial::write_str("\n");
            }
        });

    serial::write_str("USB: xHCI extended capability scan complete\n");

    // ========================================================
    // Give the controller one event-processing opportunity
    // ========================================================
    //
    // run() already seeds the root-port service state.  Poll once here so
    // any immediately-generated PORTSC or command-completion events are
    // consumed before the explicit test below.
    //
    // ========================================================

    unsafe {
        xhci.poll_events();
    }

    // ========================================================
    // Find connected USB2 root port for the reset test
    // ========================================================
    //
    // Do NOT choose an arbitrary physical PORTSC number.  USB2 and USB3 root
    // hubs are derived from Supported Protocol capabilities, and a USB3
    // physical companion must never receive the ordinary USB2 PORT_RESET path.
    //
    // ========================================================

    serial::write_str("USB: searching for a connected USB2 root port...\n");

    let mut test_port = None;

    for index in 0..xhci.usb2_ports_count() {
        let port = xhci.usb2_ports[index] as usize;
        let portsc = xhci.regs.portsc(port);

        serial::write_str("USB: USB2 port ");
        serial::write_usize(port);
        serial::write_str(" PORTSC=0x");
        serial::write_hex(portsc as u64);
        serial::write_str("\n");

        if xhci.regs.port_connected(port) {
            serial::write_str("USB: connected USB2 device detected on port ");
            serial::write_usize(port);
            serial::write_str("\n");

            test_port = Some(port);
            break;
        }
    }

    // Report connected USB3 ports too, but do not feed them into reset_port().
    for index in 0..xhci.usb3_ports_count() {
        let port = xhci.usb3_ports[index] as usize;
        let portsc = xhci.regs.portsc(port);

        if xhci.regs.port_connected(port) {
            serial::write_str("USB: connected USB3 device detected on port ");
            serial::write_usize(port);
            serial::write_str(" PORTSC=0x");
            serial::write_hex(portsc as u64);
            serial::write_str("; normal USB2 PORT_RESET will NOT be issued\n");
        }
    }

    // ========================================================
    // No connected USB2 device
    // ========================================================

    let Some(test_port) = test_port else {
        serial::write_str("USB: no connected USB2 root port found\n");
        serial::write_str("USB: skipping USB2 enumeration test\n");

        unsafe {
            (*USB_STORAGE.state.get()).write(UsbState { xhci });
        }

        USB_INITIALIZED.store(1, Ordering::Release);

        serial::write_str("USB: custom xHCI subsystem initialized\n");
        return;
    };

    // ========================================================
    // USB2 enumeration test
    // ========================================================

    serial::write_str("USB: enumeration test using USB2 port ");
    serial::write_usize(test_port);
    serial::write_str("\n");

    // ========================================================
    // Read device speed before reset
    // ========================================================

    let before_reset_portsc = xhci.regs.portsc(test_port);
    let before_reset_speed = ((before_reset_portsc >> 10) & 0xF) as u8;

    serial::write_str("USB: device speed before reset=0x");
    serial::write_hex(before_reset_speed as u64);
    serial::write_str("\n");

    // ========================================================
    // Port reset
    // ========================================================
    //
    // run() may already have started a non-blocking reset for this connected
    // USB2 port.  Do not submit a second reset if one is pending.
    //
    // If the port is still connected + disabled + not resetting and no reset
    // is pending, submit one now.
    //
    // ========================================================

    let current_portsc = xhci.regs.portsc(test_port);

    let connected = (current_portsc & (1 << 0)) != 0;
    let enabled = (current_portsc & (1 << 1)) != 0;
    let reset_active = (current_portsc & (1 << 4)) != 0;

    if xhci.port_reset_pending(test_port) {
        serial::write_str("USB: USB2 port reset already pending; waiting for completion...\n");
    } else if connected && !enabled && !reset_active {
        serial::write_str("USB: requesting USB2 port reset...\n");

        unsafe {
            xhci.reset_port(test_port);
        }

        serial::write_str("USB: reset_port() request submitted\n");
    } else if connected && enabled {
        serial::write_str(
            "USB: USB2 port is already enabled; no second PORT_RESET will be issued\n",
        );
    } else {
        serial::write_str("USB: USB2 port is not in a resettable state; skipping request\n");
    }

    // ========================================================
    // Wait for reset state machine to complete
    // ========================================================

    if xhci.port_reset_pending(test_port) {
        let reset_ready = wait_for_usb2_port_ready(&mut xhci, test_port);

        serial::write_str("USB: USB2 port reset wait result=");
        serial::write_usize(reset_ready as usize);
        serial::write_str("\n");
    }

    // ========================================================
    // Inspect PORTSC after reset processing
    // ========================================================

    let portsc = xhci.regs.portsc(test_port);

    serial::write_str("USB: PORTSC after reset processing=0x");
    serial::write_hex(portsc as u64);
    serial::write_str("\n");

    let connected = (portsc & (1 << 0)) != 0;
    let enabled = (portsc & (1 << 1)) != 0;
    let reset_active = (portsc & (1 << 4)) != 0;
    let reset_change = (portsc & (1 << 21)) != 0;
    let speed = ((portsc >> 10) & 0xF) as u8;
    let pls = ((portsc >> 5) & 0xF) as u8;

    serial::write_str("USB: after reset CCS=");
    serial::write_usize(connected as usize);
    serial::write_str(" PED=");
    serial::write_usize(enabled as usize);
    serial::write_str(" PR=");
    serial::write_usize(reset_active as usize);
    serial::write_str(" PRC=");
    serial::write_usize(reset_change as usize);
    serial::write_str(" PLS=0x");
    serial::write_hex(pls as u64);
    serial::write_str(" SPEED=0x");
    serial::write_hex(speed as u64);
    serial::write_str("\n");

    // ========================================================
    // Validate reset result
    // ========================================================

    if reset_active {
        serial::write_str("USB: WARNING: port reset is still active\n");
    }

    if !connected {
        serial::write_str("USB: WARNING: device disconnected during reset\n");
    }

    if !enabled {
        serial::write_str("USB: WARNING: port is not enabled after reset\n");
    }

    if pls != 0 {
        serial::write_str("USB: WARNING: USB2 port is not in U0 after reset\n");
    }

    // ========================================================
    // Clear Port Reset Change
    // ========================================================
    //
    // PRC is RW1C.  We only acknowledge it AFTER interpreting the post-reset
    // PORTSC snapshot.
    //
    // ========================================================

    if reset_change {
        serial::write_str("USB: clearing Port Reset Change...\n");

        unsafe {
            xhci.clear_port_reset_change(test_port);
        }

        serial::write_str("USB: Port Reset Change cleared\n");
    } else {
        serial::write_str("USB: no Port Reset Change bit set\n");
    }

    // ========================================================
    // Enable Slot command
    // ========================================================
    //
    // XhciDriver::enable_slot() submits the command, waits for the matching
    // Command Completion Event, and returns the allocated slot ID.
    //
    // ========================================================

    if connected && enabled && !reset_active {
        serial::write_str("USB: submitting Enable Slot command...\n");

        let slot = unsafe { xhci.enable_slot() };

        match slot {
            Some(slot_id) => {
                serial::write_str("USB: Enable Slot completed, slot=");
                serial::write_hex(slot_id as u64);
                serial::write_str("\n");
            }

            None => {
                serial::write_str("USB: Enable Slot command failed or timed out\n");
            }
        }
    } else {
        serial::write_str("USB: skipping Enable Slot because USB2 port is not ready\n");
    }

    // ========================================================
    // Final controller state
    // ========================================================

    serial::write_str("USB: final xHCI controller state:\n");
    xhci.debug_state();

    serial::write_str("USB: final xHCI port state:\n");
    xhci.dump_ports();

    // ========================================================
    // Publish USB state
    // ========================================================

    unsafe {
        (*USB_STORAGE.state.get()).write(UsbState { xhci });
    }

    USB_INITIALIZED.store(1, Ordering::Release);

    serial::write_str("USB: custom xHCI subsystem initialized\n");
}
