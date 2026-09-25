/*
 * Rusty custom xHCI HID layer.
 *
 * This module intentionally does NOT use CrabUSB.
 *
 * It performs the minimum real USB host-side enumeration required for
 * boot-protocol HID keyboard/mouse devices on top of Rusty's XhciDriver.
 */

use alloc::vec::Vec;

use super::xhci::XhciDriver;

// ============================================================
// USB standard requests
// ============================================================

const USB_REQ_GET_DESCRIPTOR: u8 = 0x06;
const USB_REQ_SET_CONFIGURATION: u8 = 0x09;

const USB_DESC_DEVICE: u8 = 0x01;
const USB_DESC_CONFIGURATION: u8 = 0x02;

// ============================================================
// HID class requests
// ============================================================

const HID_REQ_SET_PROTOCOL: u8 = 0x0B;

// ============================================================
// HID descriptor values
// ============================================================

const USB_CLASS_HID: u8 = 0x03;
const HID_SUBCLASS_BOOT: u8 = 0x01;
const HID_PROTOCOL_KEYBOARD: u8 = 0x01;
const HID_PROTOCOL_MOUSE: u8 = 0x02;

const USB_DESC_INTERFACE: u8 = 0x04;
const USB_DESC_ENDPOINT: u8 = 0x05;

// Interrupt endpoint transfer type.
const USB_ENDPOINT_XFER_INT: u8 = 0x03;

// Completion codes used by xHCI for successful data transfers.
const XHCI_COMPLETION_SUCCESS: u8 = 1;
const XHCI_COMPLETION_SHORT_PACKET: u8 = 13;

// ============================================================
// HID type
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HidDeviceKind {
    Keyboard,
    Mouse,
}

