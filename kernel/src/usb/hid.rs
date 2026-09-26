/*
 * Rusty custom xHCI HID layer.
 *
 * This module intentionally does NOT use CrabUSB.
 *
 * It performs USB host-side enumeration for boot-protocol HID keyboard and
 * mouse interfaces on top of Rusty's XhciDriver.
 *
 * The xHCI command/event-ring ownership remains entirely inside XhciDriver:
 * this layer only asks the driver to perform Address Device, control
 * transfers, endpoint configuration, and interrupt transfers.
 */

use alloc::vec::Vec;

use super::xhci::XhciDriver;

// ============================================================
// USB standard requests / descriptors
// ============================================================

const USB_REQ_GET_DESCRIPTOR: u8 = 0x06;
const USB_REQ_SET_CONFIGURATION: u8 = 0x09;

const USB_DESC_DEVICE: u8 = 0x01;
const USB_DESC_CONFIGURATION: u8 = 0x02;
const USB_DESC_INTERFACE: u8 = 0x04;
const USB_DESC_ENDPOINT: u8 = 0x05;
const USB_DESC_HID: u8 = 0x21;
const USB_DESC_SS_ENDPOINT_COMPANION: u8 = 0x30;

// ============================================================
// HID class requests
// ============================================================

const HID_REQ_SET_IDLE: u8 = 0x0A;
const HID_REQ_SET_PROTOCOL: u8 = 0x0B;

// ============================================================
// HID descriptor values
// ============================================================

const USB_CLASS_HID: u8 = 0x03;
const HID_SUBCLASS_BOOT: u8 = 0x01;
const HID_PROTOCOL_KEYBOARD: u8 = 0x01;
const HID_PROTOCOL_MOUSE: u8 = 0x02;

// Interrupt endpoint transfer type.
const USB_ENDPOINT_XFER_INT: u8 = 0x03;

// Completion codes used by xHCI for successful interrupt transfers.
const XHCI_COMPLETION_SUCCESS: u8 = 1;
const XHCI_COMPLETION_SHORT_PACKET: u8 = 13;

// xHCI PORTSC speed IDs are standardized by xHCI for the default mapping.
const XHCI_SPEED_FULL: u8 = 1;
const XHCI_SPEED_LOW: u8 = 2;
const XHCI_SPEED_HIGH: u8 = 3;
const XHCI_SPEED_SUPER: u8 = 4;
const XHCI_SPEED_SUPER_PLUS: u8 = 5;

// ============================================================
// HID type
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HidDeviceKind {
    Keyboard,
    Mouse,
}

// ============================================================
// Parsed HID interrupt endpoint
// ============================================================

#[derive(Clone, Copy)]
struct HidEndpointCandidate {
    interface_number: u8,
    alternate_setting: u8,
    address: u8,
    kind: HidDeviceKind,
    packet_size: u16,
    interval: u8,
}

// ============================================================
// Active HID endpoint
// ============================================================

#[derive(Clone, Copy)]
struct HidEndpoint {
    port: usize,
    slot_id: u8,
    endpoint_id: u8,
    interface_number: u8,
    kind: HidDeviceKind,
    packet_size: usize,
}

// ============================================================
// HID manager
// ============================================================

pub struct HidManager {
    devices: Vec<HidEndpoint>,
}

impl HidManager {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    pub fn has_keyboard(&self) -> bool {
        self.devices
            .iter()
            .any(|device| device.kind == HidDeviceKind::Keyboard)
    }

    pub fn has_mouse(&self) -> bool {
        self.devices
            .iter()
            .any(|device| device.kind == HidDeviceKind::Mouse)
    }

    // ========================================================
    // Enumerate one xHCI slot
    // ========================================================

