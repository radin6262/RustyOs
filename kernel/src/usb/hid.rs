// ============================================================
// USB HID keyboards and mice
// ============================================================
//
// A device may expose any number of HID interfaces. Each interface is set up
// independently:
//
//   - boot interfaces (class 3, subclass 1, protocol 1/2) are switched to
//     boot protocol and use the fixed 8-byte keyboard / 3-4 byte mouse
//     reports;
//   - other HID interfaces are read through their report descriptor and
//     decoded by crate::usb::report.
//
// Every interface gets its own interrupt IN ring and report buffer. All of
// them share the device's slot and default control pipe.
// ============================================================

use alloc::vec::Vec;

use crate::serial;
use crate::usb::device;
use crate::usb::dma::DmaBuffer;
use crate::usb::keyboard;
use crate::usb::report::{self, DecodeState, ReportLayout};
use crate::usb::xhci::ring::TrbRing;
use crate::usb::xhci::trb::{Trb, CC_SHORT_PACKET, CC_SUCCESS};
use crate::usb::xhci::{DeviceLocation, Result, Xhci};

// ============================================================
// Device kinds
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HidKind {
    Keyboard,
    Mouse,
}

impl HidKind {
    fn name(self) -> &'static str {
        match self {
            HidKind::Keyboard => "keyboard",
            HidKind::Mouse => "mouse",
        }
    }
}

// ============================================================
// Report decoding mode
// ============================================================

enum Mode {
    BootKeyboard {
        keys: [u8; 6],
        modifiers: u8,
    },
    BootMouse,
    Descriptor {
        layout: ReportLayout,
        state: DecodeState,
    },
}

impl Mode {
    fn name(&self) -> &'static str {
        match self {
            Mode::BootKeyboard { .. } | Mode::BootMouse => "boot",
            Mode::Descriptor { .. } => "report",
        }
    }
}

// ============================================================
// Device state
// ============================================================

pub struct HidDevice {
    pub slot: u8,
    pub port: u8,
    pub route: u32,
    pub interface: u8,
    pub dci: u8,
    pub kind: HidKind,
    pub max_packet: u16,
    pub transfer_length: usize,
    pub report: DmaBuffer,
    pub ring: TrbRing,
    pub in_flight: Option<u64>,
    mode: Mode,
    logged_first_report: bool,
}

impl HidDevice {
    /// Publish the next interrupt IN transfer for this interface.
    pub fn queue(&mut self, xhci: &mut Xhci) -> Result<()> {
        let address = xhci.queue_interrupt_in(
            self.slot,
            self.dci,
            &mut self.ring,
            &self.report,
            self.transfer_length,
        )?;

        self.in_flight = Some(address);

        Ok(())
    }

    /// Handle a Transfer Event for this interface's interrupt endpoint.
    pub fn complete(&mut self, event: Trb) {
        self.in_flight = None;

        let code = event.completion_code();

        if code != CC_SUCCESS && code != CC_SHORT_PACKET {
            serial::write_str("USB: ");
            serial::write_str(self.kind.name());
            serial::write_str(" transfer failed, code=");
            serial::write_hex(code as u64);
            serial::write_str("\n");
            return;
        }

        let actual = self
            .transfer_length
            .saturating_sub(event.residual() as usize);

        if actual == 0 {
            return;
        }

        let mut report = [0u8; 64];
        let length = actual.min(report.len()).min(self.report.size());

        self.report.read_bytes(0, &mut report[..length]);

        if !self.logged_first_report {
            self.logged_first_report = true;

            serial::write_str("USB: ");
            serial::write_str(self.kind.name());
            serial::write_str(" report received bytes=");
            serial::write_usize(length);
            serial::write_str("\n");
        }

        match &mut self.mode {
            Mode::BootKeyboard { keys, modifiers } => {
                let pressed = keyboard::decode(&report[..length], keys, modifiers);

                if let Some(usage) = pressed {
                    log_first_key(usage);
                }
            }

            Mode::BootMouse => decode_boot_mouse(&report[..length]),

            Mode::Descriptor { layout, state } => {
                let pressed = layout.decode(&report[..length], state);

                if let Some(usage) = pressed {
                    log_first_key(usage);
                }
            }
        }
    }
}

// ============================================================
// Boot mouse report
// ============================================================

