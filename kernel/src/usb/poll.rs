use core::{
    future::Future,
    pin::Pin,
    ptr,
    task::{
        Context,
        Poll,
        RawWaker,
        RawWakerVTable,
        Waker,
    },
};

use crab_usb::{
    EventHandler,
    ProbedDevice,
};

use super::{
    devices,
    init,
};

// ============================================================
// USB polling
// ============================================================
//
// Called repeatedly from Rusty's kernel loop.
//
// Pipeline:
//
// 1. process pending xHCI events
// 2. probe for connected/disconnected USB devices
// 3. inspect USB interfaces
// 4. classify HID devices
// 5. open newly discovered USB devices
// 6. claim HID interfaces
// 7. store opened devices
//
// devices.rs owns USB/HID classification.
//
// poll.rs owns:
//
// - USB device lifetime
// - USB polling
// - interface claiming
//
// Actual HID transfers are performed after the devices have
// been successfully opened and claimed.
//
// ============================================================

pub fn poll() {
    crate::serial::write_str(
        "USB POLL: entered\n",
    );

    // ========================================================
    // Check initialization
    // ========================================================

    if !init::is_initialized() {
        crate::serial::write_str(
            "USB POLL: subsystem not initialized\n",
        );

        return;
    }

    crate::serial::write_str(
        "USB POLL: subsystem initialized\n",
    );

    unsafe {
        let state =
            init::state_mut();

        // ====================================================
        // Process pending xHCI events
        // ====================================================

        crate::serial::write_str(
            "USB POLL: processing xHCI events...\n",
        );

        state
            .event_handler
            .handle_event();

        crate::serial::write_str(
            "USB POLL: xHCI event processing complete\n",
        );

        // ====================================================
        // Initial USB device scan
        // ====================================================

        if !state.devices_scanned {
            crate::serial::write_str(
                "USB POLL: starting USB device probe...\n",
            );

            crate::serial::write_str(
                "USB POLL: calling host.probe_devices()...\n",
            );

            let result =
                block_on_usb(
                    state.host.probe_devices(),
                    &state.event_handler,
                );

            crate::serial::write_str(
                "USB POLL: host.probe_devices() returned\n",
            );

            match result {
                Ok(changes) => {
                    // ========================================
                    // Probe succeeded
                    // ========================================

                    crate::serial::write_str(
                        "USB POLL: device probe succeeded\n",
                    );

                    crate::serial::write_str(
                        "USB POLL: connected count=",
                    );

                    crate::serial::write_hex(
                        changes.connected.len() as u64,
                    );

                    crate::serial::write_str(
                        "\n",
                    );

                    crate::serial::write_str(
                        "USB POLL: disconnected count=",
                    );

                    crate::serial::write_hex(
                        changes.disconnected.len() as u64,
                    );

                    crate::serial::write_str(
                        "\n",
                    );

                    // ========================================
                    // Process connected devices
                    // ========================================

                    for (
                        index,
                        probed_device,
                    ) in changes
                        .connected
                        .into_iter()
                        .enumerate()
                    {
                        match probed_device {
                            // ====================================
                            // Normal USB device
                            // ====================================

                            ProbedDevice::Device(info) => {
                                crate::serial::write_str(
                                    "USB POLL: connected device #",
                                );

                                crate::serial::write_hex(
                                    index as u64,
                                );

                                crate::serial::write_str(
                                    "\n",
                                );

                                crate::serial::write_str(
                                    "USB POLL: device discovered\n",
                                );

                                // --------------------------------
                                // VID / PID
                                // --------------------------------

                                crate::serial::write_str(
                                    "USB POLL: vendor_id=",
                                );

                                crate::serial::write_hex(
                                    info.vendor_id() as u64,
                                );

                                crate::serial::write_str(
                                    " product_id=",
                                );

                                crate::serial::write_hex(
                                    info.product_id() as u64,
                                );

                                crate::serial::write_str(
                                    "\n",
                                );

                                // ====================================
                                // Inspect descriptors
                                // ====================================

                                crate::serial::write_str(
                                    "USB POLL: inspecting USB interfaces...\n",
                                );

                                devices::inspect_device_info(
                                    &info,
                                );

                                // ====================================
                                // Open device
                                // ====================================

                                crate::serial::write_str(
                                    "USB POLL: opening device #",
                                );

                                crate::serial::write_hex(
                                    index as u64,
                                );

                                crate::serial::write_str(
                                    "...\n",
                                );

                                let open_result =
                                    block_on_usb(
                                        state.host.open_device(
                                            &info,
                                        ),
                                        &state.event_handler,
                                    );

                                crate::serial::write_str(
                                    "USB POLL: open_device() returned\n",
                                );

                                match open_result {
                                    Ok(mut device) => {
                                        crate::serial::write_str(
                                            "USB POLL: device opened successfully\n",
                                        );

                                        // ====================================================
                                        // Inspect opened device
                                        // ====================================================

                                        devices::inspect_open_device(
                                            &device,
                                        );

                                        // ====================================================
                                        // Find supported HID interface
                                        // ====================================================

                                        let hid_interface =
                                            devices::find_hid_interface(
                                                &device,
                                            );

                                        match hid_interface {
                                            Some(interface) => {
                                                // ============================================
                                                // Report device type
                                                // ============================================

                                                match interface.kind {
                                                    devices::HidDeviceKind::Keyboard => {
                                                        crate::serial::write_str(
                                                            "USB POLL: opened device is a keyboard\n",
                                                        );
                                                    }

                                                    devices::HidDeviceKind::Mouse => {
                                                        crate::serial::write_str(
                                                            "USB POLL: opened device is a mouse\n",
                                                        );
                                                    }
                                                }

                                                // ============================================
                                                // Find interrupt IN endpoint descriptor
                                                // ============================================

                                                let endpoint =
                                                    devices::find_hid_interrupt_in_endpoint(
                                                        &device,
                                                        interface.interface_number,
                                                        interface.alternate_setting,
                                                    );

                                                match endpoint {
                                                    Some(endpoint) => {
                                                        crate::serial::write_str(
                                                            "USB POLL: HID interrupt IN endpoint found\n",
                                                        );

                                                        crate::serial::write_str(
                                                            "USB POLL: endpoint=",
                                                        );

                                                        crate::serial::write_hex(
                                                            endpoint.address as u64,
                                                        );

                                                        crate::serial::write_str(
                                                            " packet_size=",
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

                                                        // ========================================
                                                        // Claim HID interface
                                                        // ========================================
                                                        //
                                                        // IMPORTANT:
                                                        //
                                                        // We do NOT call prepare_hid_device().
                                                        //
                                                        // That function no longer exists.
                                                        //
                                                        // CrabUSB 0.12.x returns the Interface from
                                                        // claim_interface(). The Interface is what
                                                        // owns the endpoint access.
                                                        //
                                                        // We therefore claim the interface here and
                                                        // keep the Device alive in state.devices.
                                                        //
                                                        // ========================================

                                                        crate::serial::write_str(
                                                            "USB POLL: claiming HID interface=",
                                                        );

                                                        crate::serial::write_hex(
                                                            interface.interface_number
                                                                as u64,
                                                        );

                                                        crate::serial::write_str(
                                                            " alt=",
                                                        );

                                                        crate::serial::write_hex(
                                                            interface.alternate_setting
                                                                as u64,
                                                        );

                                                        crate::serial::write_str(
                                                            "\n",
                                                        );

                                                        let claim_result =
                                                            block_on_usb(
                                                                device.claim_interface(
                                                                    interface.interface_number,
                                                                    interface.alternate_setting,
                                                                ),
                                                                &state.event_handler,
                                                            );

                                                        match claim_result {
                                                            Ok(_claimed_interface) => {
                                                                crate::serial::write_str(
                                                                    "USB POLL: HID interface claimed successfully\n",
                                                                );

                                                                crate::serial::write_str(
                                                                    "USB POLL: HID device is ready for input transfers\n",
                                                                );
                                                            }

                                                            Err(_) => {
                                                                crate::serial::write_str(
                                                                    "USB POLL: FAILED to claim HID interface\n",
                                                                );
                                                            }
                                                        }
                                                    }

                                                    None => {
                                                        crate::serial::write_str(
                                                            "USB POLL: HID interrupt IN endpoint not found\n",
                                                        );
                                                    }
                                                }
                                            }

                                            None => {
                                                crate::serial::write_str(
                                                    "USB POLL: opened device has no supported HID interface\n",
                                                );
                                            }
                                        }

                                        // ====================================================
                                        // Store opened device
                                        // ====================================================

                                        crate::serial::write_str(
                                            "USB POLL: storing opened device\n",
                                        );

                                        state.devices.push(
                                            device,
                                        );

                                        crate::serial::write_str(
                                            "USB POLL: device stored\n",
                                        );
                                    }

                                    Err(_) => {
                                        crate::serial::write_str(
                                            "USB POLL: FAILED to open device\n",
                                        );
                                    }
                                }
                            }

                            // ====================================
                            // USB hub
                            // ====================================

                            ProbedDevice::Hub(info) => {
                                crate::serial::write_str(
                                    "USB POLL: connected USB hub #",
                                );

                                crate::serial::write_hex(
                                    index as u64,
                                );

                                crate::serial::write_str(
                                    "\n",
                                );

                                crate::serial::write_str(
                                    "USB POLL: hub vendor_id=",
                                );

                                crate::serial::write_hex(
                                    info.vendor_id() as u64,
                                );

                                crate::serial::write_str(
                                    " product_id=",
                                );

                                crate::serial::write_hex(
                                    info.product_id() as u64,
                                );

                                crate::serial::write_str(
                                    "\n",
                                );

                                crate::serial::write_str(
                                    "USB POLL: hub opening is not handled yet\n",
                                );
                            }
                        }
                    }

                    // ========================================
                    // Report disconnected devices
                    // ========================================

                    for device_id
                    in changes.disconnected
                    {
                        crate::serial::write_str(
                            "USB POLL: disconnected device id=",
                        );

                        crate::serial::write_hex(
                            device_id as u64,
                        );

                        crate::serial::write_str(
                            "\n",
                        );
                    }

                    // ========================================
                    // Device count
                    // ========================================

                    crate::serial::write_str(
                        "USB POLL: stored device count=",
                    );

                    crate::serial::write_hex(
                        state.devices.len() as u64,
                    );

                    crate::serial::write_str(
                        "\n",
                    );

                    // ========================================
                    // Initial scan complete
                    // ========================================

                    state.devices_scanned =
                        true;

                    crate::serial::write_str(
                        "USB POLL: initial device scan complete\n",
                    );
                }

                Err(_) => {
                    crate::serial::write_str(
                        "USB POLL: device probe FAILED\n",
                    );
                }
            }
        } else {
            // ====================================================
            // Subsequent polls
            // ====================================================

            crate::serial::write_str(
                "USB POLL: initial device scan already completed\n",
            );

            crate::serial::write_str(
                "USB POLL: currently stored devices=",
            );

            crate::serial::write_hex(
                state.devices.len() as u64,
            );

            crate::serial::write_str(
                "\n",
            );
        }
    }

    crate::serial::write_str(
        "USB POLL: leaving\n",
    );
}

// ============================================================
// Small synchronous executor for CrabUSB futures
// ============================================================
//
// Rusty does not currently have a general async executor.
//
// While a CrabUSB future is pending:
//
// 1. poll the future
// 2. process xHCI events
// 3. poll again
//
// ============================================================

pub(crate) fn block_on_usb<F>(
    future: F,
    event_handler: &EventHandler,
) -> F::Output
where
    F: Future,
{
    crate::serial::write_str(
        "USB FUTURE: starting\n",
    );

    let waker =
        noop_waker();

    crate::serial::write_str(
        "USB FUTURE: waker created\n",
    );

    let mut context =
        Context::from_waker(
            &waker,
        );

    crate::serial::write_str(
        "USB FUTURE: context created\n",
    );

    let mut future =
        future;

    // SAFETY:
    //
    // The future remains at this exact memory location until
    // Poll::Ready is returned.
    let mut future =
        unsafe {
            Pin::new_unchecked(
                &mut future,
            )
        };

    crate::serial::write_str(
        "USB FUTURE: future pinned\n",
    );

    let mut poll_count:
        u64 = 0;

    loop {
        poll_count += 1;

        crate::serial::write_str(
            "USB FUTURE: poll #",
        );

        crate::serial::write_hex(
            poll_count,
        );

        crate::serial::write_str(
            "\n",
        );

        match Future::poll(
            future.as_mut(),
            &mut context,
        ) {
            Poll::Ready(value) => {
                crate::serial::write_str(
                    "USB FUTURE: READY\n",
                );

                return value;
            }

            Poll::Pending => {
                crate::serial::write_str(
                    "USB FUTURE: PENDING\n",
                );

                crate::serial::write_str(
                    "USB FUTURE: processing xHCI events\n",
                );

                event_handler
                    .handle_event();

                crate::serial::write_str(
                    "USB FUTURE: xHCI events processed\n",
                );

                core::hint::spin_loop();
            }
        }
    }
}

// ============================================================
// No-op waker
// ============================================================

fn noop_waker() -> Waker {
    unsafe fn clone(
        _: *const (),
    ) -> RawWaker {
        RawWaker::new(
            ptr::null(),
            &VTABLE,
        )
    }

    unsafe fn wake(
        _: *const (),
    ) {
    }

    unsafe fn wake_by_ref(
        _: *const (),
    ) {
    }

    unsafe fn drop(
        _: *const (),
    ) {
    }

    static VTABLE:
    RawWakerVTable =
        RawWakerVTable::new(
            clone,
            wake,
            wake_by_ref,
            drop,
        );

    unsafe {
        Waker::from_raw(
            RawWaker::new(
                ptr::null(),
                &VTABLE,
            ),
        )
    }
}