/*
 * Rusty xHCI driver implementation.
 *
 * THIS FILE IS driver.rs.
 * Module declarations belong in xhci/mod.rs.
 */

use core::sync::atomic::{Ordering, fence};

use alloc::vec::Vec;

use crate::memory;
use crate::usb::xhci::event_ring::EventRing;
use crate::usb::xhci::regs::XhciRegs;
use crate::usb::xhci::ring::Ring;
use crate::usb::xhci::trb::{Trb, TrbType};

/*
 * ==========================================================================
 * xHCI USBSTS
 * ==========================================================================
 */

const USBSTS_HCH: u32 = 1 << 0;

const USBSTS_HSE: u32 = 1 << 2;

const USBSTS_EINT: u32 = 1 << 3;

const USBSTS_PCD: u32 = 1 << 4;

const USBSTS_CNR: u32 = 1 << 11;

const USBSTS_HCE: u32 = 1 << 12;

/*
 * ==========================================================================
 * xHCI USBCMD
 * ==========================================================================
 */

const USBCMD_RUN: u32 = 1 << 0;

const USBCMD_RESET: u32 = 1 << 1;

const USBCMD_INTE: u32 = 1 << 2;

const USBCMD_HSEE: u32 = 1 << 3;

/*
 * ==========================================================================
 * xHCI IMAN
 * ==========================================================================
 */

const IMAN_IP: u32 = 1 << 0;

const IMAN_IE: u32 = 1 << 1;

/*
 * ==========================================================================
 * xHCI PORTSC
 * ==========================================================================
 */

const PORTSC_CCS: u32 = 1 << 0;

const PORTSC_PED: u32 = 1 << 1;

const PORTSC_PR: u32 = 1 << 4;

const PORTSC_PLS_MASK: u32 = 0xF << 5;

/*
 * USB3 SuperSpeed link states for which a warm reset is used to recover
 * the port/link.
 */
const PORTSC_PLS_INACTIVE: u8 = 6;
const PORTSC_PLS_COMPLIANCE: u8 = 10;

const PORTSC_PP: u32 = 1 << 9;

const PORTSC_SPEED_MASK: u32 = 0xF << 10;

const PORTSC_CSC: u32 = 1 << 17;

const PORTSC_PEC: u32 = 1 << 18;

const PORTSC_WRC: u32 = 1 << 19;

const PORTSC_OCC: u32 = 1 << 20;

const PORTSC_PRC: u32 = 1 << 21;

const PORTSC_PLC: u32 = 1 << 22;

const PORTSC_CEC: u32 = 1 << 23;

const PORTSC_CHANGE_MASK: u32 =
    PORTSC_CSC | PORTSC_PEC | PORTSC_WRC | PORTSC_OCC | PORTSC_PRC | PORTSC_PLC | PORTSC_CEC;

/*
 * ==========================================================================
 * xHCI Command Ring Control Register
 * ==========================================================================
 */

const CRCR_RCS: u64 = 1 << 0;

const CRCR_POINTER_MASK: u64 = !0x3F_u64;

/*
 * ==========================================================================
 * xHCI Capability / Operational offsets
 * ==========================================================================
 *
 * CAPLENGTH:
 *     Capability Registers +0x00, bits 7:0
 *
 * DBOFF:
 *     Capability Registers +0x14
 *
 * DNCTRL:
 *     Operational Registers +0x14
 * ==========================================================================
 */

const CAPLENGTH_OFFSET: usize = 0x00;

const DBOFF_OFFSET: usize = 0x14;

const DBOFF_MASK: u32 = !0x03;

const DEV_NOTIFICATION_OFFSET: usize = 0x14;

/*
 * Linux xhci.h:
 *
 *     DEV_NOTE_MASK
 *     DEV_NOTE_FWAKE
 */

const DEV_NOTE_MASK: u32 = 0xFFFF;

const DEV_NOTE_FWAKE: u32 = 1 << 1;

/*
 * ==========================================================================
 * General configuration
 * ==========================================================================
 */

/*
 * Linux uses a 32 ms maximum halt/start handshake.
 *
 * Controller reset itself gets a much longer timeout.  The xHCI spec allows
 * controller reset to be substantially slower than an ordinary Run/Stop
 * transition, and Linux uses separate short and long reset windows.
 */
const CONTROLLER_HALT_TIMEOUT_US: u64 = 200_000;
const CONTROLLER_START_TIMEOUT_US: u64 = 1_000_000;

const XHCI_RESET_LONG_TIMEOUT_US: u64 = 1_000_000;

/*
 * Existing Intel controllers may require about 1 ms after HCRST before the
 * next controller-register access.  Rusty applies this conservative delay to
 * all xHCI controllers rather than depending on a PCI vendor quirk table.
 */
const XHCI_POST_RESET_ACCESS_DELAY_US: u64 = 1_000;

/* Stellux gives the controller an additional 50 ms after HCRST/CNR clears
 * before programming the operational/runtime registers. */
const XHCI_POST_RESET_SETTLE_DELAY_US: u64 = 50_000;

/*
 * A temporary all-ones read immediately after HCRST is tolerated.  A sustained
 * all-ones read is treated as an inaccessible/removed controller.
 */
const XHCI_RESET_ALL_ONES_GRACE_US: u64 = 10_000;

/*
 * Software operation deadlines.  These are NOT the duration for which PORTSC.PR
 * or PORTSC.WR is held high; the controller owns those reset timings.
 */
/* Linux HUB_RESET_TIMEOUT: 800 ms. */
const USB2_PORT_RESET_TIMEOUT_US: u64 = 100_000;
const USB3_WARM_RESET_TIMEOUT_US: u64 = 800_000;

/*
 * USB Legacy Support handoff is intentionally bounded.  Linux historically
 * a 1 second BIOS->OS handoff timeout.
 */
const XHCI_BIOS_HANDOFF_TIMEOUT_US: u64 = 1_000_000;
const XHCI_POLL_INTERVAL_US: u64 = 10;

/* Conservative real-hardware settling delays. */
const XHCI_RUN_GRACE_PERIOD_US: u64 = 500_000;
const USB2_ROOT_RESET_DELAY_US: u64 = 1_000;
const USB3_WARM_RESET_DELAY_US: u64 = 1_000;
const USB_RESET_RECOVERY_DELAY_US: u64 = 3_000;
const USB_RESET_POST_CLEAR_DELAY_US: u64 = 3_000;
const USB_PORT_POWER_STABILIZE_DELAY_US: u64 = 20_000;

const COMMAND_RING_TRBS: usize = 256;

const EVENT_RING_TRBS: usize = 256;

const DCBAA_ENTRY_SIZE: usize = core::mem::size_of::<u64>();

const ERST_ENTRY_SIZE: usize = 16;

const XHCI_TRB_SIZE: usize = core::mem::size_of::<Trb>();

const MAX_EVENT_RING_SEGMENT_TRBS: usize = u16::MAX as usize;

const ERST_ALIGNMENT: usize = 64;

const DCBAA_ALIGNMENT: usize = 64;

const RING_ALIGNMENT: usize = 64;

/* xHCI MaxSlots is an 8-bit field; DCBAA has entries 0..MaxSlots. */
const MAX_XHCI_SLOTS: usize = 255;

/*
 * xHCI defines at most 127 root ports.
 */
const MAX_XHCI_PORTS: usize = 127;

/*
 * Supported Protocol Extended Capability.
 *
 * HCCPARAMS1[31:16] is the Extended Capability Pointer, measured in DWORDs
 * from the capability-register base. Supported Protocol capability ID = 2.
 */
const XHCI_EXT_CAPS_LEGACY: u8 = 1;
const XHCI_EXT_CAPS_PROTOCOL: u8 = 2;

const XHCI_EXT_CAP_ID_MASK: u32 = 0xFF;
const XHCI_EXT_CAP_NEXT_SHIFT: u32 = 8;
const XHCI_EXT_CAP_NEXT_MASK: u32 = 0xFF;
const XHCI_EXT_PORT_MINOR_SHIFT: u32 = 16;
const XHCI_EXT_PORT_MAJOR_SHIFT: u32 = 24;
const XHCI_EXT_PORT_OFFSET_MASK: u32 = 0xFF;
const XHCI_EXT_PORT_COUNT_MASK: u32 = 0xFF << 8;

/*
 * HCCPARAMS1 is at capability-register offset 0x10.
 */
const HCCPARAMS1_OFFSET: usize = 0x10;

/* HCCPARAMS1.PPC: software may control root-port power through PORTSC.PP. */
const HCCPARAMS1_PPC: u32 = 1 << 3;

/*
 * USB Legacy Support Capability.
 *
 * DWORD 0:
 *   bit 16 = BIOS Owned Semaphore
 *   bit 24 = OS Owned Semaphore
 *
 * DWORD 1:
 *   legacy SMI enable/status fields.
 *
 * These values match the Linux xHCI legacy-support definitions.
 */
const LEGACY_SUPPORT_OFFSET: usize = 0x00;
const LEGACY_CONTROL_OFFSET: usize = 0x04;

const LEGACY_BIOS_OWNED: u32 = 1 << 16;
const LEGACY_OS_OWNED: u32 = 1 << 24;

const LEGACY_DISABLE_SMI: u32 =
    (0x7 << 1) | (0xFF << 5) | (0x7 << 17);

const LEGACY_SMI_EVENTS: u32 = 0x7 << 29;

/*
 * Generic PORTSC neutral-write preserve set.
 *
 * This follows Linux's neutral PORTSC transformation: preserve RO status
 * fields plus the RWS state fields, including PLS.  Link-state writes still
 * have their own dedicated helper so LWS is never replayed accidentally.
 */
/*
 * Linux-style PORTSC neutral-write preservation.
 *
 * xhci_port_state_to_neutral() preserves the read-only status fields and the
 * normal read/write port state fields.  In particular, PLS MUST be preserved
 * when constructing a PORTSC write such as PORT_RESET or warm reset.
 *
 * The previous Rusty mask omitted PLS, which made a reset write encode U0 in
 * the PORTSC word regardless of the link state the controller was actually
 * reporting.  That is not the Linux neutral-write semantics and can disturb
 * real hardware during reset/recovery.
 */
const PORTSC_OCA: u32 = 1 << 3;
const PORTSC_DEVICE_REMOVABLE: u32 = 1 << 30;

const PORTSC_RO_PRESERVE: u32 =
    PORTSC_CCS | PORTSC_OCA | PORTSC_SPEED_MASK | PORTSC_DEVICE_REMOVABLE;

const PORTSC_STATE_PRESERVE: u32 =
    PORTSC_PLS_MASK | PORTSC_PP | (0x3 << 14) | (0x7 << 25);

const PORTSC_SAFE_PRESERVE: u32 = PORTSC_RO_PRESERVE | PORTSC_STATE_PRESERVE;

/* PORT Link State Strobe. */
const PORTSC_LWS: u32 = 1 << 16;

/*
 * USB3 Cold Attach Status.
 */
const PORTSC_CAS: u32 = 1 << 24;

/*
 * USB3 warm port reset.
 */
const PORTSC_WR: u32 = 1 << 31;

/*
 * ==========================================================================
 * Scratchpad fields in HCSPARAMS2
 * ==========================================================================
 */

/*
 * HCSPARAMS2[25:21] = Max Scratchpad Buffers (Hi), the five
 * most-significant bits of the 10-bit scratchpad-buffer count.
 * HCSPARAMS2[31:27] = Max Scratchpad Buffers (Lo), the five
 * least-significant bits.
 */
const SCRATCHPAD_HI_SHIFT: u32 = 21;
const SCRATCHPAD_LO_SHIFT: u32 = 27;
const SCRATCHPAD_MASK: u32 = 0x1F;

/*
 * ==========================================================================
 * Root-port protocol mapping
 * ==========================================================================
 *
 * Linux keeps a physical port_array[] and derives separate USB2 and USB3
 * root-hub port arrays from the Supported Protocol Extended Capabilities.
 */
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UsbPortProtocol {
    Unknown,
    Usb2,
    Usb3,
    Duplicate,
}

impl UsbPortProtocol {
    #[inline(always)]
    fn label(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::Usb2 => "USB2",
            Self::Usb3 => "USB3",
            Self::Duplicate => "DUPLICATE",
        }
    }
}

/*
 * Result of one non-blocking root-port operation step.
 *
 * A port-status event handler must never spin waiting for hardware.  The
 * state machine is therefore advanced one step at a time from the normal
 * polling path, or once from an event handler when that handler observes the
 * completion indication.
 */
#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingOperationResult {
    NotPending,
    Pending,
    Complete,
    Cancelled,
    TimedOut,
}

#[derive(Clone, Copy)]
struct CommandCompletion {
    command_trb: u64,
    completion_code: u8,
    slot_id: u8,
}

/*
 * ==========================================================================
 * Driver
 * ==========================================================================
 */

pub struct XhciDriver {
    /*
     * Public because usb/init.rs directly accesses regs.
     */
    pub regs: XhciRegs,

    /*
     * Capability-register virtual base.
     */
    mmio_base: usize,

    /*
     * Operational-register virtual base.
     */
    operational_base: usize,

    /*
     * Software pointer to the xHCI doorbell array.
     *
     * Linux stores this as xhci->dba.
     *
     * This is NOT a hardware register.
     */
    pub doorbell_base: usize,

    pub max_slots: usize,
    pub max_ports: usize,
    pub max_interrupters: usize,
    pub context_size: usize,

    /*
     * Linux physical port_array[] equivalent.
     *
     * Index = physical xHCI port number - 1.
     */
    pub port_protocol: [UsbPortProtocol; MAX_XHCI_PORTS],
    pub port_revision: [u8; MAX_XHCI_PORTS],

    /*
     * Linux usb2_ports[] / usb3_ports[] equivalents.
     *
     * Values are ONE-BASED physical xHCI PORTSC numbers.
     */
    pub usb2_ports: [u8; MAX_XHCI_PORTS],
    pub usb2_port_count: usize,
    pub usb3_ports: [u8; MAX_XHCI_PORTS],
    pub usb3_port_count: usize,

    /*
     * Non-blocking root-port operation state.
     *
     * These are absolute TSC-backed microsecond deadlines.  The normal poll
     * path checks them; the Port Status Change event handler never spins.
     */
    port_reset_pending: [bool; MAX_XHCI_PORTS],
    port_reset_deadline_us: [u64; MAX_XHCI_PORTS],
    usb3_warm_reset_pending: [bool; MAX_XHCI_PORTS],
    usb3_warm_reset_deadline_us: [u64; MAX_XHCI_PORTS],

    /*
     * Linux reserves one command-ring TRB.
     */
    pub cmd_ring_reserved_trbs: usize,

    /*
     * Command ring.
     *
     * Contains its Link TRB.
     */
    pub command_ring: Ring,

    /*
     * Primary event ring.
     *
     * Does NOT contain a Link TRB.
     */
    pub event_ring: EventRing,

    /*
     * Device Context Base Address Array.
     */
    pub dcbaa_phys: u64,
    pub dcbaa_virt: usize,

    /*
     * Per-slot device-context pointers. Entry 0 is reserved for scratchpads.
     */
    pub device_context_phys: Vec<u64>,
    pub device_context_virt: Vec<usize>,

    /*
     * Per-slot Input Context pointers. These are allocated now so the next
     * enumeration stage can build Slot/EP0 contexts without changing the
     * controller-memory ownership model again.
     */
    pub input_context_phys: Vec<u64>,
    pub input_context_virt: Vec<usize>,

    /* Most recently completed Enable Slot result. */
    last_command_completion: Option<CommandCompletion>,

    /* Last root port that completed a reset and is ready for enumeration. */
    ready_port: usize,

    /*
     * Event Ring Segment Table.
     */
    pub erst_phys: u64,
    pub erst_virt: usize,

    /*
     * Current software event dequeue address.
     */
    pub event_dequeue: u64,

    /*
     * Scratchpad information.
     */
    pub scratchpad_count: usize,
    pub scratchpad_page_size: usize,
    pub scratchpad_array_phys: u64,
    pub scratchpad_array_virt: usize,

    /*
     * xhci_init() completed.
     */
    initialized: bool,

    /*
     * Controller has actually been started.
     */
    running: bool,
}

