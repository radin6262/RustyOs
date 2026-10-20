#![allow(dead_code)]

// ============================================================
// USB subsystem
// ============================================================
//
// Layers:
//
//     crate::usb::pci      find xHCI controllers and map their BARs
//     crate::usb::xhci     controller, rings, ports, slots, transfers
//     crate::usb::hid      HID boot enumeration and report polling
//     crate::usb::keyboard HID usage -> crate::input::Key decoding
//
// Every xHCI controller found on the PCI bus is driven independently. The
// driver runs in polled mode: crate::usb::poll() is called from the syscall
// entry path, drains each controller's event ring, decodes completed HID
// reports and re-arms one interrupt transfer per device. Decoded input is
// pushed into the same crate::input queues used by the PS/2 driver, so the
// compositor and applications do not know which backend produced an event.
// ============================================================

pub mod device;
pub mod dma;
pub mod hid;
pub mod hub;
pub mod keyboard;
pub mod pci;
pub mod report;
pub mod xhci;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use spin::Mutex;

use crate::serial;

use hid::{HidDevice, HidKind};
use xhci::trb::{Trb, TRB_PORT_STATUS_CHANGE, TRB_TRANSFER_EVENT};
use xhci::Xhci;

// ============================================================
// Global state
// ============================================================

/// One xHCI controller with the devices it owns.
struct ControllerState {
    xhci: Xhci,
    devices: Vec<HidDevice>,
    /// Every slot owned by this controller, with the root port it belongs to.
    slots: Vec<(u8, u8)>,
    /// Ports whose status changed and that need to be re-examined.
    rescan: [bool; 256],
}

struct UsbState {
    controllers: Vec<ControllerState>,
}

static USB_STATE: Mutex<Option<UsbState>> = Mutex::new(None);
static INITIALIZED: AtomicBool = AtomicBool::new(false);

// ============================================================
// Initialization
// ============================================================

pub fn init() {
    if INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let Some(state) = initialize() else {
        return;
    };

    let (keyboards, mice, controllers) = state.counts();

    state.refresh_presence();

    *USB_STATE.lock() = Some(state);
    INITIALIZED.store(true, Ordering::Release);

    serial::write_str("USB: subsystem initialized, keyboards=");
    serial::write_usize(keyboards);
    serial::write_str(" mice=");
    serial::write_usize(mice);
    serial::write_str(" controllers=");
    serial::write_usize(controllers);
    serial::write_str("\n");
}

fn initialize() -> Option<UsbState> {
    let discovered = pci::find_all_xhci();

    if discovered.is_empty() {
        serial::write_str("USB: no xHCI controller found\n");
        return None;
    }

    serial::write_str("USB: xHCI controllers found=");
    serial::write_usize(discovered.len());
    serial::write_str("\n");

    let mut controllers = Vec::new();

    for controller_pci in discovered.iter() {
        serial::write_str("USB: xHCI controller at ");
        serial::write_usize(controller_pci.bus as usize);
        serial::write_str(":");
        serial::write_usize(controller_pci.device as usize);
        serial::write_str(".");
        serial::write_usize(controller_pci.function as usize);
        serial::write_str(" BAR0=");
        serial::write_hex(controller_pci.mmio_base as u64);
        serial::write_str("\n");

        let controller = match Xhci::new(controller_pci) {
            Ok(controller) => controller,
            Err(error) => {
                serial::write_str("USB: xHCI initialization failed: ");
                serial::write_str(error.name());
                serial::write_str("\n");
                continue;
            }
        };

        controllers.push(ControllerState::bring_up(controller));
    }

    if controllers.is_empty() {
        return None;
    }

    Some(UsbState {
        controllers,
    })
}

// ============================================================
// Polling
// ============================================================

pub fn poll() {
    if !INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let mut guard = USB_STATE.lock();

    if let Some(state) = guard.as_mut() {
        state.poll();
    }
}

impl UsbState {
    fn poll(&mut self) {
        for controller in self.controllers.iter_mut() {
            controller.poll();
        }

        // Presence is global: a USB keyboard on any controller is enough.
        self.refresh_presence();
    }

    fn counts(&self) -> (usize, usize, usize) {
        let mut keyboards = 0;
        let mut mice = 0;

        for controller in self.controllers.iter() {
            for device in controller.devices.iter() {
                match device.kind {
                    HidKind::Keyboard => keyboards += 1,
                    HidKind::Mouse => mice += 1,
                }
            }
        }

        (keyboards, mice, self.controllers.len())
    }

    fn refresh_presence(&self) {
        let usb_keyboard = self
            .controllers
            .iter()
            .flat_map(|controller| controller.devices.iter())
            .any(|device| device.kind == HidKind::Keyboard);

        let usb_mouse = self
            .controllers
            .iter()
            .flat_map(|controller| controller.devices.iter())
            .any(|device| device.kind == HidKind::Mouse);

        crate::input::set_keyboard_present(
            usb_keyboard || crate::input::ps2::has_keyboard(),
        );

        crate::input::set_mouse_present(usb_mouse || crate::input::ps2::has_mouse());
    }
}