    pub unsafe fn enumerate_device(
        &mut self,
        xhci: &mut XhciDriver,
        port: usize,
        slot_id: u8,
    ) -> bool {
        if !xhci.is_running() || !xhci.port_connected(port) {
            crate::serial::write_str(
                "USB HID: device is no longer connected\n",
            );
            return false;
        }

        let speed = xhci.port_speed(port);

        crate::serial::write_str("USB HID: enumerating slot=");
        crate::serial::write_hex(slot_id as u64);
        crate::serial::write_str(" port=");
        crate::serial::write_hex(port as u64);
        crate::serial::write_str(" speed=");
        crate::serial::write_hex(speed as u64);
        crate::serial::write_str("\n");

        // ----------------------------------------------------
        // Initial EP0 MPS used for Address Device.
        // ----------------------------------------------------

        let initial_mps = match speed {
            XHCI_SPEED_LOW => 8u16,
            XHCI_SPEED_FULL => 8u16,
            XHCI_SPEED_HIGH => 64u16,
            XHCI_SPEED_SUPER | XHCI_SPEED_SUPER_PLUS => 512u16,
            _ => {
                crate::serial::write_str(
                    "USB HID: unsupported/unknown xHCI port speed ID\n",
                );
                return false;
            }
        };

        // ----------------------------------------------------
        // Address Device.
        // ----------------------------------------------------

        if !xhci.address_device(slot_id, port, speed, initial_mps) {
            crate::serial::write_str("USB HID: Address Device failed\n");
            return false;
        }

        // ----------------------------------------------------
        // First 8 bytes of Device Descriptor.
        // ----------------------------------------------------

        let mut device_header = [0u8; 8];

        let Some(actual) = xhci.control_transfer_in(
            slot_id,
            0x80,
            USB_REQ_GET_DESCRIPTOR,
            (USB_DESC_DEVICE as u16) << 8,
            0,
            &mut device_header,
        ) else {
            crate::serial::write_str(
                "USB HID: GET_DESCRIPTOR(Device,8) failed\n",
            );
            return false;
        };

        if actual < 8 || device_header[0] < 8 || device_header[1] != USB_DESC_DEVICE {
            crate::serial::write_str("USB HID: invalid Device Descriptor header\n");
            return false;
        }

        // ----------------------------------------------------
        // Decode bMaxPacketSize0.
        // ----------------------------------------------------

        let device_mps = match speed {
            XHCI_SPEED_SUPER | XHCI_SPEED_SUPER_PLUS => {
                let exponent = device_header[7];

                if !(3..=9).contains(&exponent) {
                    crate::serial::write_str(
                        "USB HID: invalid SuperSpeed bMaxPacketSize0 exponent\n",
                    );
                    return false;
                }

                1u16 << exponent
            }

            XHCI_SPEED_LOW => {
                if device_header[7] != 8 {
                    crate::serial::write_str(
                        "USB HID: low-speed device reported invalid EP0 packet size\n",
                    );
                    return false;
                }

                8
            }

            XHCI_SPEED_FULL => match device_header[7] {
                8 | 16 | 32 | 64 => device_header[7] as u16,
                _ => {
                    crate::serial::write_str(
                        "USB HID: full-speed device reported invalid EP0 packet size\n",
                    );
                    return false;
                }
            },

            XHCI_SPEED_HIGH => {
                if device_header[7] != 64 {
                    crate::serial::write_str(
                        "USB HID: high-speed device reported invalid EP0 packet size\n",
                    );
                    return false;
                }

                64
            }

            _ => return false,
        };

        crate::serial::write_str("USB HID: bMaxPacketSize0=");
        crate::serial::write_hex(device_mps as u64);
        crate::serial::write_str("\n");

        // Full/low-speed devices can legitimately need Evaluate Context when
        // the initial EP0 MPS used by Address Device differs from the real
        // descriptor value. High/SuperSpeed defaults are already fixed.
        if device_mps != initial_mps {
            if !xhci.update_ep0_max_packet_size(slot_id, device_mps) {
                crate::serial::write_str(
                    "USB HID: Evaluate Context for EP0 failed\n",
                );
                return false;
            }
        }

        // ----------------------------------------------------
        // Full Device Descriptor.
        // ----------------------------------------------------

        let mut device_descriptor = [0u8; 18];

        let Some(actual) = xhci.control_transfer_in(
            slot_id,
            0x80,
            USB_REQ_GET_DESCRIPTOR,
            (USB_DESC_DEVICE as u16) << 8,
            0,
            &mut device_descriptor,
        ) else {
            crate::serial::write_str(
                "USB HID: full Device Descriptor request failed\n",
            );
            return false;
        };

        if actual < 18
            || device_descriptor[0] < 18
            || device_descriptor[1] != USB_DESC_DEVICE
        {
            crate::serial::write_str("USB HID: full Device Descriptor invalid\n");
            return false;
        }

        let vid = u16::from_le_bytes([
            device_descriptor[8],
            device_descriptor[9],
        ]);

        let pid = u16::from_le_bytes([
            device_descriptor[10],
            device_descriptor[11],
        ]);

        let configuration_count = device_descriptor[17];

        crate::serial::write_str("USB HID: VID=0x");
        crate::serial::write_hex(vid as u64);
        crate::serial::write_str(" PID=0x");
        crate::serial::write_hex(pid as u64);
        crate::serial::write_str(" configurations=");
        crate::serial::write_hex(configuration_count as u64);
        crate::serial::write_str("\n");

        if configuration_count == 0 {
            crate::serial::write_str("USB HID: device has no configurations\n");
            return false;
        }

        // ----------------------------------------------------
        // Configuration descriptor header.
        // ----------------------------------------------------

        let mut configuration_header = [0u8; 9];

        let Some(actual) = xhci.control_transfer_in(
            slot_id,
            0x80,
            USB_REQ_GET_DESCRIPTOR,
            (USB_DESC_CONFIGURATION as u16) << 8,
            0,
            &mut configuration_header,
        ) else {
            crate::serial::write_str(
                "USB HID: GET_DESCRIPTOR(Configuration,9) failed\n",
            );
            return false;
        };

        if actual < 9
            || configuration_header[0] < 9
            || configuration_header[1] != USB_DESC_CONFIGURATION
        {
            crate::serial::write_str("USB HID: invalid Configuration Descriptor\n");
            return false;
        }

        let total_length = u16::from_le_bytes([
            configuration_header[2],
            configuration_header[3],
        ]) as usize;

        let configuration_value = configuration_header[5];

        if total_length < 9 || total_length > 4096 {
            crate::serial::write_str(
                "USB HID: unsupported configuration descriptor size\n",
            );
            return false;
        }

        // ----------------------------------------------------
        // Full Configuration Descriptor tree.
        // ----------------------------------------------------

        let mut configuration = Vec::<u8>::with_capacity(total_length);
        configuration.resize(total_length, 0);

        let Some(actual) = xhci.control_transfer_in(
            slot_id,
            0x80,
            USB_REQ_GET_DESCRIPTOR,
            (USB_DESC_CONFIGURATION as u16) << 8,
            0,
            &mut configuration,
        ) else {
            crate::serial::write_str(
                "USB HID: full Configuration Descriptor request failed\n",
            );
            return false;
        };

        if actual < 9 {
            crate::serial::write_str(
                "USB HID: full Configuration Descriptor too short\n",
            );
            return false;
        }

        let parse_length = actual.min(configuration.len());

        let mut current_hid_interface: Option<(u8, u8, HidDeviceKind)> = None;
        let mut endpoints = Vec::<HidEndpointCandidate>::new();

        // USB 3.x Endpoint Companion descriptors immediately follow their
        // endpoint descriptor. The current driver does not expose the
        // companion's MaxBurst field, so keep this parsed and reject endpoint
        // configurations that require more than one burst/transaction.
        let mut last_endpoint_index: Option<usize> = None;

        let mut offset = 0usize;

        while offset + 2 <= parse_length {
            let length = configuration[offset] as usize;
            let descriptor_type = configuration[offset + 1];

            if length < 2 || offset + length > parse_length {
                crate::serial::write_str(
                    "USB HID: malformed configuration descriptor\n",
                );
                return false;
            }

            match descriptor_type {
                USB_DESC_INTERFACE if length >= 9 => {
                    let interface_number = configuration[offset + 2];
                    let alternate_setting = configuration[offset + 3];
                    let class = configuration[offset + 5];
                    let subclass = configuration[offset + 6];
                    let protocol = configuration[offset + 7];

                    current_hid_interface = if alternate_setting == 0
                        && class == USB_CLASS_HID
                        && subclass == HID_SUBCLASS_BOOT
                    {
                        let kind = match protocol {
                            HID_PROTOCOL_KEYBOARD => HidDeviceKind::Keyboard,
                            HID_PROTOCOL_MOUSE => HidDeviceKind::Mouse,
                            _ => {
                                current_hid_interface = None;
                                offset += length;
                                last_endpoint_index = None;
                                continue;
                            }
                        };

                        Some((interface_number, alternate_setting, kind))
                    } else {
                        None
                    };

                    last_endpoint_index = None;

                    crate::serial::write_str("USB HID: interface=");
                    crate::serial::write_hex(interface_number as u64);
                    crate::serial::write_str(" alt=");
                    crate::serial::write_hex(alternate_setting as u64);
                    crate::serial::write_str(" class=");
                    crate::serial::write_hex(class as u64);
                    crate::serial::write_str(" subclass=");
                    crate::serial::write_hex(subclass as u64);
                    crate::serial::write_str(" protocol=");
                    crate::serial::write_hex(protocol as u64);
                    crate::serial::write_str("\n");
                }

                USB_DESC_HID if length >= 9 => {
                    if current_hid_interface.is_some() {
                        crate::serial::write_str("USB HID: HID descriptor found\n");
                    }
                }

                USB_DESC_ENDPOINT if length >= 7 => {
                    last_endpoint_index = None;

                    let Some((interface_number, alternate_setting, kind)) =
                        current_hid_interface
                    else {
                        offset += length;
                        continue;
                    };

                    let address = configuration[offset + 2];
                    let attributes = configuration[offset + 3];
                    let packet = u16::from_le_bytes([
                        configuration[offset + 4],
                        configuration[offset + 5],
                    ]);
                    let interval = configuration[offset + 6];

                    let direction_in = (address & 0x80) != 0;
                    let transfer_type = attributes & 0x03;

                    if !direction_in || transfer_type != USB_ENDPOINT_XFER_INT {
                        offset += length;
                        continue;
                    }

                    let transaction_multiplier = ((packet >> 11) & 0x03) as u8;
                    if transaction_multiplier != 0 {
                        crate::serial::write_str(
                            "USB HID: rejecting interrupt endpoint with multiple transactions per microframe\n",
                        );
                        offset += length;
                        continue;
                    }

                    let max_packet = packet & 0x07FF;

                    if max_packet == 0 || max_packet > 1024 || interval == 0 {
                        crate::serial::write_str(
                            "USB HID: invalid interrupt-IN endpoint parameters\n",
                        );
                        offset += length;
                        continue;
                    }

                    endpoints.push(HidEndpointCandidate {
                        interface_number,
                        alternate_setting,
                        address,
                        kind,
                        packet_size: max_packet,
                        interval,
                    });

                    last_endpoint_index = endpoints.len().checked_sub(1);

                    crate::serial::write_str(
                        "USB HID: interrupt-IN endpoint address=0x",
                    );
                    crate::serial::write_hex(address as u64);
                    crate::serial::write_str(" max_packet=");
                    crate::serial::write_hex(max_packet as u64);
                    crate::serial::write_str(" interval=");
                    crate::serial::write_hex(interval as u64);
                    crate::serial::write_str("\n");
                }

                USB_DESC_SS_ENDPOINT_COMPANION if length >= 6 => {
                    let Some(index) = last_endpoint_index else {
                        offset += length;
                        continue;
                    };

                    if speed < XHCI_SPEED_SUPER {
                        offset += length;
                        continue;
                    }

                    let max_burst = configuration[offset + 2];
                    let bm_attributes = configuration[offset + 3];

                    // The current driver constructs a fixed endpoint context
                    // and has no API for exposing bMaxBurst / the companion
                    // fields. Boot HID devices normally use one burst.
                    if max_burst != 0 || bm_attributes != 0 {
                        endpoints.swap_remove(index);
                        last_endpoint_index = None;

                        crate::serial::write_str(
                            "USB HID: rejecting SuperSpeed HID endpoint requiring unsupported companion parameters\n",
                        );
                    }
                }

                _ => {}
            }

            offset += length;
        }

        if endpoints.is_empty() {
            crate::serial::write_str(
                "USB HID: no boot HID interrupt-IN endpoints found\n",
            );
            return false;
        }

        // ----------------------------------------------------
        // Select the USB configuration.
        // ----------------------------------------------------

        if !xhci.control_transfer_out(
            slot_id,
            0x00,
            USB_REQ_SET_CONFIGURATION,
            configuration_value as u16,
            0,
        ) {
            crate::serial::write_str("USB HID: SET_CONFIGURATION failed\n");
            return false;
        }

        crate::serial::write_str("USB HID: configuration selected value=");
        crate::serial::write_hex(configuration_value as u64);
        crate::serial::write_str("\n");

        // ----------------------------------------------------
        // Configure each boot HID interrupt-IN endpoint.
        // ----------------------------------------------------

        let mut configured_count = 0usize;

        for endpoint in endpoints {
            if endpoint.alternate_setting != 0 {
                // The parser currently only selects altsetting zero, so this
                // is defensive rather than expected.
                continue;
            }

            let interface_number = endpoint.interface_number;

            // Boot protocol is zero. Set idle to zero as well so the device
            // reports current state continuously without a host-side idle
            // timer requirement.
            if !xhci.control_transfer_out(
                slot_id,
                0x21,
                HID_REQ_SET_PROTOCOL,
                0,
                interface_number as u16,
            ) {
                crate::serial::write_str(
                    "USB HID: SET_PROTOCOL failed interface=",
                );
                crate::serial::write_hex(interface_number as u64);
                crate::serial::write_str("\n");
                return false;
            }

            if !xhci.control_transfer_out(
                slot_id,
                0x21,
                HID_REQ_SET_IDLE,
                0,
                interface_number as u16,
            ) {
                crate::serial::write_str(
                    "USB HID: SET_IDLE failed interface=",
                );
                crate::serial::write_hex(interface_number as u64);
                crate::serial::write_str("\n");
                return false;
            }

            if !xhci.configure_interrupt_in_endpoint(
                slot_id,
                endpoint.address,
                endpoint.packet_size,
                endpoint.interval,
                speed,
            ) {
                crate::serial::write_str(
                    "USB HID: Configure Endpoint failed address=0x",
                );
                crate::serial::write_hex(endpoint.address as u64);
                crate::serial::write_str("\n");
                return false;
            }

            let endpoint_id = endpoint_id_from_address(endpoint.address);

            let device = HidEndpoint {
                port,
                slot_id,
                endpoint_id,
                interface_number,
                kind: endpoint.kind,
                packet_size: endpoint.packet_size as usize,
            };

            // Arm the endpoint before publishing it to the manager. That
            // prevents a partially configured HID device from becoming visible
            // to the polling layer if the first transfer cannot be queued.
            if !xhci.submit_interrupt_in(slot_id, endpoint_id) {
                crate::serial::write_str(
                    "USB HID: initial interrupt-IN submission failed\n",
                );
                return false;
            }

            self.devices.push(device);
            configured_count += 1;

            match endpoint.kind {
                HidDeviceKind::Keyboard => {
                    crate::serial::write_str(
                        "USB HID: keyboard interrupt-IN armed\n",
                    );
                }

                HidDeviceKind::Mouse => {
                    crate::serial::write_str(
                        "USB HID: mouse interrupt-IN armed\n",
                    );
                }
            }
        }

        if configured_count == 0 {
            crate::serial::write_str(
                "USB HID: no HID endpoint was successfully configured\n",
            );
            return false;
        }

        crate::serial::write_str(
            "USB HID: enumeration/endpoint setup complete\n",
        );

        true
    }