// ============================================================
// HID endpoint
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

        crate::serial::write_str(
            "USB HID: enumerating slot=",
        );
        crate::serial::write_hex(slot_id as u64);
        crate::serial::write_str(" port=");
        crate::serial::write_hex(port as u64);
        crate::serial::write_str(" speed=");
        crate::serial::write_hex(speed as u64);
        crate::serial::write_str("\n");

        // ----------------------------------------------------
        // Initial EP0 max packet size by link speed.
        // ----------------------------------------------------

        let initial_mps = match speed {
            1 | 2 => 8u16,
            3 => 64u16,
            4 | 5 => 512u16, // SuperSpeed / SuperSpeedPlus
            _ => {
                crate::serial::write_str(
                    "USB HID: unsupported/unknown device speed\n",
                );
                return false;
            }
        };

        // ----------------------------------------------------
        // Address Device.
        // ----------------------------------------------------

        if !unsafe {
            xhci.address_device(
                slot_id,
                port,
                speed,
                initial_mps,
            )
        } {
            crate::serial::write_str(
                "USB HID: Address Device failed\n",
            );
            return false;
        }

        // ----------------------------------------------------
        // First Device Descriptor request.
        // ----------------------------------------------------
        //
        // We first request only eight bytes so bMaxPacketSize0 is available
        // before requesting the full descriptor.
        // ----------------------------------------------------

        let mut device_header = [0u8; 8];

        let Some(actual) = (unsafe {
            xhci.control_transfer_in(
                slot_id,
                0x80,
                USB_REQ_GET_DESCRIPTOR,
                (USB_DESC_DEVICE as u16) << 8,
                0,
                &mut device_header,
            )
        }) else {
            crate::serial::write_str(
                "USB HID: GET_DESCRIPTOR(Device,8) failed\n",
            );
            return false;
        };

        if actual < 8 || device_header[1] != USB_DESC_DEVICE {
            crate::serial::write_str(
                "USB HID: invalid Device Descriptor\n",
            );
            return false;
        }

        let device_mps = if speed >= 4 {
            let exponent = device_header[7];
            if exponent > 10 {
                crate::serial::write_str(
                    "USB HID: invalid SuperSpeed bMaxPacketSize0 exponent\n",
                );
                return false;
            }
            1u16 << exponent
        } else {
            device_header[7] as u16
        };

        crate::serial::write_str(
            "USB HID: bMaxPacketSize0=",
        );
        crate::serial::write_hex(device_mps as u64);
        crate::serial::write_str("\n");

        if device_mps == 0 {
            crate::serial::write_str(
                "USB HID: device reported zero EP0 packet size\n",
            );
            return false;
        }

        if device_mps != initial_mps {
            crate::serial::write_str(
                "USB HID: updating EP0 max packet size\n",
            );

            if !unsafe {
                xhci.update_ep0_max_packet_size(
                    slot_id,
                    device_mps,
                )
            } {
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

        let Some(actual) = (unsafe {
            xhci.control_transfer_in(
                slot_id,
                0x80,
                USB_REQ_GET_DESCRIPTOR,
                (USB_DESC_DEVICE as u16) << 8,
                0,
                &mut device_descriptor,
            )
        }) else {
            crate::serial::write_str(
                "USB HID: full Device Descriptor request failed\n",
            );
            return false;
        };

        if actual < 18 || device_descriptor[1] != USB_DESC_DEVICE {
            crate::serial::write_str(
                "USB HID: full Device Descriptor invalid\n",
            );
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

        crate::serial::write_str(
            "USB HID: VID=0x",
        );
        crate::serial::write_hex(vid as u64);
        crate::serial::write_str(" PID=0x");
        crate::serial::write_hex(pid as u64);
        crate::serial::write_str(" configurations=");
        crate::serial::write_hex(configuration_count as u64);
        crate::serial::write_str("\n");

        if configuration_count == 0 {
            crate::serial::write_str(
                "USB HID: device has no configurations\n",
            );
            return false;
        }

        // ----------------------------------------------------
        // Configuration descriptor header.
        // ----------------------------------------------------

        let mut configuration_header = [0u8; 9];

        let Some(actual) = (unsafe {
            xhci.control_transfer_in(
                slot_id,
                0x80,
                USB_REQ_GET_DESCRIPTOR,
                (USB_DESC_CONFIGURATION as u16) << 8,
                0,
                &mut configuration_header,
            )
        }) else {
            crate::serial::write_str(
                "USB HID: GET_DESCRIPTOR(Configuration,9) failed\n",
            );
            return false;
        };

        if actual < 9 || configuration_header[1] != USB_DESC_CONFIGURATION {
            crate::serial::write_str(
                "USB HID: invalid Configuration Descriptor\n",
            );
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

        let Some(actual) = (unsafe {
            xhci.control_transfer_in(
                slot_id,
                0x80,
                USB_REQ_GET_DESCRIPTOR,
                (USB_DESC_CONFIGURATION as u16) << 8,
                0,
                &mut configuration,
            )
        }) else {
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

        let mut current_interface: Option<(u8, u8, u8, u8, HidDeviceKind)> = None;
        let mut endpoints = Vec::<(u8, u8, u8, HidDeviceKind, u16, u8)>::new();

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

                    let kind = if class == USB_CLASS_HID
                        && subclass == HID_SUBCLASS_BOOT
                        && protocol == HID_PROTOCOL_KEYBOARD
                    {
                        Some(HidDeviceKind::Keyboard)
                    } else if class == USB_CLASS_HID
                        && subclass == HID_SUBCLASS_BOOT
                        && protocol == HID_PROTOCOL_MOUSE
                    {
                        Some(HidDeviceKind::Mouse)
                    } else {
                        None
                    };

                    current_interface = kind.map(|kind| {
                        (
                            interface_number,
                            alternate_setting,
                            class,
                            subclass,
                            kind,
                        )
                    });

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

                USB_DESC_ENDPOINT if length >= 7 => {
                    let Some((
                        interface_number,
                        alternate_setting,
                        _class,
                        _subclass,
                        kind,
                    )) = current_interface else {
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

                    if address & 0x80 != 0
                        && (attributes & 0x03) == USB_ENDPOINT_XFER_INT
                    {
                        let max_packet = packet & 0x07FF;

                        if max_packet != 0 {
                            endpoints.push((
                                interface_number,
                                alternate_setting,
                                address,
                                kind,
                                max_packet,
                                interval,
                            ));

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
        // Select configuration.
        // ----------------------------------------------------

        if !unsafe {
            xhci.control_transfer_out(
                slot_id,
                0x00,
                USB_REQ_SET_CONFIGURATION,
                configuration_value as u16,
                0,
            )
        } {
            crate::serial::write_str(
                "USB HID: SET_CONFIGURATION failed\n",
            );
            return false;
        }

        crate::serial::write_str(
            "USB HID: configuration selected value=",
        );
        crate::serial::write_hex(configuration_value as u64);
        crate::serial::write_str("\n");

        // ----------------------------------------------------
        // Configure every discovered boot-HID interface.
        // ----------------------------------------------------

        for (
            interface_number,
            _alternate_setting,
            address,
            kind,
            packet_size,
            interval,
        ) in endpoints
        {
            // Boot protocol = 0.
            if !unsafe {
                xhci.control_transfer_out(
                    slot_id,
                    0x21,
                    HID_REQ_SET_PROTOCOL,
                    0,
                    interface_number as u16,
                )
            } {
                crate::serial::write_str(
                    "USB HID: SET_PROTOCOL failed interface=",
                );
                crate::serial::write_hex(interface_number as u64);
                crate::serial::write_str("\n");
                return false;
            }

            if !unsafe {
                xhci.configure_interrupt_in_endpoint(
                    slot_id,
                    address,
                    packet_size,
                    interval,
                    speed,
                )
            } {
                crate::serial::write_str(
                    "USB HID: Configure Endpoint failed address=0x",
                );
                crate::serial::write_hex(address as u64);
                crate::serial::write_str("\n");
                return false;
            }

            let endpoint_id = endpoint_id_from_address(address);

            self.devices.push(HidEndpoint {
                port,
                slot_id,
                endpoint_id,
                interface_number,
                kind,
                packet_size: packet_size as usize,
            });

            if !unsafe {
                xhci.submit_interrupt_in(slot_id, endpoint_id)
            } {
                crate::serial::write_str(
                    "USB HID: initial interrupt-IN submission failed\n",
                );
                return false;
            }

            match kind {
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

                if xhci.port_connected(device.port) {
                    crate::serial::write_str(
                        "USB HID: endpoint was not re-armed after transfer error\n",
                    );
                }
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

            // Re-arm the endpoint immediately after the completed report is
            // copied into the normal kernel input layer.
            let _ = unsafe {
                xhci.submit_interrupt_in(
                    device.slot_id,
                    device.endpoint_id,
                )
            };
        }
    }
}

// ============================================================
// Endpoint address -> xHCI endpoint ID
// ============================================================

#[inline(always)]
fn endpoint_id_from_address(address: u8) -> u8 {
    let endpoint_number = address & 0x0F;
    endpoint_number.saturating_mul(2).saturating_add(1)
}