impl XhciDriver {
    /*
     * ======================================================================
     * Constructor
     * ======================================================================
     *
     * Allocate all persistent xHCI DMA objects here.
     *
     * This is intentionally separate from xhci_init().
     * ======================================================================
     */

    pub unsafe fn new(mmio_base: usize) -> Self {
        if mmio_base == 0 {
            panic!("xHCI: MMIO base is null",);
        }

        /*
         * Verify CAPLENGTH and calculate operational-register base.
         */
        let caplength = read_volatile(
            mmio_base
                .checked_add(CAPLENGTH_OFFSET)
                .expect("xHCI: CAPLENGTH address overflow") as *const u8,
        ) as usize;

        let operational_base = mmio_base
            .checked_add(caplength)
            .expect("xHCI: operational base overflow");

        let regs = XhciRegs::new(mmio_base);

        let max_slots = regs.max_slots();

        let max_ports = regs.max_ports();

        let max_interrupters = regs.max_interrupters();

        let context_size = regs.context_size();

        if max_slots == 0 {
            panic!("xHCI: controller reports zero device slots",);
        }

        if max_slots > MAX_XHCI_SLOTS {
            panic!("xHCI: controller reports more than 255 device slots",);
        }

        if max_ports == 0 {
            panic!("xHCI: controller reports zero ports",);
        }

        if max_ports > MAX_XHCI_PORTS {
            panic!("xHCI: controller reports more than 127 ports",);
        }

        if max_interrupters == 0 {
            panic!("xHCI: controller reports zero interrupters",);
        }

        if context_size != 32 && context_size != 64 {
            panic!("xHCI: invalid context size",);
        }

        /*
         * Read scratchpad count.
         */
        let scratchpad_count = Self::scratchpad_count_from_hcsparams2(regs.hcsparams2());

        /*
         * ==================================================================
         * DCBAA
         * ==================================================================
         */

        let dcbaa_entries = max_slots
            .checked_add(1)
            .expect("xHCI: DCBAA entry count overflow");

        let dcbaa_size = dcbaa_entries
            .checked_mul(DCBAA_ENTRY_SIZE)
            .expect("xHCI: DCBAA size overflow");

        /*
         * The DCBAA must be physically contiguous, at least 64-byte aligned,
         * and must not cross a 4 KiB page boundary.  Aligning to the next
         * power-of-two size (with a 64-byte minimum) gives us that property
         * for the architecturally bounded DCBAA size.
         */
        let dcbaa_alignment = dcbaa_size
            .max(DCBAA_ALIGNMENT)
            .next_power_of_two();

        let dcbaa_phys = memory::allocate_dma_region(dcbaa_size, dcbaa_alignment, None)
            .expect("xHCI: failed to allocate DCBAA");

        let dcbaa_virt = memory::physical_to_virtual(dcbaa_phys) as usize;

        if dcbaa_virt == 0 {
            panic!("xHCI: invalid DCBAA virtual address",);
        }

        if (dcbaa_phys & (DCBAA_ALIGNMENT as u64 - 1)) != 0 {
            panic!("xHCI: DCBAA is not 64-byte aligned",);
        }

        let dcbaa_page_offset = (dcbaa_phys as usize) & (memory::PAGE_SIZE as usize - 1);

        if dcbaa_page_offset
            .checked_add(dcbaa_size)
            .map_or(true, |end| end > memory::PAGE_SIZE as usize)
        {
            panic!("xHCI: DCBAA crosses a 4 KiB page boundary",);
        }

        write_bytes(dcbaa_virt as *mut u8, 0, dcbaa_size);
        dma_sync_for_device(dcbaa_virt as *const u8, dcbaa_size);

        /*
         * ==================================================================
         * Command ring
         * ==================================================================
         */

        let command_ring = Ring::new(COMMAND_RING_TRBS);

        /*
         * ==================================================================
         * Primary event ring
         * ==================================================================
         */

        let event_ring = EventRing::new(EVENT_RING_TRBS);

        /*
         * ==================================================================
         * ERST
         * ==================================================================
         */

        let erst_phys = memory::allocate_dma_region(ERST_ENTRY_SIZE, ERST_ALIGNMENT, None)
            .expect("xHCI: failed to allocate ERST");

        let erst_virt = memory::physical_to_virtual(erst_phys) as usize;

        if erst_virt == 0 {
            panic!("xHCI: invalid ERST virtual address",);
        }

        if (erst_phys & (ERST_ALIGNMENT as u64 - 1)) != 0 {
            panic!("xHCI: ERST is not 64-byte aligned",);
        }

        write_bytes(erst_virt as *mut u8, 0, ERST_ENTRY_SIZE);

        /*
         * Keep the per-slot context bookkeeping on the heap.  A controller may
         * expose up to 255 device slots; storing four 256-entry tables directly
         * inside XhciDriver makes the early-kernel stack unnecessarily large.
         */
        let context_table_len = max_slots
            .checked_add(1)
            .expect("xHCI: context table length overflow");

        let mut device_context_phys = Vec::<u64>::with_capacity(context_table_len);
        device_context_phys.resize(context_table_len, 0);

        let mut device_context_virt = Vec::<usize>::with_capacity(context_table_len);
        device_context_virt.resize(context_table_len, 0);

        let mut input_context_phys = Vec::<u64>::with_capacity(context_table_len);
        input_context_phys.resize(context_table_len, 0);

        let mut input_context_virt = Vec::<usize>::with_capacity(context_table_len);
        input_context_virt.resize(context_table_len, 0);

        let mut driver = Self {
            regs,

            mmio_base,
            operational_base,

            doorbell_base: 0,

            max_slots,
            max_ports,
            max_interrupters,
            context_size,

            port_protocol: [UsbPortProtocol::Unknown; MAX_XHCI_PORTS],
            port_revision: [0; MAX_XHCI_PORTS],
            usb2_ports: [0; MAX_XHCI_PORTS],
            usb2_port_count: 0,
            usb3_ports: [0; MAX_XHCI_PORTS],
            usb3_port_count: 0,

            port_reset_pending: [false; MAX_XHCI_PORTS],
            port_reset_deadline_us: [0; MAX_XHCI_PORTS],
            usb3_warm_reset_pending: [false; MAX_XHCI_PORTS],
            usb3_warm_reset_deadline_us: [0; MAX_XHCI_PORTS],

            cmd_ring_reserved_trbs: 0,

            command_ring,
            event_ring,

            dcbaa_phys,
            dcbaa_virt,

            device_context_phys,
            device_context_virt,
            input_context_phys,
            input_context_virt,
            last_command_completion: None,
            ready_port: 0,

            erst_phys,
            erst_virt,

            event_dequeue: 0,

            scratchpad_count,
            scratchpad_page_size: 0,
            scratchpad_array_phys: 0,
            scratchpad_array_virt: 0,

            initialized: false,
            running: false,
        };

        /*
         * Take ownership away from firmware before Rusty writes any controller
         * operational/runtime registers.
         */
        driver.handoff_legacy_support();

        /*
         * DEBUG CHECKPOINT #1: dump raw physical PORTSC values immediately
         * after BIOS -> OS ownership handoff, before Supported Protocol parsing.
         * The protocol labels are not populated yet, but every physical port
         * number and its PORTSC value is visible even if capability parsing later
         * fails.
         */
        crate::serial::write_str(
            "xHCI: RAW PORTSC immediately after BIOS -> OS handoff:\n",
        );
        driver.dump_ports();
        crate::serial::write_str(
            "xHCI: raw post-handoff PORTSC dump complete\n",
        );

        /*
         * Build the Linux-style physical USB2/USB3 root-port split before the
         * controller starts.
         */
        driver.setup_port_arrays();

        /*
         * DEBUG CHECKPOINT #2: now print the same post-handoff state again with
         * the Supported Protocol USB2/USB3 labels attached.  setup_port_arrays()
         * only reads capability registers, so these PORTSC values are still the
         * pre-HCRST firmware state.
         */
        crate::serial::write_str(
            "xHCI: MAPPED PORTSC immediately after BIOS -> OS handoff:\n",
        );
        driver.dump_ports();
        crate::serial::write_str(
            "xHCI: mapped post-handoff PORTSC dump complete\n",
        );

        /*
         * Validate DMA memory.
         */

        driver.validate_dma_range(driver.dcbaa_phys, dcbaa_size, "DCBAA");

        driver.validate_dma_range(
            driver.command_ring.phys_addr,
            driver.command_ring.size * XHCI_TRB_SIZE,
            "command ring",
        );

        driver.validate_dma_range(
            driver.event_ring.phys_addr,
            driver.event_ring.trbs * XHCI_TRB_SIZE,
            "event ring",
        );

        driver.validate_dma_range(driver.erst_phys, ERST_ENTRY_SIZE, "ERST");

        /*
         * Logging.
         */

        crate::serial::write_str("xHCI: controller discovered\n");

        crate::serial::write_str("xHCI: HCI version = 0x");

        crate::serial::write_hex(driver.regs.hci_version() as u64);

        crate::serial::write_str("\n");

        crate::serial::write_str("xHCI: max slots = ");

        crate::serial::write_hex(driver.max_slots as u64);

        crate::serial::write_str("\n");

        crate::serial::write_str("xHCI: max ports = ");

        crate::serial::write_hex(driver.max_ports as u64);

        crate::serial::write_str("\n");

        crate::serial::write_str("xHCI: max interrupters = ");

        crate::serial::write_hex(driver.max_interrupters as u64);

        crate::serial::write_str("\n");

        crate::serial::write_str("xHCI: context size = ");

        crate::serial::write_hex(driver.context_size as u64);

        crate::serial::write_str("\n");

        crate::serial::write_str("xHCI: scratchpad buffers = ");

        crate::serial::write_hex(driver.scratchpad_count as u64);

        crate::serial::write_str("\n");

        crate::serial::write_str("xHCI: 64-bit DMA = ");

        if driver.regs.supports_64bit() {
            crate::serial::write_str("yes\n");
        } else {
            crate::serial::write_str("no\n");
        }

        driver
    }

    /*
     * ======================================================================
     * Scratchpad count
     * ======================================================================
     */

    fn scratchpad_count_from_hcsparams2(value: u32) -> usize {
        let hi = ((value >> SCRATCHPAD_HI_SHIFT) & SCRATCHPAD_MASK) as usize;

        let lo = ((value >> SCRATCHPAD_LO_SHIFT) & SCRATCHPAD_MASK) as usize;

        (hi << 5) | lo
    }

    /*
     * ======================================================================
     * USB Legacy Support / BIOS -> OS handoff
     * ======================================================================
     *
     * The USB Legacy Support Extended Capability is optional. When present,
     * firmware can advertise that BIOS owns the xHC through BIOS Owned.
     * Rusty requests ownership with OS Owned, waits for BIOS Owned to clear,
     * and then disables legacy SMI sources.
     *
     * The Next Capability Pointer is relative to the current capability and
     * is measured in DWORDs, exactly like xECP in HCCPARAMS1.
     */
    unsafe fn handoff_legacy_support(&self) {
        let hccparams1_address = self
            .mmio_base
            .checked_add(HCCPARAMS1_OFFSET)
            .expect("xHCI: HCCPARAMS1 address overflow");

        let hccparams1 = read_volatile(hccparams1_address as *const u32);

        if hccparams1 == u32::MAX {
            panic!("xHCI: HCCPARAMS1 reads as all ones during BIOS handoff");
        }

        let mut offset = ((hccparams1 >> 16) & 0xFFFF) as usize;

        if offset == 0 {
            crate::serial::write_str(
                "xHCI: no USB Legacy Support capability present\n",
            );
            return;
        }

        let mut seen = 0usize;

        while offset != 0 {
            seen += 1;

            if seen > 256 {
                panic!(
                    "xHCI: Extended Capability list appears cyclic during BIOS handoff",
                );
            }

            let capability_address = self
                .mmio_base
                .checked_add(
                    offset
                        .checked_mul(4)
                        .expect("xHCI: legacy capability offset overflow"),
                )
                .expect("xHCI: legacy capability address overflow");

            let header = read_volatile(capability_address as *const u32);

            if header == u32::MAX {
                panic!("xHCI: Extended Capability reads as all ones during BIOS handoff");
            }

            let capability_id = (header & XHCI_EXT_CAP_ID_MASK) as u8;
            let next = ((header >> XHCI_EXT_CAP_NEXT_SHIFT)
                & XHCI_EXT_CAP_NEXT_MASK) as usize;

            if capability_id == XHCI_EXT_CAPS_LEGACY {
                crate::serial::write_str(
                    "xHCI: USB Legacy Support capability found at DWORD offset 0x",
                );
                crate::serial::write_hex(offset as u64);
                crate::serial::write_str("\n");

                let legacy_address = capability_address
                    .checked_add(LEGACY_SUPPORT_OFFSET)
                    .expect("xHCI: legacy support address overflow");

                let mut legacy = read_volatile(legacy_address as *const u32);

                if legacy == u32::MAX {
                    panic!("xHCI: USB Legacy Support reads as all ones");
                }

                if (legacy & LEGACY_BIOS_OWNED) != 0 {
                    crate::serial::write_str(
                        "xHCI: BIOS owns xHCI; requesting OS ownership\n",
                    );

                    write_volatile(
                        legacy_address as *mut u32,
                        legacy | LEGACY_OS_OWNED,
                    );

                    let _ = read_volatile(legacy_address as *const u32);

                    let start_us = crate::delay::now_us();
                    let deadline_us =
                        start_us.saturating_add(XHCI_BIOS_HANDOFF_TIMEOUT_US);

                    loop {
                        legacy = read_volatile(legacy_address as *const u32);

                        if legacy == u32::MAX {
                            panic!(
                                "xHCI: controller became inaccessible during BIOS handoff",
                            );
                        }

                        if (legacy & LEGACY_BIOS_OWNED) == 0 {
                            break;
                        }

                        if crate::delay::now_us() >= deadline_us {
                            crate::serial::write_str(
                                "xHCI: BIOS ownership did not clear; taking OS ownership anyway\n",
                            );

                            /*
                             * Match Linux's fallback for firmware that does not
                             * release the BIOS-owned semaphore.
                             */
                            let current =
                                read_volatile(legacy_address as *const u32);

                            write_volatile(
                                legacy_address as *mut u32,
                                current & !LEGACY_BIOS_OWNED,
                            );

                            let _ = read_volatile(legacy_address as *const u32);
                            break;
                        }

                        crate::delay::delay_us(XHCI_POLL_INTERVAL_US);
                    }
                }

                /*
                 * Ensure OS Owned is set even if BIOS Owned was already clear.
                 */
                legacy = read_volatile(legacy_address as *const u32);

                if legacy == u32::MAX {
                    panic!("xHCI: USB Legacy Support became inaccessible");
                }

                if (legacy & LEGACY_OS_OWNED) == 0 {
                    write_volatile(
                        legacy_address as *mut u32,
                        legacy | LEGACY_OS_OWNED,
                    );
                    let _ = read_volatile(legacy_address as *const u32);
                }

                let control_address = capability_address
                    .checked_add(LEGACY_CONTROL_OFFSET)
                    .expect("xHCI: legacy control address overflow");

                let control = read_volatile(control_address as *const u32);

                if control != u32::MAX {
                    /*
                     * Match Linux's legacy SMI shutdown sequence:
                     * preserve the defined control fields, turn off all SMI
                     * enables, and write 1 to the RW1C SMI event bits.
                     */
                    let new_control =
                        (control & LEGACY_DISABLE_SMI) | LEGACY_SMI_EVENTS;

                    write_volatile(control_address as *mut u32, new_control);

                    let _ = read_volatile(control_address as *const u32);

                    crate::serial::write_str(
                        "xHCI: legacy SMI sources disabled, control=0x",
                    );
                    crate::serial::write_hex(new_control as u64);
                    crate::serial::write_str("\n");
                }

                crate::serial::write_str(
                    "xHCI: BIOS -> OS ownership handoff complete\n",
                );
                return;
            }

            if next == 0 {
                break;
            }

            offset = offset
                .checked_add(next)
                .expect("xHCI: malformed Extended Capability chain");
        }

        crate::serial::write_str(
            "xHCI: no USB Legacy Support capability present\n",
        );
    }

