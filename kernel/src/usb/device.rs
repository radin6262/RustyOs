// ============================================================
// USB device enumeration
// ============================================================
//
// A newly attached device is addressed in two phases:
//
//   1. Address Device with BSR=1 keeps it at address 0 so the first eight
//      descriptor bytes can be read (class, max packet size).
//   2. Address Device with BSR=0 sets the real address, the Hub bit and, for
//      LS/FS devices behind a HS hub, the transaction translator fields.
//
// The configuration descriptor then decides which class driver runs: hubs
// recurse into their downstream ports, everything else is offered to the HID
// driver.
// ============================================================

use alloc::vec;
use alloc::vec::Vec;

use crate::serial;
use crate::usb::dma::DmaBuffer;
use crate::usb::hid::{self, HidDevice};
use crate::usb::hub;
use crate::usb::xhci::{DeviceLocation, Error, TtInfo, Xhci};

/// Deepest hub chain the enumerator follows (USB allows five tiers).
const MAX_DEPTH: u8 = 3;

// ============================================================
// Standard requests
// ============================================================

pub const REQUEST_GET_DESCRIPTOR: u8 = 0x06;
pub const REQUEST_SET_CONFIGURATION: u8 = 0x09;
pub const REQUEST_SET_IDLE: u8 = 0x0A;
pub const REQUEST_SET_PROTOCOL: u8 = 0x0B;

pub const DESCRIPTOR_DEVICE: u16 = 0x01;
pub const DESCRIPTOR_CONFIGURATION: u16 = 0x02;
pub const DESCRIPTOR_STRING: u16 = 0x03;
pub const DESCRIPTOR_HUB: u16 = 0x29;
pub const DESCRIPTOR_HID_REPORT: u16 = 0x22;

pub const DEVICE_CLASS_HUB: u8 = 0x09;

/// Build an 8-byte setup packet.
pub fn setup(request_type: u8, request: u8, value: u16, index: u16, length: u16) -> [u8; 8] {
    [
        request_type,
        request,
        value as u8,
        (value >> 8) as u8,
        index as u8,
        (index >> 8) as u8,
        length as u8,
        (length >> 8) as u8,
    ]
}

// ============================================================
// Enumeration result
// ============================================================

/// Every HID interface found under one attachment, plus every slot the
/// attachment consumed (a hub owns one slot, each child owns another).
pub struct Enumerated {
    pub devices: Vec<HidDevice>,
    pub slots: Vec<u8>,
    pub is_hub: bool,
}

impl Enumerated {
    fn empty() -> Self {
        Self {
            devices: Vec::new(),
            slots: Vec::new(),
            is_hub: false,
        }
    }
}

// ============================================================
// Descriptor reads
// ============================================================

fn read_descriptor(
    xhci: &mut Xhci,
    slot: u8,
    request_type: u8,
    descriptor_type: u16,
    index: u16,
    length: usize,
) -> Option<Vec<u8>> {
    if length == 0 || length > 4096 {
        return None;
    }

    let scratch = DmaBuffer::new(4096, 64)?;

    let request = setup(
        request_type,
        REQUEST_GET_DESCRIPTOR,
        descriptor_type << 8,
        index,
        length as u16,
    );

    let actual = match xhci.control(slot, request, Some((&scratch, length))) {
        Ok(actual) => actual,
        Err(error) => {
            log_error("GET_DESCRIPTOR", error);
            return None;
        }
    };

    let actual = actual.min(length);

    if actual == 0 {
        return None;
    }

    let mut descriptor = vec![0u8; actual];
    scratch.read_bytes(0, &mut descriptor);

    Some(descriptor)
}

pub fn read_device_descriptor(xhci: &mut Xhci, slot: u8) -> Option<[u8; 18]> {
    let bytes = read_descriptor(xhci, slot, 0x80, DESCRIPTOR_DEVICE, 0, 18)?;

    if bytes.len() < 8 {
        serial::write_str("USB: device descriptor too short\n");
        return None;
    }

    let mut descriptor = [0u8; 18];
    let length = bytes.len().min(descriptor.len());
    descriptor[..length].copy_from_slice(&bytes[..length]);

    Some(descriptor)
}

fn read_device_descriptor_prefix(xhci: &mut Xhci, slot: u8) -> Option<([u8; 8], u8)> {
    let bytes = read_descriptor(xhci, slot, 0x80, DESCRIPTOR_DEVICE, 0, 8)?;

    if bytes.len() < 8 {
        return None;
    }

    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&bytes[..8]);

    Some((prefix, prefix[7]))
}

pub fn read_configuration(xhci: &mut Xhci, slot: u8) -> Option<(Vec<u8>, u8)> {
    let header = read_descriptor(xhci, slot, 0x80, DESCRIPTOR_CONFIGURATION, 0, 9)?;

    if header.len() < 9 {
        serial::write_str("USB: configuration descriptor too short\n");
        return None;
    }

    let total = u16::from_le_bytes([header[2], header[3]]) as usize;

    if total < 9 || total > 4096 {
        serial::write_str("USB: invalid configuration length\n");
        return None;
    }

    let configuration = read_descriptor(xhci, slot, 0x80, DESCRIPTOR_CONFIGURATION, 0, total)?;

    if configuration.len() < 9 {
        return None;
    }

    Some((configuration, header[5]))
}

