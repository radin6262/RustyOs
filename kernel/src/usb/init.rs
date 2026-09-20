use core::{
    cell::UnsafeCell,
    future::Future,
    mem::MaybeUninit,
    pin::Pin,
    ptr::NonNull,
    sync::atomic::{
        AtomicU8,
        Ordering,
    },
};

use alloc::{
    boxed::Box,
    vec::Vec,
};

use crab_usb::{
    Device,
    EventHandler,
    KernelOp,
    USBHost,
};

use crate::{
    memory,
    serial,
    xhci_pci,
};

use crate::memory::dma::usb_device_dma;

use super::devices::HidDeviceKind;

// ============================================================
// Runtime
// ============================================================

pub struct RustyUsbRuntime;

impl KernelOp for RustyUsbRuntime {
    fn delay(
        &self,
        duration: core::time::Duration,
    ) {
        crate::delay::delay(
            duration,
        );
    }
}

static USB_RUNTIME:
RustyUsbRuntime =
    RustyUsbRuntime;

// ============================================================
// HID input task
// ============================================================
//
// We intentionally do NOT name CrabUSB's Interface or Endpoint
// types here.
//
// CrabUSB 0.12.x returns those values from:
//
//     device.claim_interface(...)
//     interface.endpoint_bulk_in(...)
//
// The concrete types are kept inside this boxed future.
//
// This allows the future to own the interface + endpoint for
// the lifetime of the HID device without depending on private
// CrabUSB type names.
//
// ============================================================

pub(crate) type HidInputTask =
Pin<
    Box<
        dyn Future<Output = ()> + 'static
    >
>;

// ============================================================
// HID input state
// ============================================================

pub(crate) struct HidInputDevice {
    // --------------------------------------------------------
    // HID device type
    // --------------------------------------------------------

    pub(crate) kind:
        HidDeviceKind,

    // --------------------------------------------------------
    // USB endpoint address
    // --------------------------------------------------------

    pub(crate) endpoint_address:
        u8,

    // --------------------------------------------------------
    // Maximum interrupt packet size
    // --------------------------------------------------------

    pub(crate) packet_size:
        usize,

    // --------------------------------------------------------
    // Persistent HID transfer task
    // --------------------------------------------------------

    pub(crate) task:
        HidInputTask,
}

// ============================================================
// USB state
// ============================================================

pub(crate) struct UsbState {
    // --------------------------------------------------------
    // CrabUSB host
    // --------------------------------------------------------

    pub(crate) host:
        USBHost,

    // --------------------------------------------------------
    // xHCI event handler
    // --------------------------------------------------------

    pub(crate) event_handler:
        EventHandler,

    // --------------------------------------------------------
    // Opened USB devices
    // --------------------------------------------------------

    pub(crate) devices:
        Vec<Device>,

    // --------------------------------------------------------
    // HID devices
    // --------------------------------------------------------

    pub(crate) hid_devices:
        Vec<HidInputDevice>,

    // --------------------------------------------------------
    // Initial device scan
    // --------------------------------------------------------

    pub(crate) devices_scanned:
        bool,
}

// ============================================================
// Global USB storage
// ============================================================

struct UsbStorage {
    state:
        UnsafeCell<
            MaybeUninit<
                UsbState,
            >,
        >,
}

unsafe impl Sync
for UsbStorage {
}

impl UsbStorage {
    const fn new() -> Self {
        Self {
            state:
            UnsafeCell::new(
                MaybeUninit::uninit(),
            ),
        }
    }
}

static USB_STORAGE:
UsbStorage =
    UsbStorage::new();

static USB_INITIALIZED:
AtomicU8 =
    AtomicU8::new(0);

// ============================================================
// Access global USB state
// ============================================================
//
// SAFETY:
//
// Rusty's USB subsystem currently runs through the
// single-threaded kernel polling path.
//
// ============================================================

pub(crate) unsafe fn state_mut()
    -> &'static mut UsbState
{
    if USB_INITIALIZED.load(
        Ordering::Acquire,
    ) == 0 {
        panic!(
            "Rusty: USB state accessed before initialization",
        );
    }

    unsafe {
        &mut *(
            (*USB_STORAGE.state.get())
                .as_mut_ptr()
        )
    }
}

// ============================================================
// Initialization status
// ============================================================

pub fn is_initialized()
    -> bool
{
    USB_INITIALIZED.load(
        Ordering::Acquire,
    ) != 0
}

