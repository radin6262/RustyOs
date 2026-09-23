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

use alloc::{
    boxed::Box,
    vec::Vec,
};

use crab_usb::{
    usb_if::endpoint::TransferRequest,
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
// Normal polling is intentionally SILENT.
//
// High-frequency serial logging inside this function can
// completely dominate the kernel loop because UART output is
// dramatically slower than CPU execution.
//
// Meaningful events are still logged:
//   - device discovery
//   - HID device setup
//   - HID task startup
//   - completed HID transfers
//   - HID transfer errors
//   - device disconnects
//
// Pipeline:
//
// 1. process pending xHCI events
// 2. poll existing HID transfers
// 3. process xHCI events again
// 4. probe for connected USB devices
// 5. inspect USB interfaces
// 6. classify HID devices
// 7. open newly discovered USB devices
// 8. claim HID interfaces
// 9. obtain EndpointHandle
// 10. perform persistent HID interrupt-IN transfers
// 11. keep opened Device alive in UsbState
//
// CrabUSB API:
//
// Device::claim_interface()
//     -> Future<Output = Result<InterfaceSession, USBError>>
//
// InterfaceSession::endpoint()
//     -> Result<EndpointHandle, USBError>
//
// EndpointHandle::wait()
//     -> Future<
//          Output = Result<TransferCompletion, TransferError>
//        >
//
// TransferRequest::interrupt_in(&mut [u8])
//
// ============================================================

pub fn poll() {
    if !init::is_initialized() {
        return;
    }

    unsafe {
        let state = init::state_mut();

        // ====================================================
        // Process xHCI events
        // ====================================================
        //
        // This handles completions from transfers that were
        // submitted during previous polls.

        state.event_handler.handle_event();

        // ====================================================
        // Poll existing HID input tasks
        // ====================================================

        if !state.hid_devices.is_empty() {
            let waker = noop_waker();

            let mut context =
                Context::from_waker(
                    &waker,
                );

            for hid in
                state.hid_devices.iter_mut()
            {
                match hid.task.as_mut().poll(
                    &mut context,
                ) {
                    Poll::Ready(()) => {
                        // HID tasks are designed to run forever.
                        //
                        // Reaching Ready therefore means the
                        // task unexpectedly stopped.
                        crate::serial::write_str(
                            "USB HID: input task stopped\n",
                        );
                    }

                    Poll::Pending => {
                        // Normal state.
                        //
                        // DO NOT log this. This function runs
                        // continuously and Pending is expected
                        // while waiting for the USB controller.
                    }
                }
            }
        }

        // ====================================================
        // Process xHCI events AGAIN
        // ====================================================
        //
        // A HID task can submit a new transfer while being
        // polled above. Process the event ring again so that
        // completions generated during this polling cycle are
        // consumed before leaving poll().

        state.event_handler.handle_event();

        // ====================================================
        // Initial USB device scan
        // ====================================================

        if state.devices_scanned {
            return;
        }

        // ====================================================
        // Probe USB devices
        // ====================================================

        let result =
            block_on_usb(
                state.host.probe_devices(),
                &state.event_handler,
            );

        match result {
            Ok(changes) => {
                // ====================================================
                // Connected devices
                // ====================================================

                for (
                    index,
                    probed_device,
                ) in changes.connected
                    .into_iter()
                    .enumerate()
                {
                    match probed_device {
                        // ====================================================
                        // Normal USB device
                        // ====================================================

                        ProbedDevice::Device(info) => {
                            crate::serial::write_str(
                                "USB: device connected #",
                            );

                            crate::serial::write_hex(
                                index as u64,
                            );

                            crate::serial::write_str(
                                " VID=",
                            );

                            crate::serial::write_hex(
                                info.vendor_id() as u64,
                            );

                            crate::serial::write_str(
                                " PID=",
                            );

                            crate::serial::write_hex(
                                info.product_id() as u64,
                            );

                            crate::serial::write_str(
                                "\n",
                            );

                            // ====================================================
                            // Inspect descriptors
                            // ====================================================

                            devices::inspect_device_info(
                                &info,
                            );

                            // ====================================================
                            // Open device
                            // ====================================================

                            let open_result =
                                block_on_usb(
                                    state.host.open_device(
                                        &info,
                                    ),
                                    &state.event_handler,
                                );

                            // IMPORTANT:
                            //
                            // Device must remain mutable because
                            // claim_interface() requires &mut self.
                            //
                            let mut device =
                                match open_result {
                                    Ok(device) => {
                                        device
                                    }

                                    Err(_) => {
                                        crate::serial::write_str(
                                            "USB: failed to open device\n",
                                        );

                                        continue;
                                    }
                                };

                            // ====================================================
                            // Inspect opened device
                            // ====================================================

                            devices::inspect_open_device(
                                &device,
                            );

                            // ====================================================
                            // Find HID interface
                            // ====================================================

                            let hid_interface =
                                match devices::find_hid_interface(
                                    &device,
                                ) {
                                    Some(interface) => {
                                        interface
                                    }

                                    None => {
                                        // Not an HID device.
                                        //
                                        // Keep the Device alive anyway.
                                        state.devices.push(
                                            device,
                                        );

                                        continue;
                                    }
                                };

                            // ====================================================
                            // HID type
                            // ====================================================

                            let kind =
                                hid_interface.kind;

                            match kind {
                                devices::HidDeviceKind::Keyboard => {
                                    crate::serial::write_str(
                                        "USB: HID keyboard detected\n",
                                    );
                                }

                                devices::HidDeviceKind::Mouse => {
                                    crate::serial::write_str(
                                        "USB: HID mouse detected\n",
                                    );
                                }
                            }

                            // ====================================================
                            // Find interrupt IN endpoint
                            // ====================================================

                            let endpoint =
                                match devices::find_hid_interrupt_in_endpoint(
                                    &device,
                                    hid_interface.interface_number,
                                    hid_interface.alternate_setting,
                                ) {
                                    Some(endpoint) => {
                                        endpoint
                                    }

                                    None => {
                                        crate::serial::write_str(
                                            "USB: HID interrupt IN endpoint not found\n",
                                        );

                                        state.devices.push(
                                            device,
                                        );

                                        continue;
                                    }
                                };

                            crate::serial::write_str(
                                "USB: HID endpoint=",
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

                            // ====================================================
                            // Claim HID interface
                            // ====================================================

                            let interface =
                                match block_on_usb(
                                    device.claim_interface(
                                        hid_interface.interface_number,
                                        hid_interface.alternate_setting,
                                    ),
                                    &state.event_handler,
                                ) {
                                    Ok(interface) => {
                                        interface
                                    }

                                    Err(_) => {
                                        crate::serial::write_str(
                                            "USB: failed to claim HID interface\n",
                                        );

                                        state.devices.push(
                                            device,
                                        );

                                        continue;
                                    }
                                };

                            // ====================================================
                            // Save HID properties
                            // ====================================================

                            let endpoint_address =
                                endpoint.address;

                            let packet_size =
                                endpoint.max_packet_size
                                    as usize;

                            // ====================================================
                            // Create EndpointHandle
                            // ====================================================

                            let hid_endpoint =
                                match interface.endpoint(
                                    endpoint_address,
                                ) {
                                    Ok(endpoint) => {
                                        endpoint
                                    }

                                    Err(_) => {
                                        crate::serial::write_str(
                                            "USB: failed to create HID endpoint handle\n",
                                        );

                                        state.devices.push(
                                            device,
                                        );

                                        continue;
                                    }
                                };

                            // ====================================================
                            // Create persistent HID task
                            // ====================================================

                            let task:
                                init::HidInputTask =
                                Box::pin(
                                    async move {
                                        // ------------------------------------------------
                                        // One-time task startup logging
                                        // ------------------------------------------------

                                        match kind {
                                            devices::HidDeviceKind::Keyboard => {
                                                crate::serial::write_str(
                                                    "USB HID: keyboard task started\n",
                                                );
                                            }

                                            devices::HidDeviceKind::Mouse => {
                                                crate::serial::write_str(
                                                    "USB HID: mouse task started\n",
                                                );
                                            }
                                        }

                                        // ------------------------------------------------
                                        // Persistent HID report buffer
                                        // ------------------------------------------------

                                        let mut report =
                                            Vec::<u8>::with_capacity(
                                                packet_size,
                                            );

                                        report.resize(
                                            packet_size,
                                            0u8,
                                        );

                                        // ------------------------------------------------
                                        // Persistent interrupt-IN loop
                                        // ------------------------------------------------

                                        loop {
                                            let request =
                                                TransferRequest::interrupt_in(
                                                    &mut report,
                                                );

                                            let result =
                                                hid_endpoint
                                                    .wait(
                                                        request,
                                                    )
                                                    .await;

                                            match result {
                                                Ok(completion) => {
                                                    let actual_length =
                                                        core::cmp::min(
                                                            completion.actual_length,
                                                            report.len(),
                                                        );

                                                    // ------------------------------------------------
                                                    // Transfer completed
                                                    // ------------------------------------------------

                                                    crate::serial::write_str(
                                                        "USB HID: transfer completed length=",
                                                    );

                                                    crate::serial::write_hex(
                                                        actual_length as u64,
                                                    );

                                                    crate::serial::write_str(
                                                        "\n",
                                                    );

                                                    // ------------------------------------------------
                                                    // Parse report
                                                    // ------------------------------------------------

                                                    let report_slice =
                                                        &report[
                                                            ..actual_length
                                                            ];

                                                    match kind {
                                                        devices::HidDeviceKind::Keyboard => {
                                                            crate::input::process_keyboard_report(
                                                                report_slice,
                                                            );
                                                        }

                                                        devices::HidDeviceKind::Mouse => {
                                                            crate::input::process_mouse_report(
                                                                report_slice,
                                                            );
                                                        }
                                                    }
                                                }

                                                Err(_) => {
                                                    // ------------------------------------------------
                                                    // Transfer error
                                                    // ------------------------------------------------
                                                    //
                                                    // Do not spam this either.
                                                    // A future revision can add
                                                    // rate limiting if required.

                                                    crate::serial::write_str(
                                                        "USB HID: interrupt transfer error\n",
                                                    );
                                                }
                                            }
                                        }
                                    },
                                );

                            // ====================================================
                            // Store HID task
                            // ====================================================

                            state.hid_devices.push(
                                init::HidInputDevice {
                                    kind,
                                    endpoint_address,
                                    packet_size,
                                    task,
                                },
                            );

                            // ====================================================
                            // Keep opened Device alive
                            // ====================================================

                            state.devices.push(
                                device,
                            );

                            crate::serial::write_str(
                                "USB: HID device initialized\n",
                            );
                        }

                        // ====================================================
                        // USB hub
                        // ====================================================

                        ProbedDevice::Hub(info) => {
                            crate::serial::write_str(
                                "USB: hub connected VID=",
                            );

                            crate::serial::write_hex(
                                info.vendor_id() as u64,
                            );

                            crate::serial::write_str(
                                " PID=",
                            );

                            crate::serial::write_hex(
                                info.product_id() as u64,
                            );

                            crate::serial::write_str(
                                "\n",
                            );

                            // Hub support is not implemented yet.
                        }
                    }
                }

                // ====================================================
                // Disconnected devices
                // ====================================================

                for device_id in
                    changes.disconnected
                {
                    crate::serial::write_str(
                        "USB: device disconnected id=",
                    );

                    crate::serial::write_hex(
                        device_id as u64,
                    );

                    crate::serial::write_str(
                        "\n",
                    );
                }

                // ====================================================
                // Mark initial scan complete
                // ====================================================

                state.devices_scanned =
                    true;

                crate::serial::write_str(
                    "USB: initial device scan complete\n",
                );
            }

            Err(_) => {
                crate::serial::write_str(
                    "USB: device probe failed\n",
                );
            }
        }
    }
}

// ============================================================
// Synchronous USB future executor
// ============================================================
//
// Used for short-lived CrabUSB operations such as:
//
//   - probe_devices()
//   - open_device()
//   - claim_interface()
//   - controller initialization
//
// This is NOT used for persistent HID transfers.
//
// Persistent HID transfers are manually polled from poll().
//
// ============================================================

pub(crate) fn block_on_usb<F>(
    future: F,
    event_handler: &EventHandler,
) -> F::Output
where
    F: Future,
{
    let waker =
        noop_waker();

    let mut context =
        Context::from_waker(
            &waker,
        );

    let mut future =
        future;

    let mut future =
        unsafe {
            Pin::new_unchecked(
                &mut future,
            )
        };

    loop {
        match Future::poll(
            future.as_mut(),
            &mut context,
        ) {
            Poll::Ready(value) => {
                return value;
            }

            Poll::Pending => {
                // Process xHCI events while waiting
                // for the synchronous operation.
                event_handler.handle_event();

                core::hint::spin_loop();
            }
        }
    }
}

// ============================================================
// No-op waker
// ============================================================
//
// Rusty's kernel currently manually polls USB futures, so a
// scheduler-backed waker isn't required yet.
//
// The xHCI event handler is explicitly driven by poll().
//
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
            )
        )
    }
}