fn decode_boot_mouse(report: &[u8]) {
    if report.len() < 3 {
        return;
    }

    // Bits 0-2 are the three standard buttons; some boot mice also report
    // the two side buttons in bits 3-4.
    let buttons = report[0] & 0x1F;

    let dx = report[1] as i8 as i32;

    // USB boot mice hand us screen-style Y (positive is down), so the delta
    // goes to the compositor unchanged. (The PS/2 decoder still negates its
    // positive-up reports.)
    let dy = report[2] as i8 as i32;

    crate::input::move_mouse(dx, dy);
    crate::input::set_mouse_buttons(buttons);

    if report.len() >= 4 {
        let wheel = report[3] as i8 as i16;

        if wheel != 0 {
            crate::input::mouse_wheel(wheel);
        }
    }
}

// ============================================================
// Configuration descriptor parsing
// ============================================================

#[derive(Clone)]
struct InterfaceInfo {
    number: u8,
    protocol: u8,
    boot: bool,
    endpoint_address: u8,
    max_packet: u16,
    interval: u8,
    report_length: u16,
}

fn parse_configuration(configuration: &[u8], length: usize) -> Vec<InterfaceInfo> {
    let mut interfaces: Vec<InterfaceInfo> = Vec::new();
    let mut current: Option<usize> = None;

    let mut offset = 0usize;

    while offset + 2 <= length {
        let descriptor_length = configuration[offset] as usize;

        if descriptor_length < 2 || offset + descriptor_length > length {
            break;
        }

        let descriptor_type = configuration[offset + 1];

        match descriptor_type {
            // Interface
            4 if descriptor_length >= 9 => {
                let number = configuration[offset + 2];
                let alternate = configuration[offset + 3];
                let class = configuration[offset + 5];
                let subclass = configuration[offset + 6];
                let protocol = configuration[offset + 7];

                current = None;

                if class == 3 && alternate == 0 {
                    let boot = subclass == 1 && (protocol == 1 || protocol == 2);

                    interfaces.push(InterfaceInfo {
                        number,
                        protocol,
                        boot,
                        endpoint_address: 0,
                        max_packet: 0,
                        interval: 0,
                        report_length: 0,
                    });

                    current = Some(interfaces.len() - 1);
                }
            }

            // Endpoint
            5 if descriptor_length >= 7 => {
                let address = configuration[offset + 2];
                let attributes = configuration[offset + 3];
                let max_packet =
                    u16::from_le_bytes([configuration[offset + 4], configuration[offset + 5]])
                        & 0x07FF;
                let interval = configuration[offset + 6];

                if let Some(index) = current {
                    let is_interrupt = attributes & 0x03 == 3;
                    let is_in = address & 0x80 != 0;
                    let info = &mut interfaces[index];

                    if is_interrupt && is_in && info.endpoint_address == 0 {
                        info.endpoint_address = address;
                        info.max_packet = max_packet;
                        info.interval = interval;
                    }
                }
            }

            // HID descriptor
            0x21 if descriptor_length >= 9 => {
                if let Some(index) = current {
                    let info = &mut interfaces[index];

                    if info.report_length == 0 {
                        info.report_length =
                            u16::from_le_bytes([configuration[offset + 7], configuration[offset + 8]]);
                    }
                }
            }

            _ => {}
        }

        offset += descriptor_length;
    }

    interfaces
}

// ============================================================
// Enumeration
// ============================================================

pub fn enumerate(
    xhci: &mut Xhci,
    slot: u8,
    location: DeviceLocation,
    speed: u8,
    _device: &[u8],
    configuration: &[u8],
    _configuration_value: u8,
) -> Vec<HidDevice> {
    let interfaces = parse_configuration(configuration, configuration.len());

    if interfaces.is_empty() {
        serial::write_str("USB: no HID interfaces found\n");
        return Vec::new();
    }

    let mut devices = Vec::new();

    for info in interfaces.iter() {
        if info.endpoint_address == 0 || info.max_packet == 0 {
            continue;
        }

        if let Some(device) = setup_interface(xhci, slot, location, speed, info) {
            devices.push(device);
        }
    }

    devices
}