pub fn read_report_descriptor(
    xhci: &mut Xhci,
    slot: u8,
    interface: u8,
    length: u16,
) -> Option<Vec<u8>> {
    let length = (length as usize).clamp(1, 4096);

    read_descriptor(
        xhci,
        slot,
        0x81,
        DESCRIPTOR_HID_REPORT,
        interface as u16,
        length,
    )
}

pub fn read_hub_descriptor(xhci: &mut Xhci, slot: u8, length: usize) -> Option<Vec<u8>> {
    read_descriptor(xhci, slot, 0xA0, DESCRIPTOR_HUB, 0, length)
}

pub fn set_configuration(xhci: &mut Xhci, slot: u8, value: u8) -> bool {
    let request = setup(0x00, REQUEST_SET_CONFIGURATION, value as u16, 0, 0);

    match xhci.control(slot, request, None) {
        Ok(_) => true,
        Err(error) => {
            log_error("SET_CONFIGURATION", error);
            false
        }
    }
}

// ============================================================
// Attach
// ============================================================

/// Enable a slot and enumerate a device plugged directly into a root port.
pub fn attach_root(xhci: &mut Xhci, port: u8, speed: u8) -> Enumerated {
    let slot = match xhci.enable_slot() {
        Ok(slot) => slot,
        Err(error) => {
            log_error("Enable Slot", error);
            return Enumerated::empty();
        }
    };

    let location = DeviceLocation {
        root_port: port,
        route: 0,
        tt: None,
    };

    match attach_slot(xhci, slot, location, speed, 0) {
        Some(result) => result,
        None => {
            xhci.disable_slot(slot);
            Enumerated::empty()
        }
    }
}

/// Address one slot and enumerate what is behind it.
pub fn attach_slot(
    xhci: &mut Xhci,
    slot: u8,
    location: DeviceLocation,
    speed: u8,
    depth: u8,
) -> Option<Enumerated> {
    let (device, hub) = address_and_read(xhci, slot, location, speed)?;

    let vendor = u16::from_le_bytes([device[8], device[9]]);
    let product = u16::from_le_bytes([device[10], device[11]]);

    serial::write_str("USB: device vendor=");
    serial::write_hex(vendor as u64);
    serial::write_str(" product=");
    serial::write_hex(product as u64);
    serial::write_str(" class=");
    serial::write_usize(device[4] as usize);
    serial::write_str(" route=");
    serial::write_hex(location.route as u64);
    serial::write_str("\n");

    let (configuration, configuration_value) = read_configuration(xhci, slot)?;

    if !set_configuration(xhci, slot, configuration_value) {
        return None;
    }

    if hub {
        let mut result = Enumerated {
            devices: Vec::new(),
            slots: vec![slot],
            is_hub: true,
        };

        hub::enumerate_children(xhci, slot, location, speed, &device, depth, &mut result);

        return Some(result);
    }

    let devices = hid::enumerate(
        xhci,
        slot,
        location,
        speed,
        &device,
        &configuration,
        configuration_value,
    );

    Some(Enumerated {
        devices,
        slots: vec![slot],
        is_hub: false,
    })
}

/// Two-phase Address Device plus descriptor read.
fn address_and_read(
    xhci: &mut Xhci,
    slot: u8,
    location: DeviceLocation,
    speed: u8,
) -> Option<([u8; 18], bool)> {
    // The default pipe size used before the device descriptor is known.
    let initial_packet: u16 = match speed {
        2 => 8,
        4 | 5 => 512,
        _ => 64,
    };

    if let Err(error) = xhci.address_device(slot, location, speed, initial_packet, false, true) {
        log_error("Address Device (default)", error);
        return None;
    }

    let (prefix, _) = read_device_descriptor_prefix(xhci, slot)?;

    let hub = prefix[4] == DEVICE_CLASS_HUB;

    // bMaxPacketSize0 is an exponent for SuperSpeed devices; full and high
    // speed devices store the byte count directly.
    let max_packet = if speed >= 4 {
        512
    } else {
        (prefix[7] as u16).clamp(8, 64)
    };

    if let Err(error) = xhci.address_device(slot, location, speed, max_packet, hub, false) {
        log_error("Address Device", error);
        return None;
    }

    let device = read_device_descriptor(xhci, slot)?;

    Some((device, hub))
}

// ============================================================
// Helpers
// ============================================================

/// Build the location of a child on a hub port.
pub fn child_location(
    parent: DeviceLocation,
    parent_speed: u8,
    parent_slot: u8,
    port: u8,
    child_speed: u8,
    multi_tt: bool,
) -> DeviceLocation {
    let route = ((parent.route << 4) | port as u32) & 0xF_FFFF;

    let tt = if parent_speed == 3 && child_speed <= 2 {
        Some(TtInfo {
            hub_slot: parent_slot,
            port,
            multi: multi_tt,
        })
    } else {
        None
    };

    DeviceLocation {
        root_port: parent.root_port,
        route,
        tt,
    }
}

pub fn depth_limit_reached(depth: u8) -> bool {
    depth >= MAX_DEPTH
}

pub fn log_error(operation: &str, error: Error) {
    serial::write_str("USB: ");
    serial::write_str(operation);
    serial::write_str(" failed: ");
    serial::write_str(error.name());

    match error {
        Error::Command(code) => {
            serial::write_str(" (command code ");
            serial::write_hex(code as u64);
            serial::write_str(")");
        }
        Error::Transfer(code) => {
            serial::write_str(" (transfer code ");
            serial::write_hex(code as u64);
            serial::write_str(")");
        }
        _ => {}
    }

    serial::write_str("\n");
}