    /*
     * ======================================================================
     * Supported Protocol Extended Capabilities / root-hub split
     * ======================================================================
     *
     * This follows Linux xhci_setup_port_arrays() / xhci_add_in_port():
     *
     *   1. Read HCCPARAMS1[31:16] for the Extended Capability List.
     *   2. Walk the linked capability list.
     *   3. Process Supported Protocol capabilities (ID 2).
     *   4. Read the protocol revision from DWORD 0.
     *   5. Read compatible port offset/count from DWORD 2.
     *   6. Build a physical port_array[] equivalent.
     *   7. Derive separate USB2 and USB3 root-hub port lists.
     *
     * The PORTSC SPEED bits are not used for this mapping: for a disconnected
     * port those bits cannot reliably tell us whether the port is USB2 or USB3.
     */
    unsafe fn setup_port_arrays(&mut self) {
        crate::serial::write_str("xHCI: scanning Supported Protocol capabilities...\n");

        let hccparams1_address = self
            .mmio_base
            .checked_add(HCCPARAMS1_OFFSET)
            .expect("xHCI: HCCPARAMS1 address overflow");

        let hccparams1 = read_volatile(hccparams1_address as *const u32);

        if hccparams1 == u32::MAX {
            panic!("xHCI: HCCPARAMS1 reads as all ones",);
        }

        /*
         * xECP is a DWORD offset from the capability-register base.
         */
        let mut offset = ((hccparams1 >> 16) & 0xFFFF) as usize;

        if offset == 0 {
            panic!("xHCI: no Extended Capability list; cannot build root hubs",);
        }

        let mut capabilities_seen = 0usize;
        let mut protocol_capabilities = 0usize;

        while offset != 0 {
            capabilities_seen += 1;

            /*
             * Defensive guard against a malformed/cyclic capability list.
             */
            if capabilities_seen > 256 {
                panic!("xHCI: Extended Capability list appears cyclic/corrupt",);
            }

            let capability_address = self
                .mmio_base
                .checked_add(
                    offset
                        .checked_mul(4)
                        .expect("xHCI: Extended Capability offset overflow"),
                )
                .expect("xHCI: Extended Capability address overflow");

            let dword0 = read_volatile(capability_address as *const u32);

            if dword0 == u32::MAX {
                panic!("xHCI: Extended Capability reads as all ones",);
            }

            let capability_id = (dword0 & XHCI_EXT_CAP_ID_MASK) as u8;
            let next = ((dword0 >> XHCI_EXT_CAP_NEXT_SHIFT) & XHCI_EXT_CAP_NEXT_MASK) as usize;

            if capability_id == XHCI_EXT_CAPS_PROTOCOL {
                protocol_capabilities += 1;

                /*
                 * Supported Protocol DWORD 0:
                 *
                 * [23:16] = minor revision
                 * [31:24] = major revision
                 */
                let minor_revision = ((dword0 >> XHCI_EXT_PORT_MINOR_SHIFT) & 0xFF) as u8;
                let major_revision = ((dword0 >> XHCI_EXT_PORT_MAJOR_SHIFT) & 0xFF) as u8;

                /*
                 * Supported Protocol DWORD 2:
                 *
                 * [7:0]  = Compatible Port Offset
                 * [15:8] = Compatible Port Count
                 */
                let dword2_address = capability_address
                    .checked_add(8)
                    .expect("xHCI: Supported Protocol DWORD 2 address overflow");

                let dword2 = read_volatile(dword2_address as *const u32);

                let port_offset = (dword2 & XHCI_EXT_PORT_OFFSET_MASK) as usize;
                let port_count = ((dword2 & XHCI_EXT_PORT_COUNT_MASK) >> 8) as usize;

                crate::serial::write_str("xHCI: Supported Protocol rev=0x");
                crate::serial::write_hex(
                    (((major_revision as u16) << 8) | minor_revision as u16) as u64,
                );
                crate::serial::write_str(" offset=");
                crate::serial::write_hex(port_offset as u64);
                crate::serial::write_str(" count=");
                crate::serial::write_hex(port_count as u64);
                crate::serial::write_str("\n");

                if port_offset == 0 || port_count == 0 {
                    crate::serial::write_str(
                        "xHCI: ignoring invalid Supported Protocol port range\n",
                    );
                } else {
                    let last_port = port_offset
                        .checked_add(port_count - 1)
                        .expect("xHCI: Supported Protocol port range overflow");

                    if last_port > self.max_ports {
                        crate::serial::write_str(
                            "xHCI: ignoring Supported Protocol range outside MaxPorts\n",
                        );
                    } else {
                        self.add_supported_protocol_range(major_revision, port_offset, port_count);
                    }
                }
            }

            /*
             * Next Capability Pointer is RELATIVE to this capability.
             * It is measured in DWORDs, exactly like xECP itself.
             */
            /*
             * The Next Capability Pointer is RELATIVE to the current
             * capability and is expressed in DWORDs.  `offset` is also stored
             * in DWORD units, so add the pointer directly.
             */
            if next == 0 {
                break;
            }

            offset = offset
                .checked_add(next)
                .expect("xHCI: malformed Extended Capability chain");
        }

        if protocol_capabilities == 0 {
            panic!("xHCI: no Supported Protocol capabilities found",);
        }

        /*
         * Derive separate USB2 and USB3 root-hub arrays.
         *
         * The array values are physical one-based xHCI PORTSC numbers.
         */
        self.usb2_port_count = 0;
        self.usb3_port_count = 0;

        for index in 0..self.max_ports {
            let physical_port = index + 1;

            match self.port_protocol[index] {
                UsbPortProtocol::Usb2 => {
                    self.usb2_ports[self.usb2_port_count] = physical_port as u8;
                    self.usb2_port_count += 1;
                }

                UsbPortProtocol::Usb3 => {
                    self.usb3_ports[self.usb3_port_count] = physical_port as u8;
                    self.usb3_port_count += 1;
                }

                UsbPortProtocol::Unknown | UsbPortProtocol::Duplicate => {}
            }
        }

        if self.usb2_port_count == 0 && self.usb3_port_count == 0 {
            panic!("xHCI: no usable USB2/USB3 root ports found",);
        }

        crate::serial::write_str("xHCI: discovered ");
        crate::serial::write_hex(self.usb2_port_count as u64);
        crate::serial::write_str(" USB2 ports and ");
        crate::serial::write_hex(self.usb3_port_count as u64);
        crate::serial::write_str(" USB3 ports\n");

        for index in 0..self.max_ports {
            let protocol = self.port_protocol[index];

            if protocol == UsbPortProtocol::Unknown {
                continue;
            }

            crate::serial::write_str("xHCI: physical PORT ");
            crate::serial::write_hex((index + 1) as u64);
            crate::serial::write_str(" -> ");
            crate::serial::write_str(protocol.label());
            crate::serial::write_str(" major=0x");
            crate::serial::write_hex(self.port_revision[index] as u64);
            crate::serial::write_str("\n");
        }
    }

    /*
     * Add one Supported Protocol range to the physical port_array[]
     * equivalent.
     *
     * Linux treats revision 0x03 as USB3 and revisions <= 0x02 as USB2.
     * Conflicting assignments are marked DUPLICATE.
     */
    fn add_supported_protocol_range(
        &mut self,
        major_revision: u8,
        port_offset: usize,
        port_count: usize,
    ) {
        let protocol = if major_revision == 0x03 {
            UsbPortProtocol::Usb3
        } else if major_revision <= 0x02 {
            UsbPortProtocol::Usb2
        } else {
            crate::serial::write_str("xHCI: ignoring unknown USB protocol major=0x");
            crate::serial::write_hex(major_revision as u64);
            crate::serial::write_str("\n");
            return;
        };

        let last_port = port_offset + port_count - 1;

        for physical_port in port_offset..=last_port {
            let index = physical_port - 1;

            if self.port_revision[index] != 0 {
                /*
                 * Identical duplicate revisions are harmless and are ignored.
                 * Conflicting revisions are treated as duplicate mappings just
                 * like Linux's DUPLICATE_ENTRY handling.
                 */
                if self.port_revision[index] == major_revision {
                    continue;
                }

                if self.port_protocol[index] == UsbPortProtocol::Duplicate {
                    continue;
                }

                crate::serial::write_str("xHCI: conflicting protocol mapping PORT ");
                crate::serial::write_hex(physical_port as u64);
                crate::serial::write_str(" old=0x");
                crate::serial::write_hex(self.port_revision[index] as u64);
                crate::serial::write_str(" new=0x");
                crate::serial::write_hex(major_revision as u64);
                crate::serial::write_str("\n");

                self.port_protocol[index] = UsbPortProtocol::Duplicate;
                continue;
            }

            self.port_revision[index] = major_revision;
            self.port_protocol[index] = protocol;
        }
    }

    /*
     * Conservative generic PORTSC neutral value.
     *
     * Keep only the RO status fields and ordinary RW state that xHCI allows
     * software to replay. PLS is preserved because it is part of the normal
     * port-state read/modify/write value; LWS is never replayed unless a
     * dedicated link-state operation explicitly adds it.
     */
    #[inline(always)]
    fn portsc_to_neutral(portsc: u32) -> u32 {
        portsc & PORTSC_SAFE_PRESERVE
    }

    /*
     * Dedicated link-state write boundary.
     *
     * This is the only helper in this driver that intentionally writes PLS
     * and LWS together.
     */
    #[inline(always)]
    fn portsc_link_state_write(portsc: u32, pls: u8) -> u32 {
        Self::portsc_to_neutral(portsc) | (((pls as u32) << 5) & PORTSC_PLS_MASK) | PORTSC_LWS
    }

    /*
     * Explicit link-state operation for future USB power-management code.
     */
    pub unsafe fn set_port_link_state(&self, port: usize, pls: u8) {
        if port == 0 || port > self.max_ports || pls > 0x0F {
            return;
        }

        let portsc = self.regs.portsc(port);
        let write_value = Self::portsc_link_state_write(portsc, pls);

        self.regs.set_portsc(port, write_value);
        let _ = self.regs.portsc(port);
    }

    /*
     * Acknowledge only the change bits the caller has already interpreted.
     * Any newer change bit appearing between the snapshot and this write is
     * left alone because it is not included in `changes`.
     */
    unsafe fn clear_port_change_bits(&self, port: usize, changes: u32) {
        let changes = changes & PORTSC_CHANGE_MASK;
        if changes == 0 {
            return;
        }

        let portsc = self.regs.portsc(port);
        let write_value = Self::portsc_to_neutral(portsc) | changes;

        self.regs.set_portsc(port, write_value);

        /* Flush posted MMIO write. */
        let _ = self.regs.portsc(port);
    }

    #[inline(always)]
    pub fn port_protocol_for(&self, port: usize) -> UsbPortProtocol {
        if port == 0 || port > self.max_ports {
            return UsbPortProtocol::Unknown;
        }

        self.port_protocol[port - 1]
    }

    /*
     * Root-hub port number -> physical xHCI PORTSC number.
     *
     * This corresponds to Linux usb2_ports[] / usb3_ports[].
     */
    pub fn usb2_root_port(&self, root_port: usize) -> Option<usize> {
        if root_port == 0 || root_port > self.usb2_port_count {
            None
        } else {
            Some(self.usb2_ports[root_port - 1] as usize)
        }
    }

    pub fn usb3_root_port(&self, root_port: usize) -> Option<usize> {
        if root_port == 0 || root_port > self.usb3_port_count {
            None
        } else {
            Some(self.usb3_ports[root_port - 1] as usize)
        }
    }

    /*
     * ======================================================================
     * Supported xHCI page size
     * ======================================================================
     */

    fn supported_page_size(&self) -> usize {
        let mask = self.regs.pagesize();

        if mask == 0 {
            panic!("xHCI: PAGESIZE reports no supported page size",);
        }

        /*
         * xHCI PAGESIZE:
         *
         * bit N => 4 KiB << N
         *
         * Select the smallest supported page size,
         * matching Linux's page-size selection behavior.
         */
        let bit = mask.trailing_zeros();

        let shift = 12u32
            .checked_add(bit)
            .expect("xHCI: invalid page-size shift");

        1usize
            .checked_shl(shift)
            .expect("xHCI: page size does not fit usize")
    }

    /*
     * ======================================================================
     * DMA validation
     * ======================================================================
     */

    fn validate_dma_address(&self, address: u64, name: &str) {
        self.validate_dma_range(address, 1, name);
    }

    fn validate_dma_range(&self, address: u64, size: usize, name: &str) {
        let end = address
            .checked_add(size.saturating_sub(1) as u64)
            .expect("xHCI: DMA range overflow");

        /*
         * Controller without AC64:
         *
         * DMA must remain below 4 GiB.
         */
        if !self.regs.supports_64bit() && end > u32::MAX as u64 {
            crate::serial::write_str("xHCI: DMA range above 4 GiB: ");

            crate::serial::write_str(name);

            crate::serial::write_str(" start=0x");

            crate::serial::write_hex(address);

            crate::serial::write_str(" end=0x");

            crate::serial::write_hex(end);

            crate::serial::write_str("\n");

            panic!("xHCI controller does not support 64-bit DMA",);
        }
    }

    /*
     * ======================================================================
     * Linux xhci_init() equivalent
     * ======================================================================
     *
     * This follows the current Linux xhci_init() sequence:
     *
     *     xhci_enable_max_dev_slots()
     *     xhci_ring_init(cmd_ring)
     *     cmd_ring_reserved_trbs = 1
     *     xhci_set_cmd_ring_deq()
     *     write DCBAAP
     *     xhci_set_doorbell_ptr()
     *     xhci_set_dev_notifications()
     *     xhci_ring_init(primary event ring)
     *     xhci_add_interrupter(0)
     *     isoc_bei_interval
     *     compliance-mode quirk
     *
     * IMPORTANT:
     *
     * This function does NOT:
     *
     *     - halt the controller
     *     - reset the controller
     *     - enable USBCMD.EIE
     *     - enable IMAN.IE
     *     - set USBCMD.RUN
     *
     * Those belong to the run phase.
     * ======================================================================
     */

