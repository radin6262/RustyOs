use crab_usb::{
    Device,
    DeviceInfo,
};

use alloc::vec::Vec;

// ============================================================
// USB HID constants
// ============================================================

pub const USB_CLASS_HID: u8 = 0x03;

pub const HID_SUBCLASS_BOOT: u8 = 0x01;

pub const HID_PROTOCOL_KEYBOARD: u8 = 0x01;
pub const HID_PROTOCOL_MOUSE: u8 = 0x02;

// ============================================================
// USB endpoint constants
// ============================================================

pub const USB_ENDPOINT_DIRECTION_IN: u8 = 0x80;

pub const USB_TRANSFER_TYPE_CONTROL: u8 = 0x00;
pub const USB_TRANSFER_TYPE_ISOCHRONOUS: u8 = 0x01;
pub const USB_TRANSFER_TYPE_BULK: u8 = 0x02;
pub const USB_TRANSFER_TYPE_INTERRUPT: u8 = 0x03;

// ============================================================
// Detected USB interface type
// ============================================================

#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
)]
pub enum HidDeviceKind {
    Keyboard,
    Mouse,
}

// ============================================================
// Detected HID interface
// ============================================================

#[derive(
    Clone,
    Copy,
)]
pub struct HidInterface {
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub kind: HidDeviceKind,
}

// ============================================================
// HID endpoint information
// ============================================================

#[derive(
    Clone,
    Copy,
)]
pub struct HidInterruptEndpoint {
    pub address: u8,
    pub max_packet_size: u16,
    pub interval: u8,
}

// ============================================================
// Inspect device information
// ============================================================
//
// Called BEFORE open_device().
//
// Does not modify the device.
//
// ============================================================

pub fn inspect_device_info(
    info: &DeviceInfo,
) {
    crate::serial::write_str(
        "USB: DEVICE DESCRIPTORS\n",
    );

    crate::serial::write_str(
        "USB: id=",
    );

    crate::serial::write_hex(
        info.id() as u64,
    );

    crate::serial::write_str(
        " vendor=",
    );

    crate::serial::write_hex(
        info.vendor_id() as u64,
    );

    crate::serial::write_str(
        " product=",
    );

    crate::serial::write_hex(
        info.product_id() as u64,
    );

    crate::serial::write_str(
        "\n",
    );

    let detected =
        detect_interfaces(info);

    if detected.is_empty() {
        crate::serial::write_str(
            "USB: no supported HID interfaces found\n",
        );

        return;
    }

    crate::serial::write_str(
        "USB: supported HID interface count=",
    );

    crate::serial::write_hex(
        detected.len() as u64,
    );

    crate::serial::write_str(
        "\n",
    );

    for interface in detected {
        match interface.kind {
            HidDeviceKind::Keyboard => {
                crate::serial::write_str(
                    "USB: keyboard interface=",
                );

                crate::serial::write_hex(
                    interface.interface_number as u64,
                );

                crate::serial::write_str(
                    " alt=",
                );

                crate::serial::write_hex(
                    interface.alternate_setting as u64,
                );

                crate::serial::write_str(
                    "\n",
                );
            }

            HidDeviceKind::Mouse => {
                crate::serial::write_str(
                    "USB: mouse interface=",
                );

                crate::serial::write_hex(
                    interface.interface_number as u64,
                );

                crate::serial::write_str(
                    " alt=",
                );

                crate::serial::write_hex(
                    interface.alternate_setting as u64,
                );

                crate::serial::write_str(
                    "\n",
                );
            }
        }
    }
}

// ============================================================
// Detect device
// ============================================================

pub fn detect_device(
    info: &DeviceInfo,
) -> Vec<HidInterface> {
    detect_interfaces(info)
}

// ============================================================
// Detect USB interfaces
// ============================================================
//
// HID Boot Keyboard:
//
// class    = 0x03
// subclass = 0x01
// protocol = 0x01
//
// HID Boot Mouse:
//
// class    = 0x03
// subclass = 0x01
// protocol = 0x02
//
// ============================================================

