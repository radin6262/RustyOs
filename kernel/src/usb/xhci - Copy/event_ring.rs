use crate::memory;
use crate::usb::xhci::trb::Trb;

use core::alloc::Layout;
use core::ptr::{NonNull, read_volatile, write_bytes};
use core::sync::atomic::{Ordering, fence};

/*
 * ==========================================================================
 * Constants
 * ==========================================================================
 */

const TRB_SIZE: usize = core::mem::size_of::<Trb>();

/*
 * xHCI ERST segment size field is 16 bits.
 *
 * The controller stores the number of TRBs in the segment in a 16-bit
 * field of the ERST entry.
 */
const MAX_EVENT_RING_TRBS: usize = u16::MAX as usize;

/*
 * Event ring segment bases are required to be aligned for xHCI DMA.
 *
 * The ERST table itself is also allocated with 64-byte alignment by the
 * driver.
 */
const EVENT_RING_ALIGNMENT: usize = 64;

const ERST_ALIGNMENT: usize = 64;

/*
 * ==========================================================================
 * Event Ring Segment Table entry
 * ==========================================================================
 *
 * xHCI ERST entry layout:
 *
 *     +0x00  Segment Base Address   u64
 *     +0x08  Segment Size           u16
 *     +0x0A  Reserved               u16
 *     +0x0C  Reserved               u32
 *
 * Total size = 16 bytes.
 *
 * IMPORTANT:
 *
 * Do NOT use #[repr(C, align(64))] here.
 *
 * An individual ERST entry is 16 bytes.
 * The ERST table base is what must satisfy the table alignment requirement.
 * ==========================================================================*/

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ErstEntry {
    pub ring_segment_base_address: u64,
    pub ring_segment_size: u16,
    pub reserved1: u16,
    pub reserved2: u32,
}

impl ErstEntry {
    #[inline(always)]
    pub const fn new(phys_addr: u64, trb_count: u16) -> Self {
        Self {
            ring_segment_base_address: phys_addr,

            ring_segment_size: trb_count,

            reserved1: 0,

            reserved2: 0,
        }
    }
}

/*
 * ==========================================================================
 * Compile-time ERST layout check
 * ==========================================================================
 */

const _: () = {
    assert!(core::mem::size_of::<ErstEntry>() == 16);

    assert!(core::mem::align_of::<ErstEntry>() == 8);
};

/*
 * ==========================================================================
 * Event Ring
 * ==========================================================================
 *
 * The xHCI Event Ring is different from a normal transfer/command ring:
 *
 *     - It does NOT contain a Link TRB.
 *     - Hardware owns the TRBs through the Cycle bit.
 *     - Software advances its dequeue pointer.
 *     - On wrap, software toggles its expected cycle state.
 *
 * `trbs` is the REAL number of event TRBs in the segment.
 *
 * There is no hidden "dummy/link" TRB.
 * ==========================================================================*/

pub struct EventRing {
    /*
     * CPU virtual address of the event-ring segment.
     */
    pub buffer: NonNull<Trb>,

    /*
     * Actual number of TRBs in this event-ring segment.
     */
    pub trbs: usize,

    /*
     * Consumer cycle state.
     *
     * Initially true.
     */
    pub cycle: bool,

    /*
     * Current software dequeue index.
     */
    pub index: usize,

    /*
     * Physical/DMA address of TRB index 0.
     */
    pub phys_addr: u64,
}

impl EventRing {
    /*
     * ======================================================================
     * Constructor
     * ======================================================================
     */

    pub unsafe fn new(trbs: usize) -> Self {
        if trbs == 0 {
            panic!("xHCI: event ring cannot contain zero TRBs",);
        }

        /*
         * ERST segment size is a 16-bit field.
         */
        if trbs > MAX_EVENT_RING_TRBS {
            panic!("xHCI: event ring segment exceeds 65535 TRBs",);
        }

        /*
         * Every TRB is exactly 16 bytes.
         */
        let size = trbs
            .checked_mul(TRB_SIZE)
            .expect("xHCI: event ring size overflow");

        let layout = Layout::array::<Trb>(trbs).expect("xHCI: invalid event ring layout");

        /*
         * The requested DMA alignment is 64 bytes.
         *
         * This is also useful for making the segment base naturally
         * compatible with xHCI's ring alignment requirements.
         */
        if layout.size() != size {
            panic!("xHCI: event ring layout size mismatch",);
        }

        let phys_addr = memory::allocate_dma_region(size, EVENT_RING_ALIGNMENT, None)
            .expect("xHCI: failed to allocate event ring");

        /*
         * Validate physical alignment.
         */
        if (phys_addr & (EVENT_RING_ALIGNMENT as u64 - 1)) != 0 {
            panic!("xHCI: event ring physical address is not 64-byte aligned",);
        }

        let virt_addr = memory::physical_to_virtual(phys_addr);

        let buffer =
            NonNull::new(virt_addr as *mut Trb).expect("xHCI: invalid event ring virtual address");

        /*
         * Start with every TRB completely zeroed.
         *
         * Cycle bit = 0 means hardware has not yet produced an event.
         *
         * Software's initial PCS = 1.
         */
        write_bytes(buffer.as_ptr() as *mut u8, 0, size);

        /*
         * Ensure the initialization stores become visible before any
         * future interaction with the controller.
         */
        fence(Ordering::SeqCst);

        Self {
            buffer,
            trbs,

            /*
             * xHCI event rings start with consumer cycle state = 1.
             */
            cycle: true,

            /*
             * Start at TRB zero.
             */
            index: 0,

            phys_addr,
        }
    }