    pub unsafe fn init(&mut self) {
        crate::serial::write_str("xHCI: xhci_init() started\n");

        /*
         * ==================================================================
         * 1. xhci_enable_max_dev_slots()
         * ==================================================================
         *
         * Linux:
         *
         *     config_reg = readl(...)
         *     config_reg &= ~HCS_SLOTS_MASK
         *     config_reg |= xhci->max_slots
         *     writel(...)
         *
         * Our regs abstraction performs the same operation.
         */

        crate::serial::write_str("xHCI: init: enabling maximum device slots\n");

        self.regs.set_max_slots(self.max_slots);

        /*
         * ==================================================================
         * 2. Reset the software-visible DCBAA contents after HCRST.
         * ==================================================================
         *
         * Stellux performs DCBAA/scratchpad construction after controller
         * reset. Existing DMA allocations can be reused, but the DCBAA must
         * be zeroed before its pointer is handed back to the controller.
         * ==================================================================
         */

        let dcbaa_size = self
            .max_slots
            .checked_add(1)
            .and_then(|entries| entries.checked_mul(DCBAA_ENTRY_SIZE))
            .expect("xHCI: DCBAA size overflow during init");

        write_bytes(self.dcbaa_virt as *mut u8, 0, dcbaa_size);
        dma_sync_for_device(self.dcbaa_virt as *const u8, dcbaa_size);

        if self.scratchpad_count != 0 {
            if self.scratchpad_array_phys == 0 {
                self.init_scratchpads();
            } else {
                /* Re-publish the already allocated scratchpad array. */
                write_volatile(self.dcbaa_virt as *mut u64, self.scratchpad_array_phys);
                dma_sync_for_device(
                    self.dcbaa_virt as *const u8,
                    DCBAA_ENTRY_SIZE,
                );
            }
        }

        /*
         * ==================================================================
         * 3. xhci_ring_init(xhci, xhci->cmd_ring)
         * ==================================================================
         */

        crate::serial::write_str("xHCI: init: initializing command ring\n");

        self.command_ring.reset();

        dma_sync_for_device(
            self.command_ring.buffer.as_ptr() as *const u8,
            self.command_ring.size * XHCI_TRB_SIZE,
        );

        /*
         * ==================================================================
         * 3. Reserve one command TRB.
         * ==================================================================
         */

        self.cmd_ring_reserved_trbs = 1;

        crate::serial::write_str("xHCI: init: command ring reserved TRBs = 1\n");

        /*
         * ==================================================================
         * 4. xhci_set_cmd_ring_deq()
         * ==================================================================
         *
         * Linux derives the dequeue DMA address from the CURRENT dequeue
         * TRB, not merely from the beginning of an arbitrary allocation.
         *
         * Your Ring abstraction exposes phys_addr as its initial physical
         * address, so this assumes Ring::reset() places dequeue at that
         * address.
         */

        let command_ring_dequeue = self.command_ring.phys_addr & CRCR_POINTER_MASK;

        if (command_ring_dequeue & 0x3F) != 0 {
            panic!("xHCI: command ring dequeue pointer is not 64-byte aligned",);
        }

        let mut crcr = self.regs.crcr();

        if crcr == u64::MAX {
            panic!("xHCI: CRCR reads as all ones",);
        }

        /*
         * Preserve every CRCR field except the dequeue pointer and cycle.
         */
        crcr &= !CRCR_POINTER_MASK;

        crcr |= command_ring_dequeue;

        crcr &= !CRCR_RCS;

        if self.command_ring.cycle {
            crcr |= CRCR_RCS;
        }

        crate::serial::write_str("xHCI: init: CRCR = 0x");

        crate::serial::write_hex(crcr);

        crate::serial::write_str("\n");

        self.regs.set_crcr(crcr);

        /*
         * ==================================================================
         * 6. Set DCBAAP.
         * ==================================================================
         */

        crate::serial::write_str("xHCI: init: programming DCBAAP\n");

        if (self.dcbaa_phys & 0x3F) != 0 {
            panic!("xHCI: DCBAA is not 64-byte aligned",);
        }

        self.regs.set_dcbaap(self.dcbaa_phys);

        /*
         * ==================================================================
         * 6. xhci_set_doorbell_ptr()
         * ==================================================================
         *
         * Linux does NOT program a hardware doorbell register here.
         *
         * It calculates:
         *
         *     cap_regs + (DBOFF & DBOFF_MASK)
         *
         * and stores that software pointer.
         * ==================================================================
         */

        crate::serial::write_str("xHCI: init: configuring doorbell pointer\n");

        self.set_doorbell_ptr();

        /*
         * ==================================================================
         * 7. xhci_set_dev_notifications()
         * ==================================================================
         */

        crate::serial::write_str("xHCI: init: configuring device notifications\n");

        self.set_dev_notifications();

        /*
         * ==================================================================
         * 8. xhci_ring_init(primary event ring)
         * ==================================================================
         */

        crate::serial::write_str("xHCI: init: initializing primary event ring\n");

        self.program_event_ring();

        /*
         * ==================================================================
         * 9. xhci_add_interrupter(xhci, 0)
         * ==================================================================
         *
         * program_event_ring() performs the hardware ERST/ERDP setup
         * for interrupter 0.
         *
         * IMPORTANT:
         *
         * Do NOT enable IMAN.IE here.
         * ==================================================================
         */

        crate::serial::write_str("xHCI: init: primary interrupter initialized\n");

        /*
         * ==================================================================
         * 10. isoc_bei_interval
         * ==================================================================
         *
         * Linux stores this as internal driver state:
         *
         *     xhci->interrupters[0]->isoc_bei_interval =
         *         AVOID_BEI_INTERVAL_MAX;
         *
         * There is no corresponding register write here.
         *
         * The current hobby driver has no equivalent isochronous scheduling
         * policy object, so there is deliberately no fake register write.
         * ==================================================================
         */

        /*
         * ==================================================================
         * 11. Compliance Mode Recovery quirk
         * ==================================================================
         *
         * Linux performs this only if its platform-specific quirk check
         * succeeds.
         *
         * There is currently no equivalent quirk framework in this driver,
         * so we intentionally do not enable a fake/unconditional quirk.
         * ==================================================================
         */

        self.initialized = true;
        self.running = false;
        self.ready_port = 0;
        self.last_command_completion = None;
        self.device_context_phys.fill(0);
        self.device_context_virt.fill(0);
        self.input_context_phys.fill(0);
        self.input_context_virt.fill(0);

        crate::serial::write_str("xHCI: xhci_init() finished\n");
    }

    /*
     * ======================================================================
     * Doorbell pointer
     * ======================================================================
     */

    unsafe fn set_doorbell_ptr(&mut self) {
        let address = self
            .mmio_base
            .checked_add(DBOFF_OFFSET)
            .expect("xHCI: DBOFF address overflow");

        let offset = read_volatile(address as *const u32) & DBOFF_MASK;

        let doorbell = self
            .mmio_base
            .checked_add(offset as usize)
            .expect("xHCI: doorbell address overflow");

        self.doorbell_base = doorbell;

        crate::serial::write_str("xHCI: DBOFF = 0x");

        crate::serial::write_hex(offset as u64);

        crate::serial::write_str(" doorbell_base = 0x");

        crate::serial::write_hex(doorbell as u64);

        crate::serial::write_str("\n");
    }

    /*
     * ======================================================================
     * Device notifications
     * ======================================================================
     */

    unsafe fn set_dev_notifications(&self) {
        let address = self
            .operational_base
            .checked_add(DEV_NOTIFICATION_OFFSET)
            .expect("xHCI: DNCTRL address overflow");

        let mut value = read_volatile(address as *const u32);

        if value == u32::MAX {
            panic!("xHCI: DNCTRL reads as all ones",);
        }

        /*
         * Match Linux:
         *
         *     dev_notf &= ~DEV_NOTE_MASK;
         *     dev_notf |= DEV_NOTE_FWAKE;
         */
        value &= !DEV_NOTE_MASK;

        value |= DEV_NOTE_FWAKE;

        write_volatile(address as *mut u32, value);

        /*
         * Flush the posted MMIO write.
         */
        let _ = read_volatile(address as *const u32);

        crate::serial::write_str("xHCI: DNCTRL = 0x");

        crate::serial::write_hex(value as u64);

        crate::serial::write_str("\n");
    }

    /*
     * ======================================================================
     * Event ring programming
     * ======================================================================
     */

    unsafe fn program_event_ring(&mut self) {
        /*
         * Event ring contains NO Link TRB.
         */

        write_bytes(
            self.event_ring.buffer.as_ptr() as *mut u8,
            0,
            self.event_ring.trbs * XHCI_TRB_SIZE,
        );

        /*
         * Ensure the controller sees the freshly initialized event-ring memory
         * before ERST/ERDP are armed.
         */
        dma_sync_for_device(
            self.event_ring.buffer.as_ptr() as *const u8,
            self.event_ring.trbs * XHCI_TRB_SIZE,
        );

        self.event_ring.index = 0;

        self.event_ring.cycle = true;

        /*
         * Software starts consuming at the first event TRB.
         */
        self.event_dequeue = self.event_ring.phys_addr;

        let segment_size = self.event_ring.capacity();

        if segment_size == 0 || segment_size > MAX_EVENT_RING_SEGMENT_TRBS {
            panic!("xHCI: invalid event ring segment size",);
        }

        /*
         * ERST entry:
         *
         * +0x00 = segment base address
         * +0x08 = segment size
         * +0x0A = reserved
         * +0x0C = interrupter target
         */

        let erst = self.erst_virt as *mut u8;

        write_volatile(erst as *mut u64, self.event_ring.phys_addr);

        write_volatile(erst.add(8) as *mut u16, segment_size as u16);

        write_volatile(erst.add(12) as *mut u32, 0);

        dma_sync_for_device(self.erst_virt as *const u8, ERST_ENTRY_SIZE);

        /*
         * One ERST segment.
         */
        self.regs.set_erstsz(0, 1);

        /*
         * ERST base.
         */
        self.regs.set_erstba(0, self.erst_phys);

        /*
         * ERDP = first TRB, EHB cleared.
         */
        self.regs.set_erdp_clear_busy(0, self.event_dequeue);

        crate::serial::write_str("xHCI: event ring = 0x");

        crate::serial::write_hex(self.event_ring.phys_addr);

        crate::serial::write_str("\n");

        crate::serial::write_str("xHCI: ERST = 0x");

        crate::serial::write_hex(self.erst_phys);

        crate::serial::write_str("\n");

        crate::serial::write_str("xHCI: ERSTSZ = ");

        crate::serial::write_hex(self.regs.erstsz(0) as u64);

        crate::serial::write_str("\n");

        crate::serial::write_str("xHCI: ERDP = 0x");

        crate::serial::write_hex(self.regs.erdp(0));

        crate::serial::write_str("\n");
    }

    /*
     * ======================================================================
     * Run finished
     * ======================================================================
     *
     * Corresponds to Linux xhci_run_finished().
     *
     * ======================================================================
     */

    pub unsafe fn run(&mut self) {
        if !self.initialized {
            panic!("xHCI: run() called before init()",);
        }

        if self.running {
            return;
        }

        crate::serial::write_str("xHCI: xhci_run_finished() started\n");

        /*
         * ==================================================================
         * Enable controller event interrupts.
         * ==================================================================
         *
         * Linux does:
         *
         *     temp = readl(&op_regs->command);
         *     temp |= CMD_EIE;
         *     writel(temp, ...);
         *
         * CMD_EIE is what this driver calls USBCMD_INTE.
         */

        let mut command = self.regs.usbcmd();

        if command == u32::MAX {
            panic!("xHCI: USBCMD reads as all ones",);
        }

        command |= USBCMD_INTE;

        self.regs.set_usbcmd(command);

        crate::serial::write_str("xHCI: controller event interrupts enabled\n");

        /*
         * ==================================================================
         * Enable primary interrupter.
         * ==================================================================
         */

        self.enable_primary_interrupter();

        /*
         * ==================================================================
         * Start controller.
         * ==================================================================
         */

        self.start();

        /*
         * Linux records a 500 ms run grace-period after the host starts.
         * Rusty deliberately waits through that interval before inspecting
         * and resetting root ports, because this driver targets real hardware
         * and intentionally uses a conservative stabilization delay.
         */
        crate::delay::delay_us(XHCI_RUN_GRACE_PERIOD_US);

        /*
         * Make sure software-controlled root ports have VBUS/port power before
         * attempting the USB reset sequences below.
         */
        self.ensure_root_ports_powered();

        /*
         * Linux exposes two root hubs to USB core.  Rusty does not have the
         * full USB root-hub layer yet, so seed our non-blocking port state
         * machine from the current hardware state.
         */
        self.running = true;
        self.service_connected_ports();
        self.service_pending_port_operations();

        /*
         * Linux transitions command-ring state to RUNNING here.
         *
         * This driver has no separate command_ring_state enum, so the
         * running flag represents the controller-running state.
         */

        crate::serial::write_str("xHCI: PORTSC after initial root-port service:\n");
        self.dump_ports();
        crate::serial::write_str("xHCI: initial root-port PORTSC dump complete\n");

        crate::serial::write_str("xHCI: xhci_run_finished() finished\n");
    }

    /*
     * ======================================================================
     * Primary interrupter enable
     * ======================================================================
     *
     * This is NOT part of xhci_init().
     * ======================================================================
     */

    unsafe fn enable_primary_interrupter(&self) {
        /*
         * Use no interrupt moderation while the native xHCI driver is being
         * brought up.  A zero IMOD gives us the fastest path from an xHCI event
         * to the PCI MSI/MSI-X message.  Moderation can be added later once the
         * event-driven path is stable.
         */
        self.regs.set_imod(0, 0);

        /*
         * USBSTS.EINT is the controller's event-interrupt status and must be
         * cleared before acknowledging IMAN.IP.
         */
        self.regs.set_usbsts(USBSTS_EINT);

        /*
         * IMAN.IP is RW1C. Writing zero does NOT clear it.
         * Write the intended interrupter state explicitly: IE=1 and IP=1
         * to clear any stale pending interrupt while enabling IE.
         */
        self.regs.set_iman(0, IMAN_IE | IMAN_IP);

        /*
         * Flush posted MMIO write.
         */
        let _ = self.regs.iman(0);

        crate::serial::write_str("xHCI: primary interrupter enabled, IMAN=0x");

        crate::serial::write_hex(self.regs.iman(0) as u64);

        crate::serial::write_str("\n");
    }

    /*
     * ======================================================================
     * Start
     * ======================================================================
     */

    pub unsafe fn start(&self) {
        let command = self.regs.usbcmd();

        if command == u32::MAX {
            panic!("xHCI: USBCMD reads as all ones",);
        }

        self.regs.set_usbcmd(command | USBCMD_RUN);

        self.wait_for_status(USBSTS_HCH, 0, CONTROLLER_START_TIMEOUT_US);
        self.wait_for_status(USBSTS_CNR, 0, CONTROLLER_START_TIMEOUT_US);

        if self.regs.halted() {
            panic!("xHCI: controller failed to start",);
        }

        crate::serial::write_str("xHCI: controller started\n");
    }

    /*
     * ======================================================================
     * Halt
     * ======================================================================
     */

    pub unsafe fn halt(&self) {
        let mut command = self.regs.usbcmd();

        if command == u32::MAX {
            panic!("xHCI: controller inaccessible while halting",);
        }

        command &= !(USBCMD_RUN | USBCMD_INTE | USBCMD_HSEE);

        self.regs.set_usbcmd(command);

        if self.regs.halted() {
            return;
        }

        self.wait_for_status(USBSTS_HCH, USBSTS_HCH, CONTROLLER_HALT_TIMEOUT_US);
    }

    /*
     * ======================================================================
     * Reset
     * ======================================================================
     *
     * Kept separate from xhci_init().
     *
     * Linux also separates xhci_reset() from xhci_init().
     * ======================================================================
     */

    pub unsafe fn reset(&self) {
        if !self.regs.halted() {
            self.halt();
        }

        let command = self.regs.usbcmd();

        if command == u32::MAX {
            panic!("xHCI: controller inaccessible during reset",);
        }

        self.regs.set_usbcmd(command | USBCMD_RESET);

        /*
         * HCRST is self-clearing.  Do not touch HC registers immediately after
         * asserting it; Linux documents a 1 ms minimum delay for affected Intel
         * controllers, and Rusty uses the conservative delay unconditionally.
         */
        fence(Ordering::SeqCst);
        crate::delay::delay_us(XHCI_POST_RESET_ACCESS_DELAY_US);

        self.wait_for_command_clear(USBCMD_RESET, XHCI_RESET_LONG_TIMEOUT_US);

        /*
         * xHCI cannot safely accept doorbell/operational accesses until
         * Controller Not Ready has cleared.
         */

        self.wait_for_status(USBSTS_CNR, 0, XHCI_RESET_LONG_TIMEOUT_US);

        /*
         * Match the hardware-tested Stellux sequence: once HCRST and CNR
         * have cleared, give the controller a further 50 ms before touching
         * the operational register defaults.
         */
        crate::delay::delay_us(XHCI_POST_RESET_SETTLE_DELAY_US);

        let status = self.regs.usbsts();

        if status == u32::MAX {
            panic!("xHCI: controller disappeared during reset",);
        }

        if (status & USBSTS_HCE) != 0 {
            panic!("xHCI: Host Controller Error during reset",);
        }

        /*
         * Verify the operational reset defaults like Stellux does before
         * programming any xHCI operational structures.
         */
        let usbcmd = self.regs.usbcmd();
        let dnctrl = read_volatile(
            self.operational_base
                .checked_add(DEV_NOTIFICATION_OFFSET)
                .expect("xHCI: DNCTRL address overflow") as *const u32,
        );
        let crcr = self.regs.crcr();
        let dcbaap = self.regs.dcbaap();
        let config = self.regs.config();

        crate::serial::write_str("xHCI: reset defaults USBCMD=0x");
        crate::serial::write_hex(usbcmd as u64);
        crate::serial::write_str(" DNCTRL=0x");
        crate::serial::write_hex(dnctrl as u64);
        crate::serial::write_str(" CRCR=0x");
        crate::serial::write_hex(crcr);
        crate::serial::write_str(" DCBAAP=0x");
        crate::serial::write_hex(dcbaap);
        crate::serial::write_str(" CONFIG=0x");
        crate::serial::write_hex(config as u64);
        crate::serial::write_str("\n");

        if usbcmd != 0 || dnctrl != 0 || crcr != 0 || dcbaap != 0 || config != 0 {
            panic!("xHCI: controller did not return expected reset defaults");
        }

        /*
         * HCRST returns the xHC operational state and root-port state machines
         * to their reset/default state. CNR is clear and reset-defaults have
         * been verified, so this snapshot is safe for hardware debugging.
         */
        crate::serial::write_str("xHCI: PORTSC after xHC reset and reset-default validation:\n");
        self.dump_ports();
        crate::serial::write_str("xHCI: post-reset PORTSC dump complete\n");

        crate::serial::write_str("xHCI: reset complete\n");
    }