    // ========================================================
    // Normal-context transfer polling
    // ========================================================

    pub unsafe fn poll(&mut self, xhci: &mut XhciDriver) {
        loop {
            let Some(completion) = xhci.take_transfer_completion() else {
                break;
            };

            let Some(index) = self.devices.iter().position(|device| {
                device.slot_id == completion.slot_id
                    && device.endpoint_id == completion.endpoint_id
            }) else {
                crate::serial::write_str(
                    "USB HID: transfer completion for unknown endpoint slot=",
                );
                crate::serial::write_hex(completion.slot_id as u64);
                crate::serial::write_str(" ep=");
                crate::serial::write_hex(completion.endpoint_id as u64);
                crate::serial::write_str("\n");
                continue;
            };

            let device = self.devices[index];

            if completion.completion_code != XHCI_COMPLETION_SUCCESS
                && completion.completion_code != XHCI_COMPLETION_SHORT_PACKET
            {
                crate::serial::write_str(
                    "USB HID: interrupt transfer error code=",
                );
                crate::serial::write_hex(completion.completion_code as u64);
                crate::serial::write_str(" slot=");
                crate::serial::write_hex(completion.slot_id as u64);
                crate::serial::write_str(" ep=");
                crate::serial::write_hex(completion.endpoint_id as u64);
                crate::serial::write_str("\n");

                // The current xHCI driver does not expose endpoint-halt
                // recovery/ClearFeature, so a stalled or transaction-error
                // endpoint must not be blindly re-armed as though the TD
                // completed normally.
                continue;
            }

            let residual = completion.transfer_length as usize;
            let actual_length = device
                .packet_size
                .saturating_sub(residual)
                .min(device.packet_size);

            let mut report = [0u8; 1024];

            let copied = xhci
                .copy_interrupt_report(
                    device.slot_id,
                    device.endpoint_id,
                    &mut report[..device.packet_size],
                )
                .unwrap_or(0)
                .min(actual_length);

            if copied != 0 {
                let report = &report[..copied];

                match device.kind {
                    HidDeviceKind::Keyboard => {
                        crate::input::process_keyboard_report(report);
                    }

                    HidDeviceKind::Mouse => {
                        crate::input::process_mouse_report(report);
                    }
                }
            }

            // Re-arm only after the completed buffer has been consumed by the
            // input layer.
            if !xhci.submit_interrupt_in(device.slot_id, device.endpoint_id) {
                crate::serial::write_str(
                    "USB HID: failed to re-arm interrupt-IN endpoint slot=",
                );
                crate::serial::write_hex(device.slot_id as u64);
                crate::serial::write_str(" ep=");
                crate::serial::write_hex(device.endpoint_id as u64);
                crate::serial::write_str("\n");
            }
        }
    }
}

// ============================================================
// Endpoint address -> xHCI endpoint ID
// ============================================================

#[inline(always)]
fn endpoint_id_from_address(address: u8) -> u8 {
    let endpoint_number = address & 0x0F;
    endpoint_number
        .saturating_mul(2)
        .saturating_add(if address & 0x80 != 0 { 1 } else { 0 })
}