    /*
     * ======================================================================
     * Capacity
     * ======================================================================
     *
     * Unlike a command/transfer Ring abstraction, there is NO Link TRB
     * occupying one slot.
     *
     * Therefore:
     *
     *     capacity() == trbs
     * ======================================================================
     */

    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        self.trbs
    }

    /*
     * ======================================================================
     * Current TRB physical address
     * ======================================================================
     */

    #[inline(always)]
    pub fn current_phys_addr(&self) -> u64 {
        let offset = self
            .index
            .checked_mul(TRB_SIZE)
            .expect("xHCI: event-ring index overflow");

        self.phys_addr
            .checked_add(offset as u64)
            .expect("xHCI: event-ring physical address overflow")
    }

    /*
     * ======================================================================
     * Current cycle state
     * ======================================================================
     */

    #[inline(always)]
    pub const fn current_cycle(&self) -> bool {
        self.cycle
    }

    /*
     * ======================================================================
     * Bounds validation
     * ======================================================================
     */

    #[inline(always)]
    fn validate_index(&self) {
        if self.index >= self.trbs {
            panic!("xHCI: event ring index out of bounds",);
        }
    }

    /*
     * ======================================================================
     * Read one TRB from hardware-owned memory
     * ======================================================================
     *
     * IMPORTANT:
     *
     * We perform the volatile read first and then an Acquire fence.
     *
     * That makes the DMA-produced TRB contents visible before software
     * interprets the event.
     *
     * ======================================================================
     */

    #[inline(always)]
    unsafe fn load_current(&self) -> Trb {
        self.validate_index();

        let trb_ptr = self.buffer.as_ptr().add(self.index);

        let trb = read_volatile(trb_ptr);

        fence(Ordering::Acquire);

        trb
    }

    /*
     * ======================================================================
     * Has event
     * ======================================================================
     *
     * Hardware writes the Cycle bit when it produces an event.
     *
     * An event belongs to software when:
     *
     *     TRB.Cycle == software_cycle
     * ======================================================================
     */

    #[inline]
    pub fn has_event(&self) -> bool {
        let trb = unsafe { self.load_current() };

        trb.cycle() == self.cycle
    }

    /*
     * ======================================================================
     * Peek
     * ======================================================================
     *
     * Reads the current event without advancing.
     * ======================================================================
     */

    pub fn peek(&self) -> Option<Trb> {
        let trb = unsafe { self.load_current() };

        if trb.cycle() != self.cycle {
            return None;
        }

        Some(trb)
    }

    /*
     * ======================================================================
     * Pop
     * ======================================================================
     *
     * Reads the current event and advances the software dequeue pointer.
     * ======================================================================
     */

    pub fn pop(&mut self) -> Option<Trb> {
        let trb = unsafe { self.load_current() };

        if trb.cycle() != self.cycle {
            return None;
        }

        self.advance();

        Some(trb)
    }

    /*
     * ======================================================================
     * Unconditional read
     * ======================================================================
     *
     * Existing xHCI code uses this directly while polling.
     * ======================================================================
     */

    #[inline]
    pub unsafe fn read_current(&self) -> Trb {
        self.load_current()
    }

    /*
     * ======================================================================
     * Advance
     * ======================================================================
     *
     * Advance one TRB.
     *
     * When the segment wraps:
     *
     *     index = 0
     *     cycle = !cycle
     *
     * This is the Event Ring Producer/Consumer cycle-state mechanism.
     * ======================================================================
     */

    #[inline]
    pub fn advance(&mut self) {
        self.validate_index();

        self.index = self
            .index
            .checked_add(1)
            .expect("xHCI: event ring index overflow");

        if self.index >= self.trbs {
            self.index = 0;

            self.cycle = !self.cycle;
        }
    }

    /*
     * ======================================================================
     * Reset
     * ======================================================================
     *
     * Clears the entire event ring.
     *
     * Because there is no Link TRB, every TRB belongs to the event ring.
     * ======================================================================
     */

    pub unsafe fn reset(&mut self) {
        let size = self
            .trbs
            .checked_mul(TRB_SIZE)
            .expect("xHCI: event ring reset size overflow");

        write_bytes(self.buffer.as_ptr() as *mut u8, 0, size);

        /*
         * Make sure zeroed event state is visible before resetting the
         * software pointers.
         */
        fence(Ordering::SeqCst);

        self.index = 0;

        self.cycle = true;
    }

    /*
     * ======================================================================
     * Physical address of arbitrary TRB
     * ======================================================================
     */

    #[inline(always)]
    pub fn trb_phys_addr(&self, index: usize) -> u64 {
        if index >= self.trbs {
            panic!("xHCI: event ring TRB index out of bounds",);
        }

        let offset = index
            .checked_mul(TRB_SIZE)
            .expect("xHCI: event ring TRB offset overflow");

        self.phys_addr
            .checked_add(offset as u64)
            .expect("xHCI: event ring TRB physical address overflow")
    }

    /*
     * ======================================================================
     * Current virtual TRB
     * ======================================================================
     */

    #[inline(always)]
    pub fn current_trb_ptr(&self) -> *mut Trb {
        self.validate_index();

        unsafe { self.buffer.as_ptr().add(self.index) }
    }
}