    // wait for status

    /*
     * ======================================================================
     * Timed MMIO handshakes
     * ======================================================================
     *
     * Time is measured with the kernel's calibrated TSC clock rather than a
     * "number of loop iterations" counter.  This makes the timeout independent
     * of CPU speed, interrupt activity, and MMIO latency.
     */
    fn wait_for_status(&self, mask: u32, expected: u32, timeout_us: u64) {
        let start_us = crate::delay::now_us();
        let deadline_us = start_us.saturating_add(timeout_us);

        loop {
            let status = self.regs.usbsts();

            if status == u32::MAX {
                panic!("xHCI: controller became inaccessible");
            }

            if (status & mask) == expected {
                return;
            }

            if crate::delay::now_us() >= deadline_us {
                crate::serial::write_str("xHCI: status timeout, USBSTS=0x");
                crate::serial::write_hex(self.regs.usbsts() as u64);
                crate::serial::write_str("\n");
                panic!("xHCI: timed out waiting for controller status");
            }

            crate::delay::delay_us(XHCI_POLL_INTERVAL_US);
        }
    }

    fn wait_for_command_clear(&self, mask: u32, timeout_us: u64) {
        let start_us = crate::delay::now_us();
        let deadline_us = start_us.saturating_add(timeout_us);
        let all_ones_deadline_us =
            start_us.saturating_add(XHCI_RESET_ALL_ONES_GRACE_US);

        loop {
            let command = self.regs.usbcmd();
            let now_us = crate::delay::now_us();

            if command == u32::MAX {
                /*
                 * HCRST can temporarily make register reads return all ones
                 * on some PCIe xHCI implementations.  Do not immediately call
                 * the controller dead; allow the explicit short grace window.
                 */
                if now_us >= all_ones_deadline_us {
                    panic!("xHCI: controller became inaccessible during reset");
                }
            } else if (command & mask) == 0 {
                return;
            }

            if now_us >= deadline_us {
                crate::serial::write_str("xHCI: USBCMD timeout, value=0x");
                crate::serial::write_hex(self.regs.usbcmd() as u64);
                crate::serial::write_str("\n");
                panic!("xHCI: timed out waiting for USBCMD");
            }

            crate::delay::delay_us(XHCI_POLL_INTERVAL_US);
        }
    }

    /*
     * ======================================================================
     * Scratchpads
     * ======================================================================
     */

    unsafe fn init_scratchpads(&mut self) {
        self.scratchpad_page_size = self.supported_page_size();

        if self.scratchpad_count == 0 {
            crate::serial::write_str("xHCI: no scratchpad buffers required\n");

            return;
        }

        let page_size = self.scratchpad_page_size;

        crate::serial::write_str("xHCI: scratchpad page size = ");

        crate::serial::write_hex(page_size as u64);

        crate::serial::write_str("\n");

        /*
         * Scratchpad Buffer Array.
         */

        let array_size = self
            .scratchpad_count
            .checked_mul(DCBAA_ENTRY_SIZE)
            .expect("xHCI: scratchpad array size overflow");

        let array_phys = memory::allocate_dma_region(array_size, DCBAA_ALIGNMENT, None)
            .expect("xHCI: failed to allocate scratchpad array");

        let array_virt = memory::physical_to_virtual(array_phys) as usize;

        if array_virt == 0 {
            panic!("xHCI: invalid scratchpad array virtual address",);
        }

        self.validate_dma_range(array_phys, array_size, "scratchpad array");

        write_bytes(array_virt as *mut u8, 0, array_size);

        self.scratchpad_array_phys = array_phys;

        self.scratchpad_array_virt = array_virt;

        /*
         * Allocate scratchpad buffers.
         */

        for index in 0..self.scratchpad_count {
            let page_phys = memory::allocate_dma_region(page_size, page_size, None)
                .expect("xHCI: failed to allocate scratchpad page");

            self.validate_dma_range(page_phys, page_size, "scratchpad page");

            if (page_phys & (page_size as u64 - 1)) != 0 {
                panic!("xHCI: scratchpad page is not correctly aligned",);
            }

            /*
             * xHCI requires every newly allocated scratchpad buffer to be
             * cleared to zero before its address is handed to the controller.
             * After this point software must not touch the scratchpad buffer.
             */
            let page_virt = memory::physical_to_virtual(page_phys) as usize;

            if page_virt == 0 {
                panic!("xHCI: invalid scratchpad page virtual address",);
            }

            write_bytes(page_virt as *mut u8, 0, page_size);

            dma_sync_for_device(page_virt as *const u8, page_size);

            /*
             * Scratchpad Buffer Array entry = physical base of this
             * PAGESIZE-aligned scratchpad buffer.  The low address bits are
             * therefore zero as required by the xHCI data structure.
             */
            write_volatile(
                (self.scratchpad_array_virt as *mut u64).add(index),
                page_phys,
            );
        }

        dma_sync_for_device(
            self.scratchpad_array_virt as *const u8,
            array_size,
        );

        /*
         * DCBAA[0] = scratchpad buffer array.
         */

        write_volatile(self.dcbaa_virt as *mut u64, array_phys);
        dma_sync_for_device(self.dcbaa_virt as *const u8, DCBAA_ENTRY_SIZE);

        crate::serial::write_str("xHCI: scratchpad array = 0x");

        crate::serial::write_hex(array_phys);

        crate::serial::write_str("\n");

        crate::serial::write_str("xHCI: scratchpads initialized = ");

        crate::serial::write_hex(self.scratchpad_count as u64);

        crate::serial::write_str("\n");
    }

    /*
     * ======================================================================
     * Debug state
     * ======================================================================
     */

    pub fn debug_state(&self) {
        let command = self.regs.usbcmd();

        let status = self.regs.usbsts();

        crate::serial::write_str("xHCI STATE:\n");

        crate::serial::write_str("  USBCMD = 0x");

        crate::serial::write_hex(command as u64);

        crate::serial::write_str("\n");

        crate::serial::write_str("  USBSTS = 0x");

        crate::serial::write_hex(status as u64);

        crate::serial::write_str("\n");

        crate::serial::write_str("  CONFIG = 0x");

        crate::serial::write_hex(self.regs.config() as u64);

        crate::serial::write_str("\n");

        crate::serial::write_str("  CRCR = 0x");

        crate::serial::write_hex(self.regs.crcr());

        crate::serial::write_str("\n");

        crate::serial::write_str("  DCBAAP = 0x");

        crate::serial::write_hex(self.regs.dcbaap());

        crate::serial::write_str("\n");

        crate::serial::write_str("  PAGESIZE = 0x");

        crate::serial::write_hex(self.regs.pagesize() as u64);

        crate::serial::write_str("\n");

        crate::serial::write_str("  DBOFF = 0x");

        crate::serial::write_hex(self.doorbell_base as u64);

        crate::serial::write_str("\n");

        crate::serial::write_str("  IMAN[0] = 0x");

        crate::serial::write_hex(self.regs.iman(0) as u64);

        crate::serial::write_str("\n");

        crate::serial::write_str("  ERSTSZ[0] = 0x");

        crate::serial::write_hex(self.regs.erstsz(0) as u64);

        crate::serial::write_str("\n");

        crate::serial::write_str("  ERSTBA[0] = 0x");

        crate::serial::write_hex(self.regs.erstba(0));

        crate::serial::write_str("\n");

        crate::serial::write_str("  ERDP[0] = 0x");

        crate::serial::write_hex(self.regs.erdp(0));

        crate::serial::write_str("\n");

        crate::serial::write_str("  Event dequeue = 0x");

        crate::serial::write_hex(self.event_dequeue);

        crate::serial::write_str("\n");

        crate::serial::write_str("  Ready port = ");

        crate::serial::write_hex(self.ready_port as u64);

        crate::serial::write_str("\n");

        if let Some(completion) = self.last_command_completion {
            crate::serial::write_str("  Last command completion = code=0x");
            crate::serial::write_hex(completion.completion_code as u64);
            crate::serial::write_str(" slot=0x");
            crate::serial::write_hex(completion.slot_id as u64);
            crate::serial::write_str(" cmd=0x");
            crate::serial::write_hex(completion.command_trb);
            crate::serial::write_str("\n");
        }

        crate::serial::write_str("  Scratchpads = ");

        crate::serial::write_hex(self.scratchpad_count as u64);

        crate::serial::write_str("\n");

        crate::serial::write_str("  Scratchpad page = 0x");

        crate::serial::write_hex(self.scratchpad_page_size as u64);

        crate::serial::write_str("\n");

        for slot in 1..=self.max_slots.min(MAX_XHCI_SLOTS) {
            if self.device_context_phys[slot] == 0 {
                continue;
            }

            crate::serial::write_str("  Slot ");
            crate::serial::write_hex(slot as u64);
            crate::serial::write_str(" Device Context=0x");
            crate::serial::write_hex(self.device_context_phys[slot]);
            crate::serial::write_str(" Input Context=0x");
            crate::serial::write_hex(self.input_context_phys[slot]);
            crate::serial::write_str("\n");
        }

        crate::serial::write_str("  Initialized = ");

        crate::serial::write_str(if self.initialized { "1" } else { "0" });

        crate::serial::write_str("\n");

        crate::serial::write_str("  Running = ");

        crate::serial::write_str(if self.running { "1" } else { "0" });

        crate::serial::write_str("\n");
    }

    /*
     * ======================================================================
     * Root-port scanner
     * ======================================================================
     *
     * This only READS port state.
     *
     * It does not:
     *   - reset ports
     *   - clear change bits
     *   - enable ports
     *   - enumerate devices
     *
     * It reports the current state of every xHCI root port.
     * ======================================================================
     */

    pub fn dump_ports(&self) {
        crate::serial::write_str("xHCI: scanning root ports...\n");

        /*
         * Report every physical PORTSC, but explicitly identify which
         * Supported Protocol root hub owns it.
         *
         * A USB3 companion can legitimately be CCS=0 / RxDetect while the
         * USB2 companion of the same physical connector reports the device.
         */
        for port in 1..=self.max_ports {
            let portsc = unsafe { self.regs.portsc(port) };

            let connected = (portsc & PORTSC_CCS) != 0;
            let enabled = (portsc & PORTSC_PED) != 0;
            let resetting = (portsc & PORTSC_PR) != 0;
            let powered = (portsc & PORTSC_PP) != 0;

            let pls = ((portsc & PORTSC_PLS_MASK) >> 5) as u8;
            let speed = ((portsc & PORTSC_SPEED_MASK) >> 10) as u8;

            let csc = (portsc & PORTSC_CSC) != 0;
            let pec = (portsc & PORTSC_PEC) != 0;
            let wrc = (portsc & PORTSC_WRC) != 0;
            let occ = (portsc & PORTSC_OCC) != 0;
            let prc = (portsc & PORTSC_PRC) != 0;
            let plc = (portsc & PORTSC_PLC) != 0;
            let cec = (portsc & PORTSC_CEC) != 0;
            let cas = (portsc & PORTSC_CAS) != 0;

            let protocol = self.port_protocol_for(port);

            crate::serial::write_str("xHCI: PORT ");
            crate::serial::write_hex(port as u64);
            crate::serial::write_str(" [");
            crate::serial::write_str(protocol.label());
            crate::serial::write_str("]: ");

            match (connected, powered) {
                (true, true) => crate::serial::write_str("CONNECTED + POWERED"),
                (true, false) => crate::serial::write_str("CONNECTED + UNPOWERED"),
                (false, true) => crate::serial::write_str("DISCONNECTED + POWERED"),
                (false, false) => crate::serial::write_str("DISCONNECTED + UNPOWERED"),
            }

            crate::serial::write_str(" PORTSC=0x");
            crate::serial::write_hex(portsc as u64);

            crate::serial::write_str(" CCS=");
            crate::serial::write_str(if connected { "1" } else { "0" });

            crate::serial::write_str(" PED=");
            crate::serial::write_str(if enabled { "1" } else { "0" });

            crate::serial::write_str(" PR=");
            crate::serial::write_str(if resetting { "1" } else { "0" });

            crate::serial::write_str(" PP=");
            crate::serial::write_str(if powered { "1" } else { "0" });

            crate::serial::write_str(" PLS=");
            crate::serial::write_hex(pls as u64);
            crate::serial::write_str(" (");

            match pls {
                0 => crate::serial::write_str("U0"),
                1 => crate::serial::write_str("U1"),
                2 => crate::serial::write_str("U2"),
                3 => crate::serial::write_str("U3"),
                4 => crate::serial::write_str("Disabled"),
                5 => crate::serial::write_str("RxDetect"),
                6 => crate::serial::write_str("Inactive"),
                7 => crate::serial::write_str("Polling"),
                8 => crate::serial::write_str("Recovery"),
                9 => crate::serial::write_str("HotReset"),
                10 => crate::serial::write_str("Compliance"),
                11 => crate::serial::write_str("TestMode"),
                15 => crate::serial::write_str("Resume"),
                _ => crate::serial::write_str("Reserved"),
            }

            crate::serial::write_str(")");

            crate::serial::write_str(" SPEED=");
            crate::serial::write_hex(speed as u64);
            crate::serial::write_str(" (");

            match speed {
                0 => crate::serial::write_str("Undefined"),
                1 => crate::serial::write_str("FullSpeed"),
                2 => crate::serial::write_str("LowSpeed"),
                3 => crate::serial::write_str("HighSpeed"),
                4 => crate::serial::write_str("SuperSpeed"),
                5 => crate::serial::write_str("SuperSpeedPlus"),
                _ => crate::serial::write_str("Reserved"),
            }

            crate::serial::write_str(")");

            crate::serial::write_str(" CHG=[");
            let mut first_change = true;

            if csc {
                crate::serial::write_str("CSC");
                first_change = false;
            }

            if pec {
                if !first_change {
                    crate::serial::write_str(",");
                }
                crate::serial::write_str("PEC");
                first_change = false;
            }

            if wrc {
                if !first_change {
                    crate::serial::write_str(",");
                }
                crate::serial::write_str("WRC");
                first_change = false;
            }

            if occ {
                if !first_change {
                    crate::serial::write_str(",");
                }
                crate::serial::write_str("OCC");
                first_change = false;
            }

            if prc {
                if !first_change {
                    crate::serial::write_str(",");
                }
                crate::serial::write_str("PRC");
                first_change = false;
            }

            if plc {
                if !first_change {
                    crate::serial::write_str(",");
                }
                crate::serial::write_str("PLC");
                first_change = false;
            }

            if cec {
                if !first_change {
                    crate::serial::write_str(",");
                }
                crate::serial::write_str("CEC");
                first_change = false;
            }

            if first_change {
                crate::serial::write_str("none");
            }

            crate::serial::write_str("]");

            if cas {
                crate::serial::write_str(" CAS=1");
            }

            crate::serial::write_str("\n");
        }

        crate::serial::write_str("xHCI: USB2 root-hub ports=");
        crate::serial::write_hex(self.usb2_port_count as u64);
        crate::serial::write_str(" USB3 root-hub ports=");
        crate::serial::write_hex(self.usb3_port_count as u64);
        crate::serial::write_str("\n");

        crate::serial::write_str("xHCI: root-port scan complete\n");
    }