impl ControllerState {
    /// Scan the controller's root ports and enumerate every HID device.
    fn bring_up(xhci: Xhci) -> Self {
        let mut state = Self {
            xhci,
            devices: Vec::new(),
            slots: Vec::new(),
            rescan: [false; 256],
        };

        for port in 1..=state.xhci.max_ports() {
            if !state.xhci.port_connected(port) {
                continue;
            }

            let usb3 = state.xhci.is_usb3_port(port);

            serial::write_str("USB: port ");
            serial::write_usize(port as usize);
            serial::write_str(" connected (");
            serial::write_str(if usb3 {
                "USB3"
            } else if state.xhci.is_usb2_port(port) {
                "USB2"
            } else {
                "unknown"
            });
            serial::write_str(") PORTSC=");
            serial::write_hex(state.xhci.port_debug(port) as u64);
            serial::write_str("\n");

            let speed = match state.xhci.reset_port(port, usb3) {
                Ok(speed) => speed,
                Err(error) => {
                    serial::write_str("USB: port ");
                    serial::write_usize(port as usize);
                    serial::write_str(" reset failed: ");
                    serial::write_str(error.name());
                    serial::write_str("\n");
                    continue;
                }
            };

            serial::write_str("USB: port ");
            serial::write_usize(port as usize);
            serial::write_str(" ready, speed=");
            serial::write_usize(speed as usize);
            serial::write_str("\n");

            let result = device::attach_root(&mut state.xhci, port, speed);

            state.register(result, port);
        }

        state
    }

    /// Record slots and HID interfaces, or release a slot whose device this
    /// driver cannot use (for example a storage stick).
    fn register(&mut self, result: device::Enumerated, port: u8) {
        if !result.is_hub && result.devices.is_empty() {
            for slot in result.slots {
                self.xhci.disable_slot(slot);
            }

            return;
        }

        for slot in result.slots {
            self.slots.push((slot, port));
        }

        self.devices.extend(result.devices);
    }

    fn poll(&mut self) {
        loop {
            let Some(event) = self.xhci.pop_event() else {
                break;
            };

            self.handle_event(event);
        }

        self.process_rescans();
        self.rearm();
    }

    fn handle_event(&mut self, event: Trb) {
        match event.trb_type() {
            TRB_TRANSFER_EVENT => {
                let slot = event.slot_id();
                let dci = event.endpoint_id();

                for device in self.devices.iter_mut() {
                    if device.slot != slot || device.dci != dci {
                        continue;
                    }

                    if device.in_flight == Some(event.event_pointer()) {
                        device.complete(event);
                    } else {
                        // A completion for a transfer this driver no longer
                        // tracks. Drop it so the endpoint can be re-armed.
                        device.in_flight = None;
                    }

                    break;
                }
            }

            TRB_PORT_STATUS_CHANGE => {
                let port = event.port_id();

                if port != 0 {
                    self.rescan[port as usize] = true;
                }
            }

            _ => {}
        }
    }

    /// Bring newly attached devices up and release devices that disappeared.
    fn process_rescans(&mut self) {
        for port in 1..=self.xhci.max_ports() {
            if !self.rescan[port as usize] {
                continue;
            }

            self.rescan[port as usize] = false;

            if !self.xhci.port_connected(port) {
                self.remove_port(port);
                continue;
            }

            if self.slots.iter().any(|(_, root_port)| *root_port == port) {
                // The port already has a device tree this driver owns.
                // Ignore the duplicate change notification.
                continue;
            }

            let usb3 = self.xhci.is_usb3_port(port);

            serial::write_str("USB: new device on port ");
            serial::write_usize(port as usize);
            serial::write_str("\n");

            let speed = match self.xhci.reset_port(port, usb3) {
                Ok(speed) => speed,
                Err(error) => {
                    serial::write_str("USB: hotplug port reset failed: ");
                    serial::write_str(error.name());
                    serial::write_str("\n");
                    continue;
                }
            };

            let result = device::attach_root(&mut self.xhci, port, speed);

            self.register(result, port);
        }
    }

    fn remove_port(&mut self, port: u8) {
        let mut removed_slots = Vec::new();

        let mut index = 0;

        while index < self.slots.len() {
            if self.slots[index].1 == port {
                removed_slots.push(self.slots[index].0);
                self.slots.remove(index);
            } else {
                index += 1;
            }
        }

        self.devices.retain(|device| device.port != port);

        // Children were allocated after their hub, so disable in reverse
        // order: endpoints first, hub slot last.
        for slot in removed_slots.iter().rev() {
            // Best effort: the device may already be gone from the bus.
            self.xhci.disable_slot(*slot);
        }

        if !removed_slots.is_empty() {
            serial::write_str("USB: device removed from port ");
            serial::write_usize(port as usize);
            serial::write_str("\n");
        }
    }

    fn rearm(&mut self) {
        let controller = &mut self.xhci;

        for device in self.devices.iter_mut() {
            if device.in_flight.is_some() {
                continue;
            }

            if let Err(error) = device.queue(controller) {
                serial::write_str("USB: failed to re-arm interrupt endpoint: ");
                serial::write_str(error.name());
                serial::write_str("\n");
            }
        }
    }
}