pub fn detect_interfaces(
    info: &DeviceInfo,
) -> Vec<HidInterface> {
    let mut detected =
        Vec::<HidInterface>::new();

    crate::serial::write_str(
        "USB: INTERFACES\n",
    );

    for interface in info.interface_descriptors() {
        // ----------------------------------------------------
        // Print descriptor
        // ----------------------------------------------------

        crate::serial::write_str(
            "USB: interface=",
        );

        crate::serial::write_hex(
            interface.interface_number as u64,
        );

        crate::serial::write_str(
            " alt=",
        );

        crate::serial::write_hex(
            interface.alternate_setting as u64,
        );

        crate::serial::write_str(
            " class=",
        );

        crate::serial::write_hex(
            interface.class as u64,
        );

        crate::serial::write_str(
            " subclass=",
        );

        crate::serial::write_hex(
            interface.subclass as u64,
        );

        crate::serial::write_str(
            " protocol=",
        );

        crate::serial::write_hex(
            interface.protocol as u64,
        );

        crate::serial::write_str(
            "\n",
        );

        // ----------------------------------------------------
        // HID class
        // ----------------------------------------------------

        if interface.class != USB_CLASS_HID {
            crate::serial::write_str(
                "USB: interface is not HID\n",
            );

            continue;
        }

        crate::serial::write_str(
            "USB: HID interface detected\n",
        );

        // ----------------------------------------------------
        // Boot subclass
        // ----------------------------------------------------

        if interface.subclass != HID_SUBCLASS_BOOT {
            crate::serial::write_str(
                "USB: HID interface is not Boot subclass\n",
            );

            continue;
        }

        // ----------------------------------------------------
        // Protocol
        // ----------------------------------------------------

        let kind =
            match interface.protocol {
                HID_PROTOCOL_KEYBOARD => {
                    crate::serial::write_str(
                        "USB: HID BOOT KEYBOARD DETECTED\n",
                    );

                    Some(
                        HidDeviceKind::Keyboard,
                    )
                }

                HID_PROTOCOL_MOUSE => {
                    crate::serial::write_str(
                        "USB: HID BOOT MOUSE DETECTED\n",
                    );

                    Some(
                        HidDeviceKind::Mouse,
                    )
                }

                _ => {
                    crate::serial::write_str(
                        "USB: HID protocol unsupported\n",
                    );

                    None
                }
            };

        // ----------------------------------------------------
        // Save interface
        // ----------------------------------------------------

        if let Some(kind) = kind {
            detected.push(
                HidInterface {
                    interface_number:
                    interface.interface_number,

                    alternate_setting:
                    interface.alternate_setting,

                    kind,
                },
            );
        }
    }

    // --------------------------------------------------------
    // Summary
    // --------------------------------------------------------

    crate::serial::write_str(
        "USB: supported HID interface count=",
    );

    crate::serial::write_hex(
        detected.len() as u64,
    );

    crate::serial::write_str(
        "\n",
    );

    detected
}

// ============================================================
// Inspect opened USB device
// ============================================================
//
// Called AFTER open_device() succeeds.
//
// ============================================================