    /*
     * ======================================================================
     * Root-port power
     * ======================================================================
     *
     * A full USB root-hub implementation would normally perform this power
     * sequencing for us. Rusty does not have that layer yet, so when the xHC
     * advertises HCCPARAMS1.PPC we explicitly power the root ports before
     * attempting device reset/enumeration.
     */
    unsafe fn ensure_root_ports_powered(&self) {
        let hccparams1_address = self
            .mmio_base
            .checked_add(HCCPARAMS1_OFFSET)
            .expect("xHCI: HCCPARAMS1 address overflow");

        let hccparams1 = read_volatile(hccparams1_address as *const u32);

        if hccparams1 == u32::MAX {
            panic!("xHCI: HCCPARAMS1 reads as all ones while powering ports");
        }

        if (hccparams1 & HCCPARAMS1_PPC) == 0 {
            /*
             * When PPC is clear, port power is not software-controlled. The
             * platform/controller provides the port power state instead.
             */
            crate::serial::write_str(
                "xHCI: root-port power is not software controlled (PPC=0)\n",
            );
            return;
        }

        let mut powered_ports = 0usize;

        for port in 1..=self.max_ports {
            let portsc = self.regs.portsc(port);

            if (portsc & PORTSC_PP) != 0 {
                continue;
            }

            let write_value = Self::portsc_to_neutral(portsc) | PORTSC_PP;

            crate::serial::write_str("xHCI: powering root PORT ");
            crate::serial::write_hex(port as u64);
            crate::serial::write_str(" PORTSC=0x");
            crate::serial::write_hex(write_value as u64);
            crate::serial::write_str("\n");

            self.regs.set_portsc(port, write_value);
            let _ = self.regs.portsc(port);

            powered_ports += 1;
        }

        if powered_ports != 0 {
            /*
             * Give VBUS/port power a conservative settling interval before
             * root-port reset or enumeration is attempted.
             */
            crate::delay::delay_us(USB_PORT_POWER_STABILIZE_DELAY_US);

            crate::serial::write_str("xHCI: powered root ports = ");
            crate::serial::write_hex(powered_ports as u64);
            crate::serial::write_str("\n");
        }
    }

    /*
     * ======================================================================
     * Initial root-port service + non-blocking reset state machine
     * ======================================================================
     */

    pub unsafe fn service_connected_ports(&mut self) {
        crate::serial::write_str("xHCI: servicing connected root ports...\n");

        for index in 0..self.usb2_port_count {
            self.service_connected_port(self.usb2_ports[index] as usize);
        }

        for index in 0..self.usb3_port_count {
            self.service_connected_port(self.usb3_ports[index] as usize);
        }

        crate::serial::write_str("xHCI: connected root-port service complete\n");
    }

    unsafe fn service_connected_port(&mut self, port: usize) {
        let portsc = self.regs.portsc(port);
        let protocol = self.port_protocol_for(port);

        match protocol {
            UsbPortProtocol::Usb2 => {
                /*
                 * USB2 attach state: connected, disabled, not already reset.
                 * Do the reset asynchronously.  Do not gate the request on a
                 * single instantaneous PLS value.
                 */
                if (portsc & PORTSC_CSC) != 0
                    && (portsc & PORTSC_CCS) != 0
                    && (portsc & PORTSC_PED) == 0
                    && (portsc & PORTSC_PR) == 0
                {
                    self.request_usb2_port_reset(port);
                }
            }

            UsbPortProtocol::Usb3 => {
                /*
                 * USB3 does not use the USB2 PORTSC.PR reset.
                 *
                 * Warm reset is appropriate for the cold-attach condition
                 * (CAS) and for the SuperSpeed Inactive/Compliance recovery
                 * states.  A healthy connected U0 port is left alone.
                 */
                let pls = ((portsc & PORTSC_PLS_MASK) >> 5) as u8;
                let warm_reset_required =
                    (portsc & PORTSC_CAS) != 0
                        || pls == PORTSC_PLS_INACTIVE
                        || pls == PORTSC_PLS_COMPLIANCE;

                if warm_reset_required {
                    self.warm_reset_usb3_port(port);
                } else if (portsc & PORTSC_CCS) != 0
                    && (portsc & PORTSC_PED) != 0
                    && pls == 0
                {
                    crate::serial::write_str("xHCI: USB3 device ready on PORT ");
                    crate::serial::write_hex(port as u64);
                    crate::serial::write_str("\n");
                } else if (portsc & PORTSC_CCS) != 0 {
                    crate::serial::write_str(
                        "xHCI: USB3 device connected but link not ready PORT ",
                    );
                    crate::serial::write_hex(port as u64);
                    crate::serial::write_str(" PLS=");
                    crate::serial::write_hex(pls as u64);
                    crate::serial::write_str("\n");
                }
            }

            UsbPortProtocol::Unknown | UsbPortProtocol::Duplicate => {}
        }
    }

    /*
     * ======================================================================
     * Advance all outstanding root-port operations without blocking
     * ======================================================================
     *
     * This is deliberately called from the normal polling path rather than
     * from inside a Port Status Change Event handler.  These helpers never
     * acknowledge RW1C change bits themselves; the caller first interprets
     * the resulting state and then acknowledges only the bits it consumed.
     */
    unsafe fn service_pending_port_operations(&mut self) {
        /*
         * Advance each state machine once.  Completion of a reset may happen
         * without a corresponding event being delivered to the software poller,
         * so the normal path may also acknowledge the associated RW1C bit after
         * taking a fresh snapshot.
         */
        for port in 1..=self.max_ports {
            let reset_result = self.service_pending_port_reset(port);

            if matches!(
                reset_result,
                PendingOperationResult::Complete
                    | PendingOperationResult::Cancelled
                    | PendingOperationResult::TimedOut
            ) {
                let protocol = self.port_protocol_for(port);

                if protocol == UsbPortProtocol::Usb2 {
                    let portsc = self.regs.portsc(port);

                    /*
                     * PRC is acknowledged only after the software has interpreted
                     * the post-reset snapshot.  A newly asserted PRC is the same
                     * hardware condition we are waiting for, so clearing it here
                     * does not erase an unrelated software event.
                     */
                    if (portsc & PORTSC_PRC) != 0 {
                        self.clear_port_change_bits(port, PORTSC_PRC);
                        crate::delay::delay_us(USB_RESET_POST_CLEAR_DELAY_US);
                    }
                }
            }

            let warm_result = self.service_pending_usb3_warm_reset(port);

            if matches!(
                warm_result,
                PendingOperationResult::Complete
                    | PendingOperationResult::Cancelled
                    | PendingOperationResult::TimedOut
            ) {
                let protocol = self.port_protocol_for(port);

                if protocol == UsbPortProtocol::Usb3 {
                    let portsc = self.regs.portsc(port);

                    /*
                     * WRC is the warm-reset completion/change bit.  As above,
                     * acknowledge it only after interpreting the fresh state.
                     */
                    if (portsc & PORTSC_WRC) != 0 {
                        self.clear_port_change_bits(port, PORTSC_WRC);
                    }
                }
            }
        }
    }

    unsafe fn service_pending_port_reset(&mut self, port: usize) -> PendingOperationResult {
        if port == 0 || port > self.max_ports || !self.port_reset_pending[port - 1] {
            return PendingOperationResult::NotPending;
        }

        if self.port_protocol_for(port) != UsbPortProtocol::Usb2 {
            self.port_reset_pending[port - 1] = false;
            self.port_reset_deadline_us[port - 1] = 0;
            return PendingOperationResult::Cancelled;
        }

        let value = self.regs.portsc(port);
        let now_us = crate::delay::now_us();

        if (value & PORTSC_CCS) == 0 {
            crate::serial::write_str("xHCI: USB2 device disconnected during reset PORT ");
            crate::serial::write_hex(port as u64);
            crate::serial::write_str("\n");

            self.port_reset_pending[port - 1] = false;
            self.port_reset_deadline_us[port - 1] = 0;
            return PendingOperationResult::Cancelled;
        }

        /*
         * Match Stellux's hardware-tested completion condition: for USB2 the
         * reset is considered complete when PR has cleared and PRC is set.
         * We then require PED=1 before allowing enumeration to proceed.
         */
        if (value & PORTSC_PR) != 0 || (value & PORTSC_PRC) == 0 {
            if now_us >= self.port_reset_deadline_us[port - 1] {
                crate::serial::write_str("xHCI: USB2 port reset timed out waiting for PR/PRC PORT ");
                crate::serial::write_hex(port as u64);
                crate::serial::write_str(" PORTSC=0x");
                crate::serial::write_hex(value as u64);
                crate::serial::write_str("\n");

                self.port_reset_pending[port - 1] = false;
                self.port_reset_deadline_us[port - 1] = 0;
                return PendingOperationResult::TimedOut;
            }

            /* Preserve the Stellux behavior of checking approximately every
             * millisecond instead of spinning indefinitely on the same read. */
            crate::delay::delay_us(USB2_ROOT_RESET_DELAY_US);
            return PendingOperationResult::Pending;
        }

        let pls = ((value & PORTSC_PLS_MASK) >> 5) as u8;
        let enabled = (value & PORTSC_PED) != 0;

        crate::serial::write_str("xHCI: USB2 port reset completion PORT ");
        crate::serial::write_hex(port as u64);
        crate::serial::write_str(" PORTSC=0x");
        crate::serial::write_hex(value as u64);
        crate::serial::write_str(" PED=");
        crate::serial::write_str(if enabled { "1" } else { "0" });
        crate::serial::write_str(" PLS=");
        crate::serial::write_hex(pls as u64);
        crate::serial::write_str("\n");

        if !enabled {
            if now_us >= self.port_reset_deadline_us[port - 1] {
                self.port_reset_pending[port - 1] = false;
                self.port_reset_deadline_us[port - 1] = 0;
                return PendingOperationResult::TimedOut;
            }

            crate::delay::delay_us(USB2_ROOT_RESET_DELAY_US);
            return PendingOperationResult::Pending;
        }

        self.port_reset_pending[port - 1] = false;
        self.port_reset_deadline_us[port - 1] = 0;
        self.ready_port = port;

        /* Stellux waits 3 ms after reset completion before clearing change
         * bits and another 3 ms before re-reading PED. */
        crate::delay::delay_us(USB_RESET_RECOVERY_DELAY_US);

        crate::serial::write_str("xHCI: USB2 root-port ready for enumeration PORT ");
        crate::serial::write_hex(port as u64);
        crate::serial::write_str("\n");

        PendingOperationResult::Complete
    }

    unsafe fn service_pending_usb3_warm_reset(&mut self, port: usize) -> PendingOperationResult {
        if port == 0 || port > self.max_ports || !self.usb3_warm_reset_pending[port - 1] {
            return PendingOperationResult::NotPending;
        }

        if self.port_protocol_for(port) != UsbPortProtocol::Usb3 {
            self.usb3_warm_reset_pending[port - 1] = false;
            self.usb3_warm_reset_deadline_us[port - 1] = 0;
            return PendingOperationResult::Cancelled;
        }

        let value = self.regs.portsc(port);
        let now_us = crate::delay::now_us();

        if (value & PORTSC_WRC) != 0 && (value & PORTSC_PR) == 0 {
            let connected = (value & PORTSC_CCS) != 0;
            let enabled = (value & PORTSC_PED) != 0;
            let pls = ((value & PORTSC_PLS_MASK) >> 5) as u8;

            crate::serial::write_str("xHCI: USB3 warm reset complete PORT ");
            crate::serial::write_hex(port as u64);
            crate::serial::write_str(" CCS=");
            crate::serial::write_str(if connected { "1" } else { "0" });
            crate::serial::write_str(" PED=");
            crate::serial::write_str(if enabled { "1" } else { "0" });
            crate::serial::write_str(" PLS=");
            crate::serial::write_hex(pls as u64);
            crate::serial::write_str("\n");

            /*
             * WRC says the warm reset operation completed. For enumeration,
             * require the SuperSpeed port to have returned to U0/enabled.
             * If it has not, leave the operation pending and allow the normal
             * polling path to observe the link settling until the 800 ms
             * deadline.
             */
            if !connected || !enabled || pls != 0 {
                if !connected {
                    crate::serial::write_str(
                        "xHCI: USB3 device disconnected during warm-reset recovery PORT ",
                    );
                    crate::serial::write_hex(port as u64);
                    crate::serial::write_str("\n");

                    self.usb3_warm_reset_pending[port - 1] = false;
                    self.usb3_warm_reset_deadline_us[port - 1] = 0;
                    return PendingOperationResult::Cancelled;
                }

                if now_us >= self.usb3_warm_reset_deadline_us[port - 1] {
                    self.usb3_warm_reset_pending[port - 1] = false;
                    self.usb3_warm_reset_deadline_us[port - 1] = 0;
                    return PendingOperationResult::TimedOut;
                }

                return PendingOperationResult::Pending;
            }

            self.usb3_warm_reset_pending[port - 1] = false;
            self.usb3_warm_reset_deadline_us[port - 1] = 0;

            /* Linux applies a normal post-reset recovery interval here. */
            crate::delay::delay_us(USB_RESET_RECOVERY_DELAY_US);

            return PendingOperationResult::Complete;
        }

        if (value & PORTSC_CCS) == 0 && (value & PORTSC_CAS) == 0 {
            crate::serial::write_str("xHCI: USB3 device disappeared during warm reset PORT ");
            crate::serial::write_hex(port as u64);
            crate::serial::write_str("\n");

            self.usb3_warm_reset_pending[port - 1] = false;
            self.usb3_warm_reset_deadline_us[port - 1] = 0;

            return PendingOperationResult::Cancelled;
        }

        if now_us >= self.usb3_warm_reset_deadline_us[port - 1] {
            crate::serial::write_str("xHCI: USB3 warm reset timed out PORT ");
            crate::serial::write_hex(port as u64);
            crate::serial::write_str("\n");

            self.usb3_warm_reset_pending[port - 1] = false;
            self.usb3_warm_reset_deadline_us[port - 1] = 0;

            return PendingOperationResult::TimedOut;
        }

        PendingOperationResult::Pending
    }

    /*
     * ======================================================================
     * Service one xHCI hardware interrupt
     * ======================================================================
     *
     * The actual IDT handler is intentionally tiny. It only sets an atomic
     * pending flag and sends LAPIC EOI. The normal kernel/USB service context
     * calls this method to drain the event ring.
     * ======================================================================
     */

    pub unsafe fn service_interrupt(&mut self) {
        let pending = crate::interrupts::take_xhci_interrupt();
        let controller_pending = (self.regs.iman(0) & IMAN_IP) != 0
            || (self.regs.usbsts() & USBSTS_EINT) != 0;

        if pending || controller_pending {
            self.poll_events();
        }
    }

    /*
     * ======================================================================
     * Event polling
     * ======================================================================
     */

    pub unsafe fn poll_events(&mut self) {
        if !self.running {
            return;
        }

        let mut processed = 0usize;

        loop {
            if processed >= self.event_ring.capacity() {
                break;
            }

            /*
             * On coherent x86 this is conservative, but it makes the DMA
             * visibility contract explicit: flush the cache line containing
             * the event TRB before consuming controller-written data.
             */
            dma_sync_for_cpu(
                self.event_ring.buffer.as_ptr().add(self.event_ring.index) as *const u8,
                XHCI_TRB_SIZE,
            );

            let trb = self.event_ring.read_current();

            if trb.cycle_bit() != self.event_ring.current_cycle() {
                break;
            }

            self.handle_event(trb);

            self.event_ring.advance();
            processed += 1;
        }

        /*
         * Advance root-port reset state outside the event handler.
         * The state machines never acknowledge PRC/WRC themselves.
         */
        self.service_pending_port_operations();

        /*
         * Also rescan current port state. This makes root-port reset/recovery
         * self-healing when hardware changes state without delivering a
         * consumable Port Status Change Event at exactly the expected moment.
         */
        self.service_connected_ports();

        self.event_dequeue = self.event_ring.current_phys_addr();

        fence(Ordering::SeqCst);
        self.regs.set_erdp_clear_busy(0, self.event_dequeue);

        if processed != 0 {
            crate::serial::write_str("xHCI: drained event TRBs = ");
            crate::serial::write_hex(processed as u64);
            crate::serial::write_str(" next ERDP = 0x");
            crate::serial::write_hex(self.event_dequeue);
            crate::serial::write_str("\n");
        }

        /*
         * Clear the controller's event-interrupt indication after the event
         * ring consumer pointer has been committed.  This must also happen
         * when the handler woke us but the consumer observed no additional
         * event TRB, otherwise IMAN.IP could remain asserted and a later edge
         * may never be delivered.
         *
         * IMAN.IP is RW1C. Writing IE=1 + IP=1 preserves enabled delivery while
         * acknowledging the pending interrupt.
         */
        let iman = self.regs.iman(0);
        let usbsts = self.regs.usbsts();

        if (iman & IMAN_IP) != 0 || (usbsts & USBSTS_EINT) != 0 {
            self.regs.set_usbsts(USBSTS_EINT);
            self.regs.set_iman(0, IMAN_IE | IMAN_IP);
        }
    }

