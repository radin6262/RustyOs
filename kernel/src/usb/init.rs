use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    ptr::NonNull,
    sync::atomic::{
        AtomicU8,
        Ordering,
    },
};

use alloc::vec::Vec;

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

// ============================================================
// Runtime
// ============================================================
//
// CrabUSB uses KernelOp for operations that need to interact
// with the kernel, such as delays.
//
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
// USB state
// ============================================================
//
// This contains the persistent CrabUSB state.
//
// The actual device classification is handled by devices.rs.
//
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
    //
    // These are kept alive after open_device() succeeds.
    //
    // --------------------------------------------------------

    pub(crate) devices:
        Vec<Device>,

    // --------------------------------------------------------
    // Initial device scan
    // --------------------------------------------------------
    //
    // Prevents the initial probe from being performed every
    // time usb::poll() runs.
    //
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
// The caller must ensure that USB state is not accessed
// concurrently.
//
// Rusty's current kernel USB polling path is single-threaded,
// so this is currently acceptable.
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

// ============================================================
// Initialize USB
// ============================================================
//
// Initialization pipeline:
//
// 1. Find PCI xHCI controller
// 2. Map xHCI MMIO
// 3. Create USB DMA allocator/device
// 4. Create CrabUSB xHCI host
// 5. Create CrabUSB event handler
// 6. Initialize xHCI
// 7. Publish USB state
//
// Device probing and HID classification happen later from
// usb::poll().
//
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
    //
    // CrabUSB needs DMA-capable memory for:
    //
    // - command/event rings
    // - device contexts
    // - transfer rings
    // - USB transfer buffers
    //
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
    //
    // This is used by the kernel polling loop and by the
    // synchronous future executor in poll.rs.
    //
    // ========================================================

    let event_handler =
        host.create_event_handler();

    serial::write_str(
        "USB: event handler created\n",
    );

    // ========================================================
    // Initialize xHCI controller
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
    //
    // From this point onward usb::poll() may access the state.
    //
    // IMPORTANT:
    //
    // USB_INITIALIZED is intentionally set only AFTER the
    // entire UsbState has been initialized.
    //
    // ========================================================

    unsafe {
        (*USB_STORAGE.state.get())
            .write(
                UsbState {
                    host,
                    event_handler,

                    devices:
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