pub fn inspect_open_device(
    device: &Device,
) {
    crate::serial::write_str(
        "USB: DEVICE INFORMATION\n",
    );

    // --------------------------------------------------------
    // Device identity
    // --------------------------------------------------------

    crate::serial::write_str(
        "USB: slot=",
    );

    crate::serial::write_hex(
        device.slot_id() as u64,
    );

    crate::serial::write_str(
        " vendor=",
    );

    crate::serial::write_hex(
        device.vendor_id() as u64,
    );

    crate::serial::write_str(
        " product=",
    );

    crate::serial::write_hex(
        device.product_id() as u64,
    );

    crate::serial::write_str(
        "\n",
    );

    // --------------------------------------------------------
    // Configurations
    // --------------------------------------------------------

    let configurations =
        device.configurations();

    crate::serial::write_str(
        "USB: configuration_count=",
    );

    crate::serial::write_hex(
        configurations.len() as u64,
    );

    crate::serial::write_str(
        "\n",
    );

    // --------------------------------------------------------
    // Configuration loop
    // --------------------------------------------------------

    for configuration in configurations {
        crate::serial::write_str(
            "USB: configuration=",
        );

        crate::serial::write_hex(
            configuration.configuration_value as u64,
        );

        crate::serial::write_str(
            "\n",
        );

        // ----------------------------------------------------
        // Interface groups
        // ----------------------------------------------------

        for interface_group in &configuration.interfaces {
            // ------------------------------------------------
            // Alternate settings
            // ------------------------------------------------

            for interface in &interface_group.alt_settings {
                crate::serial::write_str(
                    "USB: interface=",
                );

                crate::serial::write_hex(
                    interface.interface_number as u64,
                );

                crate::serial::write_str(
                    " alt=",
                );

                crate::serial::write_hex(
                    interface.alternate_setting as u64,
                );

                crate::serial::write_str(
                    " class=",
                );

                crate::serial::write_hex(
                    interface.class as u64,
                );

                crate::serial::write_str(
                    " subclass=",
                );

                crate::serial::write_hex(
                    interface.subclass as u64,
                );

                crate::serial::write_str(
                    " protocol=",
                );

                crate::serial::write_hex(
                    interface.protocol as u64,
                );

                crate::serial::write_str(
                    "\n",
                );

                // ------------------------------------------------
                // Endpoint descriptors
                // ------------------------------------------------

                for endpoint in &interface.endpoints {
                    crate::serial::write_str(
                        "USB: endpoint address=",
                    );

                    crate::serial::write_hex(
                        endpoint.address as u64,
                    );

                    crate::serial::write_str(
                        " max_packet=",
                    );

                    crate::serial::write_hex(
                        endpoint.max_packet_size as u64,
                    );

                    crate::serial::write_str(
                        " interval=",
                    );

                    crate::serial::write_hex(
                        endpoint.interval as u64,
                    );

                    crate::serial::write_str(
                        "\n",
                    );

                    // ------------------------------------------------
                    // Direction
                    // ------------------------------------------------

                    if endpoint.address
                        & USB_ENDPOINT_DIRECTION_IN
                        != 0
                    {
                        crate::serial::write_str(
                            "USB: endpoint direction=IN\n",
                        );
                    } else {
                        crate::serial::write_str(
                            "USB: endpoint direction=OUT\n",
                        );
                    }

                    // ------------------------------------------------
                    // Transfer type
                    //
                    // We intentionally do not reference usb_if
                    // here. This keeps this module independent
                    // from the private usb-if crate path.
                    // ------------------------------------------------

                    crate::serial::write_str(
                        "USB: endpoint transfer_type=",
                    );

                    crate::serial::write_hex(
                        endpoint.transfer_type as u64,
                    );

                    crate::serial::write_str(
                        "\n",
                    );
                }
            }
        }
    }
}

// ============================================================
// Find HID interface in an opened device
// ============================================================

pub fn find_hid_interface(
    device: &Device,
) -> Option<HidInterface> {
    let configurations =
        device.configurations();

    for configuration in configurations {
        for interface_group in &configuration.interfaces {
            for interface in &interface_group.alt_settings {
                if interface.class != USB_CLASS_HID {
                    continue;
                }

                if interface.subclass != HID_SUBCLASS_BOOT {
                    continue;
                }

                let kind =
                    match interface.protocol {
                        HID_PROTOCOL_KEYBOARD => {
                            HidDeviceKind::Keyboard
                        }

                        HID_PROTOCOL_MOUSE => {
                            HidDeviceKind::Mouse
                        }

                        _ => {
                            continue;
                        }
                    };

                return Some(
                    HidInterface {
                        interface_number:
                        interface.interface_number,

                        alternate_setting:
                        interface.alternate_setting,

                        kind,
                    },
                );
            }
        }
    }

    None
}

// ============================================================
// Find HID interrupt IN endpoint
// ============================================================
//
// This function ONLY examines descriptors.
//
// It does NOT:
//
// - claim the interface
// - create an endpoint
// - start a transfer
//
// ============================================================