    /*
     * ======================================================================
     * Event handling
     * ======================================================================
     */

    unsafe fn handle_event(&mut self, trb: Trb) {
        match trb.trb_type() {
            x if x == TrbType::CommandCompletionEvent as u8 => {
                self.handle_command_completion(trb);
            }

            x if x == TrbType::PortStatusChangeEvent as u8 => {
                self.handle_port_status_change(trb);
            }

            x if x == TrbType::TransferEvent as u8 => {
                self.handle_transfer_event(trb);
            }

            x if x == TrbType::HostControllerEvent as u8 => {
                crate::serial::write_str("xHCI: Host Controller Event\n");
            }

            _ => {
                crate::serial::write_str("xHCI: unhandled event TRB type=");

                crate::serial::write_hex(trb.trb_type() as u64);

                crate::serial::write_str(" parameter=0x");

                crate::serial::write_hex(trb.parameter);

                crate::serial::write_str("\n");
            }
        }
    }

    /*
     * ======================================================================
     * Command completion
     * ======================================================================
     */

    unsafe fn handle_command_completion(&mut self, trb: Trb) {
        let completion_code = (trb.status >> 24) as u8;

        let slot_id = (trb.control >> 24) as u8;

        self.last_command_completion = Some(CommandCompletion {
            command_trb: trb.parameter,
            completion_code,
            slot_id,
        });

        crate::serial::write_str("xHCI: command completion: code=");

        crate::serial::write_hex(completion_code as u64);

        crate::serial::write_str(" slot=");

        crate::serial::write_hex(slot_id as u64);

        crate::serial::write_str(" command=0x");

        crate::serial::write_hex(trb.parameter);

        crate::serial::write_str("\n");
    }

    /*
     * ======================================================================
     * Port Status Change
     * ======================================================================
     */

    unsafe fn handle_port_status_change(&mut self, trb: Trb) {
        let port = ((trb.parameter >> 24) & 0xFF) as usize;

        if port == 0 || port > self.max_ports {
            crate::serial::write_str("xHCI: invalid port status event\n");
            return;
        }

        let protocol = self.port_protocol_for(port);

        if matches!(
            protocol,
            UsbPortProtocol::Unknown | UsbPortProtocol::Duplicate
        ) {
            crate::serial::write_str("xHCI: ignoring port status event for invalid PORT ");
            crate::serial::write_hex(port as u64);
            crate::serial::write_str("\n");
            return;
        }

        /*
         * Take one PORTSC snapshot.  Interpret this snapshot first; only then
         * acknowledge the RW1C bits that this handler actually consumed.
         */
        let portsc = self.regs.portsc(port);

        let connected = (portsc & PORTSC_CCS) != 0;
        let enabled = (portsc & PORTSC_PED) != 0;
        let resetting = (portsc & PORTSC_PR) != 0;
        let pls = ((portsc & PORTSC_PLS_MASK) >> 5) as u8;

        let csc = (portsc & PORTSC_CSC) != 0;
        let pec = (portsc & PORTSC_PEC) != 0;
        let wrc = (portsc & PORTSC_WRC) != 0;
        let occ = (portsc & PORTSC_OCC) != 0;
        let prc = (portsc & PORTSC_PRC) != 0;
        let plc = (portsc & PORTSC_PLC) != 0;
        let cec = (portsc & PORTSC_CEC) != 0;
        let cas = (portsc & PORTSC_CAS) != 0;

        crate::serial::write_str("xHCI: port status change PORT ");
        crate::serial::write_hex(port as u64);
        crate::serial::write_str(" [");
        crate::serial::write_str(protocol.label());
        crate::serial::write_str("] PORTSC=0x");
        crate::serial::write_hex(portsc as u64);
        crate::serial::write_str(" CCS=");
        crate::serial::write_str(if connected { "1" } else { "0" });
        crate::serial::write_str(" PED=");
        crate::serial::write_str(if enabled { "1" } else { "0" });
        crate::serial::write_str(" PR=");
        crate::serial::write_str(if resetting { "1" } else { "0" });
        crate::serial::write_str(" PLS=");
        crate::serial::write_hex(pls as u64);
        crate::serial::write_str(" CHG=");
        crate::serial::write_str(if csc { "CSC " } else { "" });
        crate::serial::write_str(if pec { "PEC " } else { "" });
        crate::serial::write_str(if wrc { "WRC " } else { "" });
        crate::serial::write_str(if occ { "OCC " } else { "" });
        crate::serial::write_str(if prc { "PRC " } else { "" });
        crate::serial::write_str(if plc { "PLC " } else { "" });
        crate::serial::write_str(if cec { "CEC " } else { "" });
        crate::serial::write_str("\n");

        let mut changes_to_ack = 0u32;

        /*
         * Connection change is consumed first because unplugging a device
         * cancels any outstanding reset state for this physical port.
         */
        if csc {
            if connected {
                crate::serial::write_str("xHCI: connection asserted on PORT ");
                crate::serial::write_hex(port as u64);
                crate::serial::write_str("\n");
            } else {
                crate::serial::write_str("xHCI: connection removed from PORT ");
                crate::serial::write_hex(port as u64);
                crate::serial::write_str("\n");

                self.port_reset_pending[port - 1] = false;
                self.port_reset_deadline_us[port - 1] = 0;
                self.usb3_warm_reset_pending[port - 1] = false;
                self.usb3_warm_reset_deadline_us[port - 1] = 0;
            }

            changes_to_ack |= PORTSC_CSC;
        }

        if pec {
            crate::serial::write_str("xHCI: port enable change observed PORT ");
            crate::serial::write_hex(port as u64);
            crate::serial::write_str("\n");
            changes_to_ack |= PORTSC_PEC;
        }

        if occ {
            crate::serial::write_str("xHCI: over-current change observed PORT ");
            crate::serial::write_hex(port as u64);
            crate::serial::write_str("\n");
            changes_to_ack |= PORTSC_OCC;
        }

        if plc {
            crate::serial::write_str("xHCI: link-state change observed PORT ");
            crate::serial::write_hex(port as u64);
            crate::serial::write_str("\n");
            changes_to_ack |= PORTSC_PLC;
        }

        if cec {
            crate::serial::write_str("xHCI: configuration-error change observed PORT ");
            crate::serial::write_hex(port as u64);
            crate::serial::write_str("\n");
            changes_to_ack |= PORTSC_CEC;
        }

        match protocol {
            UsbPortProtocol::Usb2 => {
                /*
                 * USB2 uses the standard PORTSC.PR reset sequence.  PRC is
                 * completion/change notification, not a reason to start a
                 * second reset.
                 */
                if prc {
                    if self.port_reset_pending[port - 1] {
                        match self.service_pending_port_reset(port) {
                            PendingOperationResult::Complete
                            | PendingOperationResult::Cancelled
                            | PendingOperationResult::TimedOut => {
                                changes_to_ack |= PORTSC_PRC;
                            }
                            PendingOperationResult::Pending => {
                                /* Leave PRC set until the state is consumable. */
                            }
                            PendingOperationResult::NotPending => {
                                changes_to_ack |= PORTSC_PRC;
                            }
                        }
                    } else {
                        crate::serial::write_str(
                            "xHCI: stale USB2 PRC with no reset pending on PORT ",
                        );
                        crate::serial::write_hex(port as u64);
                        crate::serial::write_str("\n");
                        changes_to_ack |= PORTSC_PRC;
                    }
                }

                /*
                 * Only a newly asserted USB2 connection may initiate the
                 * ordinary port reset.  The hardware, not software, controls
                 * the exact pre-reset link-state transition, so do not reject
                 * a valid connected/disabled port solely because PLS is not
                 * exactly Polling at this instant.  Never trigger reset from
                 * PRC alone.
                 */
                if csc
                    && connected
                    && !enabled
                    && !resetting
                    && !self.port_reset_pending[port - 1]
                {
                    let _ = self.request_usb2_port_reset(port);
                }

                if wrc {
                    crate::serial::write_str(
                        "xHCI: unexpected USB3 warm-reset change on USB2 PORT ",
                    );
                    crate::serial::write_hex(port as u64);
                    crate::serial::write_str("\n");
                    changes_to_ack |= PORTSC_WRC;
                }
            }

            UsbPortProtocol::Usb3 => {
                /*
                 * USB3 connection does NOT trigger the USB2 PORTSC.PR reset.
                 * CAS and the SuperSpeed Inactive/Compliance states are the
                 * recovery conditions for which a warm reset is appropriate.
                 */
                let warm_reset_required =
                    cas
                        || pls == PORTSC_PLS_INACTIVE
                        || pls == PORTSC_PLS_COMPLIANCE;

                if warm_reset_required {
                    crate::serial::write_str(
                        "xHCI: USB3 warm-reset recovery condition on PORT ",
                    );
                    crate::serial::write_hex(port as u64);
                    crate::serial::write_str(" PLS=");
                    crate::serial::write_hex(pls as u64);
                    crate::serial::write_str(" CAS=");
                    crate::serial::write_str(if cas { "1" } else { "0" });
                    crate::serial::write_str("; requesting warm reset\n");
                    let _ = self.warm_reset_usb3_port(port);
                } else if connected {
                    if enabled && pls == 0 {
                        crate::serial::write_str("xHCI: USB3 device ready PORT ");
                        crate::serial::write_hex(port as u64);
                        crate::serial::write_str("\n");
                    } else {
                        crate::serial::write_str(
                            "xHCI: USB3 connected but link not ready PORT ",
                        );
                        crate::serial::write_hex(port as u64);
                        crate::serial::write_str(" PLS=");
                        crate::serial::write_hex(pls as u64);
                        crate::serial::write_str("\n");
                    }
                } else if csc {
                    crate::serial::write_str("xHCI: USB3 port disconnected PORT ");
                    crate::serial::write_hex(port as u64);
                    crate::serial::write_str("\n");
                }

                if wrc {
                    if self.usb3_warm_reset_pending[port - 1] {
                        match self.service_pending_usb3_warm_reset(port) {
                            PendingOperationResult::Complete
                            | PendingOperationResult::Cancelled
                            | PendingOperationResult::TimedOut => {
                                changes_to_ack |= PORTSC_WRC;
                            }
                            PendingOperationResult::Pending => {
                                /* Leave WRC set until the state is consumed. */
                            }
                            PendingOperationResult::NotPending => {
                                changes_to_ack |= PORTSC_WRC;
                            }
                        }
                    } else {
                        crate::serial::write_str(
                            "xHCI: stale USB3 WRC with no warm reset pending on PORT ",
                        );
                        crate::serial::write_hex(port as u64);
                        crate::serial::write_str("\n");
                        changes_to_ack |= PORTSC_WRC;
                    }
                }

                /* CAS is status, not an RW1C change bit.  Never write it back. */
            }

            UsbPortProtocol::Unknown | UsbPortProtocol::Duplicate => {}
        }

        /*
         * Acknowledge only bits whose meaning was consumed from this snapshot.
         * No raw PORTSC readback fields are replayed.
         */
        if changes_to_ack != 0 {
            self.clear_port_change_bits(port, changes_to_ack);
        }
    }

    /*
     * ======================================================================
     * Transfer Event
     * ======================================================================
     */

    unsafe fn handle_transfer_event(&mut self, trb: Trb) {
        let completion_code = (trb.status >> 24) as u8;

        let transfer_length = trb.status & 0x00FF_FFFF;

        let endpoint_id = ((trb.control >> 16) & 0x1F) as u8;

        let slot_id = (trb.control >> 24) as u8;

        crate::serial::write_str("xHCI: transfer event code=");

        crate::serial::write_hex(completion_code as u64);

        crate::serial::write_str(" slot=");

        crate::serial::write_hex(slot_id as u64);

        crate::serial::write_str(" endpoint=");

        crate::serial::write_hex(endpoint_id as u64);

        crate::serial::write_str(" length=");

        crate::serial::write_hex(transfer_length as u64);

        crate::serial::write_str(" ptr=0x");

        crate::serial::write_hex(trb.parameter);

        crate::serial::write_str("\n");
    }

    /*
     * ======================================================================
     * Enable Slot
     * ======================================================================
     *
     * Stellux waits for the Command Completion Event and returns the slot ID.
     * Rusty keeps the same public method name; callers may ignore the return
     * value without changing existing call sites.
     * ======================================================================
     */

    pub unsafe fn enable_slot(&mut self) -> Option<u8> {
        if !self.running {
            panic!("xHCI: enable_slot() called while controller is not running",);
        }

        self.last_command_completion = None;

        let trb = Trb::new(0, 0, (TrbType::EnableSlotCommand as u32) << 10);

        let command_phys = self.command_ring.push(trb);

        crate::serial::write_str("xHCI: Enable Slot command @ 0x");
        crate::serial::write_hex(command_phys);
        crate::serial::write_str("\n");

        dma_sync_for_device(
            self.command_ring.buffer.as_ptr() as *const u8,
            self.command_ring.size * XHCI_TRB_SIZE,
        );
        fence(Ordering::SeqCst);

        self.regs.ring_command();

        let timeout = crate::delay::now_us().saturating_add(1_000_000);

        loop {
            let event = dma_read_current_event(self)?;

            if event.cycle_bit() != self.event_ring.current_cycle() {
                if crate::delay::now_us() >= timeout {
                    crate::serial::write_str("xHCI: Enable Slot command timed out waiting for event\n");
                    return None;
                }

                crate::delay::delay_us(XHCI_POLL_INTERVAL_US);
                continue;
            }

            match event.trb_type() {
                x if x == TrbType::CommandCompletionEvent as u8 => {
                    let completion = ((event.status >> 24) & 0xFF) as u8;
                    let slot_id = ((event.control >> 24) & 0xFF) as u8;
                    let completed_command = event.parameter;

                    crate::serial::write_str("xHCI: Enable Slot completion code=");
                    crate::serial::write_hex(completion as u64);
                    crate::serial::write_str(" slot=");
                    crate::serial::write_hex(slot_id as u64);
                    crate::serial::write_str(" command=0x");
                    crate::serial::write_hex(completed_command);
                    crate::serial::write_str("\n");

                    self.last_command_completion = Some(CommandCompletion {
                        command_trb: completed_command,
                        completion_code: completion,
                        slot_id,
                    });

                    self.event_ring.advance();
                    self.event_dequeue = self.event_ring.current_phys_addr();
                    fence(Ordering::SeqCst);
                    self.regs.set_erdp_clear_busy(0, self.event_dequeue);

                    if completed_command != command_phys {
                        crate::serial::write_str("xHCI: Command Completion does not match Enable Slot TRB\n");
                        if crate::delay::now_us() >= timeout {
                            return None;
                        }
                        continue;
                    }

                    if completion != 1 || slot_id == 0 || slot_id > self.max_slots as u8 {
                        crate::serial::write_str("xHCI: Enable Slot failed\n");
                        return None;
                    }

                    /* Mirror Stellux: immediately create the slot's Device
                     * Context and publish it in DCBAA[slot]. */
                    if !self.create_device_context(slot_id) {
                        return None;
                    }

                    /* Allocate the Input Context now as the next enumeration
                     * object, even though Address Device is implemented by the
                     * higher layer in a later step. */
                    if !self.create_input_context(slot_id) {
                        return None;
                    }

                    crate::serial::write_str("xHCI: Enable Slot succeeded, slot=");
                    crate::serial::write_hex(slot_id as u64);
                    crate::serial::write_str("\n");

                    return Some(slot_id);
                }

                _ => {
                    /* Keep processing asynchronous port/transfer events while
                     * waiting for the command completion, exactly as the IRQ
                     * path would do on hardware. */
                    self.handle_event(event);
                    self.event_ring.advance();
                    self.event_dequeue = self.event_ring.current_phys_addr();
                    fence(Ordering::SeqCst);
                    self.regs.set_erdp_clear_busy(0, self.event_dequeue);
                }
            }

            if crate::delay::now_us() >= timeout {
                crate::serial::write_str("xHCI: Enable Slot command timed out\n");
                return None;
            }
        }
    }