fn setup_interface(
    xhci: &mut Xhci,
    slot: u8,
    location: DeviceLocation,
    speed: u8,
    info: &InterfaceInfo,
) -> Option<HidDevice> {
    let mut boot_mode = false;

    if info.boot {
        let request = device::setup(
            0x21,
            device::REQUEST_SET_PROTOCOL,
            0,
            info.number as u16,
            0,
        );

        match xhci.control(slot, request, None) {
            Ok(_) => boot_mode = true,
            Err(_) => {
                serial::write_str("USB: boot protocol unavailable, using report descriptor\n");
            }
        }
    }

    // SET_IDLE(0): only report on change. Optional; some devices stall it.
    let request = device::setup(0x21, device::REQUEST_SET_IDLE, 0, info.number as u16, 0);
    let _ = xhci.control(slot, request, None);

    let (kind, mode, transfer_length) = if boot_mode {
        let kind = match info.protocol {
            1 => HidKind::Keyboard,
            2 => HidKind::Mouse,
            _ => return None,
        };

        let mode = match kind {
            HidKind::Keyboard => Mode::BootKeyboard {
                keys: [0; 6],
                modifiers: 0,
            },
            HidKind::Mouse => Mode::BootMouse,
        };

        (kind, mode, info.max_packet.max(8) as usize)
    } else {
        if info.report_length == 0 {
            serial::write_str("USB: HID interface has no report descriptor\n");
            return None;
        }

        let descriptor =
            device::read_report_descriptor(xhci, slot, info.number, info.report_length)?;

        let Some(layout) = report::parse(&descriptor) else {
            serial::write_str("USB: unsupported HID report descriptor\n");
            return None;
        };

        let kind = match &layout.layout {
            report::Layout::Keyboard(_) => HidKind::Keyboard,
            report::Layout::Mouse(_) => HidKind::Mouse,
        };

        let length = (info.max_packet as usize).max(layout.byte_len()).max(8);

        (
            kind,
            Mode::Descriptor {
                layout,
                state: DecodeState::new(),
            },
            length,
        )
    };

    serial::write_str("USB: HID ");
    serial::write_str(kind.name());
    serial::write_str(" interface=");
    serial::write_usize(info.number as usize);
    serial::write_str(" endpoint=");
    serial::write_hex(info.endpoint_address as u64);
    serial::write_str(" maxpacket=");
    serial::write_usize(info.max_packet as usize);
    serial::write_str(" interval=");
    serial::write_usize(info.interval as usize);
    serial::write_str(" mode=");
    serial::write_str(mode.name());
    serial::write_str("\n");

    let endpoint_number = (info.endpoint_address & 0x0F) as u8;
    let dci = endpoint_number * 2 + 1;

    let interval = interval_exponent(speed, info.interval);

    let ring = TrbRing::new()?;

    if let Err(error) = xhci.configure_interrupt_in(
        slot,
        dci,
        info.max_packet,
        interval,
        ring.physical(),
    ) {
        device::log_error("Configure Endpoint", error);
        return None;
    }

    let report = DmaBuffer::new(transfer_length, 64)?;

    let mut device = HidDevice {
        slot,
        port: location.root_port,
        route: location.route,
        interface: info.number,
        dci,
        kind,
        max_packet: info.max_packet,
        transfer_length,
        report,
        ring,
        in_flight: None,
        mode,
        logged_first_report: false,
    };

    if let Err(error) = device.queue(xhci) {
        device::log_error("interrupt transfer", error);
        return None;
    }

    serial::write_str("USB: HID ");
    serial::write_str(kind.name());
    serial::write_str(" ready on port ");
    serial::write_usize(location.root_port as usize);
    serial::write_str(" interface ");
    serial::write_usize(info.number as usize);
    serial::write_str(" route=");
    serial::write_hex(location.route as u64);
    serial::write_str("\n");

    Some(device)
}

// ============================================================
// Helpers
// ============================================================

/// xHCI endpoint interval field.
///
/// Full/low speed interrupt endpoints use frames directly. High speed and
/// SuperSpeed express the interval as 2^(bInterval-1) microframes, so the
/// context stores the exponent.
fn interval_exponent(speed: u8, b_interval: u8) -> u8 {
    let interval = b_interval.clamp(1, 16);

    if speed <= 2 {
        return interval;
    }

    let mut exponent = 0u8;

    while exponent < 15 && (1u16 << exponent) < interval as u16 {
        exponent += 1;
    }

    exponent.max(1)
}

// ============================================================
// One-shot diagnostics
// ============================================================

use core::sync::atomic::{AtomicBool, Ordering};

static FIRST_KEY_LOGGED: AtomicBool = AtomicBool::new(false);

fn log_first_key(usage: u8) {
    if FIRST_KEY_LOGGED.swap(true, Ordering::AcqRel) {
        return;
    }

    serial::write_str("USB: first key usage=");
    serial::write_hex(usage as u64);
    serial::write_str("\n");
}