// Check xhci ports
unsafe fn power_on_xhci_ports(
    bar0: usize,
) {
    //
    // xHCI capability registers
    //
    let cap_base =
        bar0 as *mut u8;

    let cap_length =
        core::ptr::read_volatile(
            cap_base
                .add(0x00)
        ) as usize;


    //
    // Operational registers start here
    //
    let op_base =
        bar0 + cap_length;


    //
    // Read HCSPARAMS1
    //
    let hcsparams1 =
        core::ptr::read_volatile(
            (cap_base.add(0x04))
                as *const u32,
        );


    let max_ports =
        (hcsparams1 & 0xff)
            as usize;


    serial::write_str(
        "USB: xHCI ports=",
    );

    serial::write_usize(
        max_ports,
    );

    serial::write_str(
        "\n",
    );


    //
    // PORTSC registers
    //
    // PORTSC1 = operational + 0x400
    //
    let portsc_base =
        op_base + 0x400;


    for port in 0..max_ports {

        let portsc =
            (portsc_base
                + port * 0x10)
                as *mut u32;


        let mut value =
            core::ptr::read_volatile(
                portsc,
            );


        //
        // Port Power bit
        //
        // xHCI spec:
        // PORTSC.PP = bit 9
        //
        value |= 1 << 9;


        core::ptr::write_volatile(
            portsc,
            value,
        );
    }


    serial::write_str(
        "USB: xHCI ports powered\n",
    );
}

// ============================================================
// Initialize USB
// ============================================================

pub fn init() {
    // --------------------------------------------------------
    // Prevent double initialization.
    // --------------------------------------------------------

    if USB_INITIALIZED.load(
        Ordering::Acquire,
    ) != 0 {
        serial::write_str(
            "USB: already initialized\n",
        );

        return;
    }

    serial::write_str(
        "USB: subsystem initialization started\n",
    );

    // ========================================================
    // Find xHCI controller
    // ========================================================

    serial::write_str(
        "USB: searching for xHCI controller...\n",
    );

    let controller =
        match xhci_pci::find_xhci()
        {
            Some(controller) => {
                controller
            }

            None => {
                serial::write_str(
                    "USB: no xHCI controller found\n",
                );

                return;
            }
        };

    serial::write_str(
        "USB: xHCI controller found\n",
    );

    serial::write_str(
        "USB: BAR0=",
    );

    serial::write_hex(
        controller.bar0,
    );

    serial::write_str(
        " size=",
    );

    serial::write_hex(
        controller.bar0_size,
    );

    serial::write_str(
        "\n",
    );

    // ========================================================
    // Validate BAR0
    // ========================================================

    let bar0 =
        controller.bar0
            as usize;

    let bar0_size =
        controller.bar0_size
            as usize;

    if bar0 == 0 {
        panic!(
            "Rusty: xHCI BAR0 is zero",
        );
    }

    if bar0_size == 0 {
        panic!(
            "Rusty: xHCI BAR0 size is zero",
        );
    }

    // ========================================================
    // Map xHCI MMIO
    // ========================================================

    unsafe {
        memory::identity_map_mmio(
            bar0,
            bar0_size,
        );
    }

    serial::write_str(
        "USB: xHCI MMIO mapped\n",
    );

    // ========================================================
    // Create USB DMA device
    // ========================================================

    let dma =
        usb_device_dma();

    serial::write_str(
        "USB: DMA device ready\n",
    );

    // ========================================================
    // Create MMIO pointer
    // ========================================================

    let mmio =
        NonNull::new(
            bar0 as *mut u8,
        )
            .expect(
                "Rusty: invalid xHCI MMIO pointer",
            );

    // ========================================================
    // Create CrabUSB xHCI host
    // ========================================================

    serial::write_str(
        "USB: creating CrabUSB xHCI host...\n",
    );

    let mut host =
        match USBHost::new_xhci(
            mmio,
            dma,
            &USB_RUNTIME,
        ) {
            Ok(host) => {
                host
            }

            Err(error) => {
                panic!(
                    "Rusty: CrabUSB xHCI host creation failed: {:?}",
                    error,
                );
            }
        };

    serial::write_str(
        "USB: CrabUSB host created\n",
    );

    // ========================================================
    // Create event handler
    // ========================================================

    let event_handler =
        host.create_event_handler();

    serial::write_str(
        "USB: event handler created\n",
    );

    // ========================================================
    // Initialize xHCI
    // ========================================================

    serial::write_str(
        "USB: initializing xHCI controller...\n",
    );

    let result =
        crate::usb::poll::block_on_usb(
            host.init(),
            &event_handler,
        );

    match result {
        Ok(()) => {
            serial::write_str(
                "USB: xHCI controller initialized\n",
            );

            // ====================================================
            // DEBUG: dump xHCI ports
            // ====================================================

            xhci_pci::debug_dump_ports(
                bar0,
            );
        }

        Err(error) => {
            panic!(
                "Rusty: CrabUSB xHCI initialization failed: {:?}",
                error,
            );
        }
    }

    // ========================================================
    // Publish USB state
    // ========================================================

    unsafe {
        (*USB_STORAGE.state.get())
            .write(
                UsbState {
                    host,

                    event_handler,

                    devices:
                    Vec::new(),

                    hid_devices:
                    Vec::new(),

                    devices_scanned:
                    false,
                },
            );
    }

    USB_INITIALIZED.store(
        1,
        Ordering::Release,
    );

    serial::write_str(
        "USB: subsystem initialized\n",
    );
}