    fn create_device_context(&mut self, slot_id: u8) -> bool {
        let slot = slot_id as usize;

        if slot == 0 || slot >= self.device_context_phys.len() || slot > self.max_slots {
            return false;
        }

        if self.device_context_phys[slot] != 0 {
            /* Already allocated for this slot. */
            return true;
        }

        /* One Slot Context + 31 Endpoint Contexts. */
        let context_bytes = self
            .context_size
            .checked_mul(32)
            .expect("xHCI: device context size overflow");

        let phys = match memory::allocate_dma_region(context_bytes, DCBAA_ALIGNMENT, None) {
            Some(value) => value,
            None => {
                crate::serial::write_str("xHCI: failed to allocate Device Context\n");
                return false;
            }
        };

        let virt = memory::physical_to_virtual(phys) as usize;

        if virt == 0 {
            crate::serial::write_str("xHCI: invalid Device Context virtual address\n");
            return false;
        }

        if (phys & (DCBAA_ALIGNMENT as u64 - 1)) != 0 {
            crate::serial::write_str("xHCI: Device Context is not 64-byte aligned\n");
            return false;
        }

        unsafe {
            write_bytes(virt as *mut u8, 0, context_bytes);
            dma_sync_for_device(virt as *const u8, context_bytes);

            write_volatile(
                (self.dcbaa_virt as *mut u64).add(slot),
                phys,
            );

            dma_sync_for_device(
                self.dcbaa_virt as *const u8,
                (slot + 1) * DCBAA_ENTRY_SIZE,
            );
        }

        fence(Ordering::SeqCst);

        self.device_context_phys[slot] = phys;
        self.device_context_virt[slot] = virt;

        crate::serial::write_str("xHCI: Device Context allocated slot=");
        crate::serial::write_hex(slot_id as u64);
        crate::serial::write_str(" phys=0x");
        crate::serial::write_hex(phys);
        crate::serial::write_str(" size=0x");
        crate::serial::write_hex(context_bytes as u64);
        crate::serial::write_str("\n");

        true
    }

    fn create_input_context(&mut self, slot_id: u8) -> bool {
        let slot = slot_id as usize;

        if slot == 0 || slot >= self.input_context_phys.len() || slot > self.max_slots {
            return false;
        }

        if self.input_context_phys[slot] != 0 {
            return true;
        }

        /* Input Control Context + Slot Context + 31 Endpoint Contexts. */
        let context_bytes = self
            .context_size
            .checked_mul(33)
            .expect("xHCI: input context size overflow");

        let phys = match memory::allocate_dma_region(context_bytes, DCBAA_ALIGNMENT, None) {
            Some(value) => value,
            None => {
                crate::serial::write_str("xHCI: failed to allocate Input Context\n");
                return false;
            }
        };

        let virt = memory::physical_to_virtual(phys) as usize;

        if virt == 0 {
            crate::serial::write_str("xHCI: invalid Input Context virtual address\n");
            return false;
        }

        if (phys & (DCBAA_ALIGNMENT as u64 - 1)) != 0 {
            crate::serial::write_str("xHCI: Input Context is not 64-byte aligned\n");
            return false;
        }

        unsafe {
            write_bytes(virt as *mut u8, 0, context_bytes);
            dma_sync_for_device(virt as *const u8, context_bytes);
        }

        self.input_context_phys[slot] = phys;
        self.input_context_virt[slot] = virt;

        crate::serial::write_str("xHCI: Input Context allocated slot=");
        crate::serial::write_hex(slot_id as u64);
        crate::serial::write_str(" phys=0x");
        crate::serial::write_hex(phys);
        crate::serial::write_str(" size=0x");
        crate::serial::write_hex(context_bytes as u64);
        crate::serial::write_str("\n");

        true
    }

    pub fn last_slot_id(&self) -> Option<u8> {
        self.last_command_completion.and_then(|completion| {
            if completion.completion_code == 1 && completion.slot_id != 0 {
                Some(completion.slot_id)
            } else {
                None
            }
        })
    }

    pub fn device_context_address(&self, slot_id: u8) -> Option<u64> {
        let slot = slot_id as usize;
        if slot == 0 || slot >= self.device_context_phys.len() {
            None
        } else if self.device_context_phys[slot] == 0 {
            None
        } else {
            Some(self.device_context_phys[slot])
        }
    }

    pub fn input_context_address(&self, slot_id: u8) -> Option<u64> {
        let slot = slot_id as usize;
        if slot == 0 || slot >= self.input_context_phys.len() {
            None
        } else if self.input_context_phys[slot] == 0 {
            None
        } else {
            Some(self.input_context_phys[slot])
        }
    }

    /*
     * ======================================================================
     * USB2 root port reset request
     * ======================================================================
     *
     * This is intentionally NON-BLOCKING.  PORT_RESET is written here, then
     * PRC/PR completion is consumed by service_pending_port_reset().
     */

    pub unsafe fn request_usb2_port_reset(&mut self, port: usize) -> bool {
        if port == 0 || port > self.max_ports {
            return false;
        }

        if self.port_protocol_for(port) != UsbPortProtocol::Usb2 {
            crate::serial::write_str("xHCI: refusing USB2 reset on non-USB2 PORT ");
            crate::serial::write_hex(port as u64);
            crate::serial::write_str("\n");
            return false;
        }

        if self.port_reset_pending[port - 1] {
            return true;
        }

        let mut portsc = self.regs.portsc(port);

        /*
         * PRC is RW1C.  Clear an old PRC before starting a new USB2 reset so
         * the completion of THIS reset can create a fresh PRC transition/event.
         */
        if (portsc & PORTSC_PRC) != 0 {
            crate::serial::write_str("xHCI: clearing stale USB2 PRC before reset PORT ");
            crate::serial::write_hex(port as u64);
            crate::serial::write_str("\n");

            self.clear_port_change_bits(port, PORTSC_PRC);
            portsc = self.regs.portsc(port);
        }

        let connected = (portsc & PORTSC_CCS) != 0;
        let enabled = (portsc & PORTSC_PED) != 0;
        let resetting = (portsc & PORTSC_PR) != 0;
        let pls = ((portsc & PORTSC_PLS_MASK) >> 5) as u8;

        /*
         * A normal USB2 root-port reset requires a connected, disabled port
         * that is not already being reset.  Do not impose PLS==Polling here:
         * xHCI hardware can legitimately report another transient pre-reset
         * state while still accepting the USB2 reset request.
         */
        if !connected || enabled || resetting {
            return false;
        }

        let write_value = Self::portsc_to_neutral(portsc) | PORTSC_PR;

        crate::serial::write_str("xHCI: requesting USB2 PORT_RESET PORT ");
        crate::serial::write_hex(port as u64);
        crate::serial::write_str(" PLS=");
        crate::serial::write_hex(pls as u64);
        crate::serial::write_str(" PORTSC=0x");
        crate::serial::write_hex(write_value as u64);
        crate::serial::write_str("\n");

        /*
         * Make the command visible before the following MMIO read and before
         * the controller can be expected to act on it.
         */
        /*
         * Mark the operation pending BEFORE the intentional 60 ms settling
         * delay.  A fast controller/device must never be observed as a stale
         * PRC event by a re-entrant event-polling path.
         */
        self.port_reset_pending[port - 1] = true;
        self.port_reset_deadline_us[port - 1] =
            crate::delay::now_us().saturating_add(USB2_PORT_RESET_TIMEOUT_US);

        fence(Ordering::SeqCst);
        self.regs.set_portsc(port, write_value);
        let _ = self.regs.portsc(port);

        true
    }

    /*
     * Compatibility entry point.  Historically this method synchronously
     * waited for PORT_RESET.  It is now deliberately non-blocking so callers
     * cannot stall the xHCI event-consumer path.
     */
    pub unsafe fn reset_port(&mut self, port: usize) {
        let _ = self.request_usb2_port_reset(port);
    }

    /*
     * ======================================================================
     * USB3 warm reset request
     * ======================================================================
     *
     * USB3 initial attach does not use the normal USB2 PORT_RESET path.
     * Warm reset uses PORT_WR and completes with PORT_WRC.
     */

    pub unsafe fn warm_reset_usb3_port(&mut self, port: usize) -> bool {
        if port == 0 || port > self.max_ports {
            return false;
        }

        if self.port_protocol_for(port) != UsbPortProtocol::Usb3 {
            return false;
        }

        if self.usb3_warm_reset_pending[port - 1] {
            return true;
        }

        let mut portsc = self.regs.portsc(port);
        let pls = ((portsc & PORTSC_PLS_MASK) >> 5) as u8;

        /*
         * Allow warm reset for CAS, a connected USB3 device, or the specific
         * SuperSpeed Inactive/Compliance recovery states.  CCS may temporarily
         * be clear in the latter cases.
         */
        if (portsc & PORTSC_CCS) == 0
            && (portsc & PORTSC_CAS) == 0
            && pls != PORTSC_PLS_INACTIVE
            && pls != PORTSC_PLS_COMPLIANCE
        {
            return false;
        }

        if (portsc & PORTSC_WR) != 0 {
            return false;
        }

        /*
         * WRC and PRC are RW1C change bits.  Clear stale completion indications
         * before requesting a new warm reset so its completion can be observed
         * as a fresh change condition.
         */
        let stale_changes = portsc & (PORTSC_WRC | PORTSC_PRC);
        if stale_changes != 0 {
            crate::serial::write_str(
                "xHCI: clearing stale USB3 reset change bits PORT ",
            );
            crate::serial::write_hex(port as u64);
            crate::serial::write_str("\n");

            self.clear_port_change_bits(port, stale_changes);
            portsc = self.regs.portsc(port);
        }

        let write_value = Self::portsc_to_neutral(portsc) | PORTSC_WR;

        crate::serial::write_str("xHCI: requesting USB3 warm reset PORT ");
        crate::serial::write_hex(port as u64);
        crate::serial::write_str(" PORTSC=0x");
        crate::serial::write_hex(write_value as u64);
        crate::serial::write_str("\n");

        /*
         * Mark the operation pending before the warm-reset settling delay so
         * a very fast completion cannot be misclassified as stale WRC/PRC.
         */
        self.usb3_warm_reset_pending[port - 1] = true;
        self.usb3_warm_reset_deadline_us[port - 1] =
            crate::delay::now_us().saturating_add(USB3_WARM_RESET_TIMEOUT_US);

        fence(Ordering::SeqCst);
        self.regs.set_portsc(port, write_value);
        let _ = self.regs.portsc(port);

        /*
         * Linux's SuperSpeed recovery path uses a conservative 50 ms interval
         * before checking the result of a warm reset.
         */
        crate::delay::delay_us(USB3_WARM_RESET_DELAY_US);

        true
    }

    /*
     * ======================================================================
     * Clear Port Reset Change
     * ======================================================================
     */
    pub unsafe fn clear_port_reset_change(&self, port: usize) {
        if port == 0 || port > self.max_ports {
            return;
        }

        let portsc = self.regs.portsc(port);

        if (portsc & PORTSC_PRC) == 0 {
            return;
        }

        self.clear_port_change_bits(port, PORTSC_PRC);
    }

    /*
     * ======================================================================
     * Convenience accessors
     * ======================================================================
     */

    pub fn max_port_count(&self) -> usize {
        self.max_ports
    }

    pub fn port_status(&self, port: usize) -> u32 {
        self.regs.portsc(port)
    }

    pub fn port_connected(&self, port: usize) -> bool {
        self.regs.port_connected(port)
    }

    pub fn port_enabled(&self, port: usize) -> bool {
        self.regs.port_enabled(port)
    }

    pub fn port_speed(&self, port: usize) -> u8 {
        self.regs.port_speed(port)
    }

    pub fn usb2_ports_count(&self) -> usize {
        self.usb2_port_count
    }

    pub fn usb3_ports_count(&self) -> usize {
        self.usb3_port_count
    }

    pub fn is_usb2_port(&self, port: usize) -> bool {
        self.port_protocol_for(port) == UsbPortProtocol::Usb2
    }

    pub fn is_usb3_port(&self, port: usize) -> bool {
        self.port_protocol_for(port) == UsbPortProtocol::Usb3
    }

    pub fn port_reset_pending(&self, port: usize) -> bool {
        if port == 0 || port > self.max_ports {
            false
        } else {
            self.port_reset_pending[port - 1]
        }
    }

    pub fn usb3_warm_reset_pending(&self, port: usize) -> bool {
        if port == 0 || port > self.max_ports {
            false
        } else {
            self.usb3_warm_reset_pending[port - 1]
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub fn is_running(&self) -> bool {
        self.running
    }
}

/*
 * Read and cache-sync the current event-ring TRB.
 */
#[inline(always)]
unsafe fn dma_read_current_event(driver: &XhciDriver) -> Option<Trb> {
    if driver.event_ring.trbs == 0 {
        return None;
    }

    let index = driver.event_ring.index;

    dma_sync_for_cpu(
        driver.event_ring.buffer.as_ptr().add(index) as *const u8,
        XHCI_TRB_SIZE,
    );

    Some(driver.event_ring.read_current())
}

/*
 * ==========================================================================
 * Volatile-memory helpers
 * ==========================================================================
 */

#[inline(always)]
unsafe fn write_bytes(destination: *mut u8, value: u8, count: usize) {
    core::ptr::write_bytes(destination, value, count);
}

#[inline(always)]
unsafe fn write_volatile<T>(destination: *mut T, value: T) {
    core::ptr::write_volatile(destination, value);
}

#[inline(always)]
unsafe fn read_volatile<T: Copy>(source: *const T) -> T {
    core::ptr::read_volatile(source)
}

/*
 * ==========================================================================
 * Conservative DMA cache synchronization
 * ==========================================================================
 *
 * PCIe/x86 systems are normally cache coherent. These helpers are therefore a
 * conservative visibility boundary, not a claim that ordinary coherent x86
 * PCIe DMA requires software cache maintenance for every transaction.
 *
 * CPU -> controller:
 *   flush dirty cache lines before the controller consumes DMA memory.
 *
 * Controller -> CPU:
 *   flush/invalidate the relevant lines before the CPU consumes controller-
 *   written data.
 *
 * CLFLUSH is used instead of clflushopt so this remains broadly available on
 * x86-64 without a CPU-feature-specific code path.
 */
const CACHE_LINE_SIZE: usize = 64;

#[inline(always)]
unsafe fn dma_sync_for_device(address: *const u8, size: usize) {
    if size == 0 {
        fence(Ordering::SeqCst);
        return;
    }

    let start = (address as usize) & !(CACHE_LINE_SIZE - 1);

    let last = (address as usize)
        .checked_add(size - 1)
        .expect("xHCI: DMA cache-sync address overflow");

    let end = last & !(CACHE_LINE_SIZE - 1);

    let mut current = start;

    loop {
        core::arch::asm!(
            "clflush [{addr}]",
            addr = in(reg) current,
            options(nostack, preserves_flags),
        );

        if current == end {
            break;
        }

        current = current
            .checked_add(CACHE_LINE_SIZE)
            .expect("xHCI: DMA cache-sync iteration overflow");
    }

    core::arch::asm!(
        "mfence",
        options(nostack, preserves_flags),
    );

    fence(Ordering::SeqCst);
}

#[inline(always)]
unsafe fn dma_sync_for_cpu(address: *const u8, size: usize) {
    /*
     * On coherent x86, a device write is normally snooped by the cache
     * hierarchy. Flushing the line here is still a conservative way to ensure
     * an old CPU-resident copy cannot be consumed by the driver.
     */
    dma_sync_for_device(address, size);
}