pub fn find_hid_interrupt_in_endpoint(
    device: &Device,
    interface_number: u8,
    alternate_setting: u8,
) -> Option<HidInterruptEndpoint> {
    let configurations =
        device.configurations();

    for configuration in configurations {
        for interface_group in &configuration.interfaces {
            for interface in &interface_group.alt_settings {
                if interface.interface_number
                    != interface_number
                {
                    continue;
                }

                if interface.alternate_setting
                    != alternate_setting
                {
                    continue;
                }

                for endpoint in &interface.endpoints {
                    // ------------------------------------------------
                    // Must be IN.
                    // ------------------------------------------------

                    if endpoint.address
                        & USB_ENDPOINT_DIRECTION_IN
                        == 0
                    {
                        continue;
                    }

                    // ------------------------------------------------
                    // Must be interrupt.
                    //
                    // Use a numeric comparison here so this file
                    // does not need to import usb_if.
                    // ------------------------------------------------

                    if endpoint.transfer_type as u8
                        != USB_TRANSFER_TYPE_INTERRUPT
                    {
                        continue;
                    }

                    let result =
                        HidInterruptEndpoint {
                            address:
                            endpoint.address,

                            max_packet_size:
                            endpoint.max_packet_size,

                            interval:
                            endpoint.interval,
                        };

                    crate::serial::write_str(
                        "USB HID: interrupt IN endpoint found address=",
                    );

                    crate::serial::write_hex(
                        result.address as u64,
                    );

                    crate::serial::write_str(
                        " max_packet=",
                    );

                    crate::serial::write_hex(
                        result.max_packet_size as u64,
                    );

                    crate::serial::write_str(
                        " interval=",
                    );

                    crate::serial::write_hex(
                        result.interval as u64,
                    );

                    crate::serial::write_str(
                        "\n",
                    );

                    return Some(result);
                }
            }
        }
    }

    crate::serial::write_str(
        "USB HID: no interrupt IN endpoint found\n",
    );

    None
}

// ============================================================
// Prepare / claim HID interface
// ============================================================
//
// CrabUSB 0.12.x:
//
//     device.claim_interface(...).await
//
// returns an Interface object.
//
// The Interface then provides the endpoint APIs.
//
// IMPORTANT:
//
// This function intentionally does NOT return the Interface.
// The actual transfer should be performed by the caller while
// the returned Interface is alive.
//
// ============================================================

pub async fn claim_hid_interface(
    device: &mut Device,
    interface: HidInterface,
) -> Result<(), ()> {
    crate::serial::write_str(
        "USB HID: claiming interface=",
    );

    crate::serial::write_hex(
        interface.interface_number as u64,
    );

    crate::serial::write_str(
        " alt=",
    );

    crate::serial::write_hex(
        interface.alternate_setting as u64,
    );

    crate::serial::write_str(
        "\n",
    );

    match device
        .claim_interface(
            interface.interface_number,
            interface.alternate_setting,
        )
        .await
    {
        Ok(_) => {
            crate::serial::write_str(
                "USB HID: interface claimed\n",
            );

            Ok(())
        }

        Err(_) => {
            crate::serial::write_str(
                "USB HID: failed to claim interface\n",
            );

            Err(())
        }
    }
}

// ============================================================
// HID keyboard report parsing
// ============================================================
//
// Standard Boot Keyboard:
//
// byte 0 = modifier
// byte 1 = reserved
// byte 2..7 = six simultaneous key usages
//
// ============================================================

pub fn parse_keyboard_report(
    report: &[u8],
) {
    if report.len() < 2 {
        crate::serial::write_str(
            "USB KEYBOARD: invalid report\n",
        );

        return;
    }

    let modifier =
        report[0];

    crate::serial::write_str(
        "USB KEYBOARD: modifier=",
    );

    crate::serial::write_hex(
        modifier as u64,
    );

    crate::serial::write_str(
        "\n",
    );

    let key_count =
        core::cmp::min(
            report.len(),
            8,
        );

    if key_count <= 2 {
        return;
    }

    for key_index in 2..key_count {
        let usage =
            report[key_index];

        if usage == 0 {
            continue;
        }

        crate::serial::write_str(
            "USB KEYBOARD: key usage=",
        );

        crate::serial::write_hex(
            usage as u64,
        );

        crate::serial::write_str(
            "\n",
        );
    }
}

// ============================================================
// HID mouse report parsing
// ============================================================
//
// Standard Boot Mouse:
//
// byte 0 = buttons
// byte 1 = X movement
// byte 2 = Y movement
//
// ============================================================

pub fn parse_mouse_report(
    report: &[u8],
) {
    if report.len() < 3 {
        crate::serial::write_str(
            "USB MOUSE: invalid report\n",
        );

        return;
    }

    let buttons =
        report[0];

    let x =
        report[1] as i8;

    let y =
        report[2] as i8;

    crate::serial::write_str(
        "USB MOUSE: buttons=",
    );

    crate::serial::write_hex(
        buttons as u64,
    );

    crate::serial::write_str(
        " x=",
    );

    crate::serial::write_hex(
        x as i64 as u64,
    );

    crate::serial::write_str(
        " y=",
    );

    crate::serial::write_hex(
        y as i64 as u64,
    );

    crate::serial::write_str(
        "\n",
    );
}