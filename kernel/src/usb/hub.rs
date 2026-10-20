// ============================================================
// USB hub class driver
// ============================================================
//
// Supports external USB 2.0 hubs (full or high speed). Downstream devices
// are enumerated recursively: the hub's ports are powered and reset, each
// child is addressed through its route string, and the child is handed back
// to the generic enumerator (which may find another hub).
//
// USB 3.x hubs use a different port-status layout and are rejected for now.
// ============================================================

use crate::delay;
use crate::serial;
use crate::usb::dma::DmaBuffer;
use crate::usb::device::{self, Enumerated};
use crate::usb::xhci::Xhci;

// ============================================================
// Hub class requests and port bits
// ============================================================

const REQUEST_GET_STATUS: u8 = 0x00;
const REQUEST_SET_FEATURE: u8 = 0x03;
const REQUEST_CLEAR_FEATURE: u8 = 0x01;

const PORT_CONNECTION: u16 = 1 << 0;
const PORT_ENABLE: u16 = 1 << 1;
const PORT_LOW_SPEED: u16 = 1 << 9;
const PORT_HIGH_SPEED: u16 = 1 << 10;

const CHANGE_CONNECTION: u16 = 1 << 0;
const CHANGE_RESET: u16 = 1 << 4;

const FEATURE_PORT_RESET: u16 = 4;
const FEATURE_PORT_POWER: u16 = 8;
const FEATURE_CHANGE_CONNECTION: u16 = 16;
const FEATURE_CHANGE_RESET: u16 = 20;

const PORT_RESET_TIMEOUT_US: u64 = 500_000;

// ============================================================
// Port status helpers
// ============================================================

fn get_port_status(
    xhci: &mut Xhci,
    slot: u8,
    port: u8,
    scratch: &DmaBuffer,
) -> Option<(u16, u16)> {
    let request = device::setup(0xA3, REQUEST_GET_STATUS, 0, port as u16, 4);

    xhci.control(slot, request, Some((scratch, 4))).ok()?;

    let mut bytes = [0u8; 4];
    scratch.read_bytes(0, &mut bytes);

    Some((
        u16::from_le_bytes([bytes[0], bytes[1]]),
        u16::from_le_bytes([bytes[2], bytes[3]]),
    ))
}

fn set_port_feature(xhci: &mut Xhci, slot: u8, port: u8, feature: u16) -> bool {
    let request = device::setup(0x23, REQUEST_SET_FEATURE, feature, port as u16, 0);

    xhci.control(slot, request, None).is_ok()
}

fn clear_port_feature(xhci: &mut Xhci, slot: u8, port: u8, feature: u16) {
    let request = device::setup(0x23, REQUEST_CLEAR_FEATURE, feature, port as u16, 0);

    let _ = xhci.control(slot, request, None);
}

// ============================================================
// Hub enumeration
// ============================================================

pub fn enumerate_children(
    xhci: &mut Xhci,
    slot: u8,
    location: crate::usb::xhci::DeviceLocation,
    speed: u8,
    device_descriptor: &[u8],
    depth: u8,
    result: &mut Enumerated,
) {
    if device::depth_limit_reached(depth) {
        serial::write_str("USB: hub depth limit reached\n");
        return;
    }

    if speed >= 4 {
        serial::write_str("USB: USB3 hubs are not supported\n");
        return;
    }

    let Some(descriptor) = device::read_hub_descriptor(xhci, slot, 16) else {
        serial::write_str("USB: failed to read hub descriptor\n");
        return;
    };

    if descriptor.len() < 7 {
        serial::write_str("USB: hub descriptor too short\n");
        return;
    }

    // A route string gives each hub tier four bits, so ports above 15
    // cannot be represented.
    let ports = descriptor[2].min(15);
    let characteristics = u16::from_le_bytes([descriptor[3], descriptor[4]]);
    let power_on_delay = descriptor[5];

    // Device descriptor byte 6 is bDeviceProtocol: 0 = full-speed hub,
    // 1 = high-speed single TT, 2 = high-speed multi TT.
    let protocol = device_descriptor.get(6).copied().unwrap_or(0);
    let multi_tt = protocol == 2;

    serial::write_str("USB: hub slot=");
    serial::write_usize(slot as usize);
    serial::write_str(" ports=");
    serial::write_usize(ports as usize);
    serial::write_str(" protocol=");
    serial::write_usize(protocol as usize);
    serial::write_str("\n");

    // --------------------------------------------------------
    // Port power
    // --------------------------------------------------------

    let power_mode = characteristics & 0x3;

    if power_mode != 2 {
        if power_mode == 0 {
            let _ = set_port_feature(xhci, slot, 1, FEATURE_PORT_POWER);
        } else {
            for port in 1..=ports {
                let _ = set_port_feature(xhci, slot, port, FEATURE_PORT_POWER);
            }
        }

        let delay_ms = (power_on_delay as u64).saturating_mul(2).max(10);
        delay::delay_ms(delay_ms);
    }

    let Some(scratch) = DmaBuffer::new(16, 64) else {
        return;
    };

    // --------------------------------------------------------
    // Downstream ports
    // --------------------------------------------------------

    for port in 1..=ports {
        let Some((status, _)) = get_port_status(xhci, slot, port, &scratch) else {
            continue;
        };

        if status & PORT_CONNECTION == 0 {
            continue;
        }

        if !set_port_feature(xhci, slot, port, FEATURE_PORT_RESET) {
            serial::write_str("USB: hub port reset request failed\n");
            continue;
        }

        let deadline = delay::now_us().saturating_add(PORT_RESET_TIMEOUT_US);
        let mut reset_done = false;

        while delay::now_us() < deadline {
            let Some((status, change)) = get_port_status(xhci, slot, port, &scratch) else {
                break;
            };

            if change & CHANGE_RESET != 0 || status & PORT_ENABLE != 0 {
                reset_done = true;
                break;
            }

            delay::delay_ms(10);
        }

        clear_port_feature(xhci, slot, port, FEATURE_CHANGE_RESET);
        clear_port_feature(xhci, slot, port, FEATURE_CHANGE_CONNECTION);

        if !reset_done {
            serial::write_str("USB: hub port reset timed out\n");
            continue;
        }

        // Reset recovery time.
        delay::delay_ms(10);

        let Some((status, _)) = get_port_status(xhci, slot, port, &scratch) else {
            continue;
        };

        if status & PORT_CONNECTION == 0 {
            continue;
        }

        let child_speed = if status & PORT_LOW_SPEED != 0 {
            2
        } else if status & PORT_HIGH_SPEED != 0 {
            3
        } else {
            1
        };

        let child_location =
            device::child_location(location, speed, slot, port, child_speed, multi_tt);

        serial::write_str("USB: hub port ");
        serial::write_usize(port as usize);
        serial::write_str(" device, speed=");
        serial::write_usize(child_speed as usize);
        serial::write_str(" route=");
        serial::write_hex(child_location.route as u64);
        serial::write_str("\n");

        let child_slot = match xhci.enable_slot() {
            Ok(slot) => slot,
            Err(error) => {
                device::log_error("Enable Slot (hub child)", error);
                continue;
            }
        };

        match device::attach_slot(xhci, child_slot, child_location, child_speed, depth + 1) {
            Some(child) => {
                result.devices.extend(child.devices);
                result.slots.extend(child.slots);
            }
            None => {
                xhci.disable_slot(child_slot);
            }
        }
    }
}
