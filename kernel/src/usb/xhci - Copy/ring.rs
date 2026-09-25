use crate::memory;
use crate::usb::xhci::trb::{CONTROL_CHAIN, CONTROL_CYCLE, LINK_TOGGLE_CYCLE, Trb, TrbType};

use core::alloc::Layout;
use core::ptr::{NonNull, read_volatile, write_volatile};
use core::sync::atomic::{Ordering, fence};

/*
 * ==========================================================================
 * xHCI ring constants
 * ==========================================================================
 *
 * Linux:
 *
 *     #define TRBS_PER_SEGMENT 256
 *
 * Every TRB is 16 bytes, so one complete segment is:
 *
 *     256 * 16 = 4096 bytes
 *
 * The final TRB in a Command/Transfer ring segment is the Link TRB.
 *
 * Therefore:
 *
 *     255 usable TRBs
 *     1 Link TRB
 *
 * Event rings are different and do NOT use this Ring type.
 *
 * ==========================================================================*/

const TRB_SIZE: usize = core::mem::size_of::<Trb>();

pub const TRBS_PER_SEGMENT: usize = 256;

pub const TRB_SEGMENT_SIZE: usize = TRBS_PER_SEGMENT * TRB_SIZE;

/*
 * Linux's DMA segment allocation provides a naturally aligned segment.
 *
 * Using the complete segment size as the alignment gives us:
 *
 *     4096-byte aligned segment
 */
const RING_ALIGNMENT: usize = TRB_SEGMENT_SIZE;

/*
 * ==========================================================================
 * Producer Ring
 * ==========================================================================
 *
 * This type models a single Linux xHCI command/transfer ring segment:
 *
 *     TRB 0
 *     TRB 1
 *     ...
 *     TRB 254
 *     TRB 255 -> Link TRB -> TRB 0
 *
 * Linux actually supports multiple segments. This hobby-OS implementation
 * currently uses one circular segment, but preserves Linux's enqueue/link
 * semantics so it can be extended later.
 *
 * ==========================================================================*/

pub struct Ring {
    /*
     * Virtual address of TRB 0.
     */
    pub buffer: NonNull<Trb>,

    /*
     * Total number of TRBs in this segment.
     *
     * Includes the Link TRB.
     */
    pub size: usize,

    /*
     * Linux ring cycle_state equivalent.
     *
     * New rings start with PCS = 1.
     */
    pub cycle: bool,

    /*
     * Linux ring enqueue pointer equivalent.
     *
     * Range:
     *
     *     0 ..= capacity()
     *
     * `index == capacity()` means the enqueue pointer is currently on
     * the Link TRB.
     */
    pub index: usize,

    /*
     * Physical/DMA address of TRB 0.
     */
    pub phys_addr: u64,
}

impl Ring {
    /*
     * ======================================================================
     * Constructor
     * ======================================================================
     *
     * Linux allocates segments with TRBS_PER_SEGMENT TRBs.
     *
     * This implementation intentionally requires exactly one such segment.
     * ======================================================================
     */

    pub fn new(size: usize) -> Self {
        if size != TRBS_PER_SEGMENT {
            panic!("xHCI: ring segment must contain exactly 256 TRBs",);
        }

        let layout = Layout::array::<Trb>(size).expect("xHCI: invalid ring layout");

        if layout.size() != TRB_SEGMENT_SIZE {
            panic!("xHCI: ring segment size mismatch",);
        }

        /*
         * Allocate one complete DMA segment.
         */
        let phys_addr = memory::allocate_dma_region(layout.size(), RING_ALIGNMENT, None)
            .expect("xHCI: failed to allocate DMA for xHCI ring");

        /*
         * Linux's segment DMA address must be correctly aligned.
         */
        if (phys_addr & (RING_ALIGNMENT as u64 - 1)) != 0 {
            panic!("xHCI: ring segment is not 4096-byte aligned",);
        }

        /*
         * Convert physical DMA address to CPU virtual address.
         */
        let virt_addr = memory::physical_to_virtual(phys_addr);

        let buffer =
            NonNull::new(virt_addr as *mut Trb).expect("xHCI: invalid ring virtual address");

        /*
         * Linux's dma_pool_zalloc() returns zeroed memory.
         *
         * Section 4.11.1.1 requires all Command/Transfer TRBs to start
         * initialized to zero.
         */
        unsafe {
            core::ptr::write_bytes(buffer.as_ptr() as *mut u8, 0, layout.size());
        }

        /*
         * Linux's xhci_initialize_ring_info():
         *
         *     ring->cycle_state = 1;
         */
        let mut ring = Self {
            buffer,
            size,

            cycle: true,

            /*
             * The enqueue pointer starts at the first TRB.
             */
            index: 0,

            phys_addr,
        };

        /*
         * Linux's xhci_initialize_ring_segments():
         *
         * For a command/transfer ring, initialize the final TRB as a Link
         * TRB and set Toggle Cycle on the last segment's Link TRB.
         *
         * For our single-segment ring, the Link TRB points back to TRB 0.
         */
        ring.initialize_link_trb();

        /*
         * Ensure ring initialization is visible before the controller can
         * be started / doorbelled.
         */
        fence(Ordering::SeqCst);

        ring
    }

    /*
     * ======================================================================
     * Number of usable TRBs
     * ======================================================================
     *
     * Linux:
     *
     *     TRBS_PER_SEGMENT - 1
     *
     * because the last TRB is the Link TRB.
     * ======================================================================
     */

    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        self.size - 1
    }

    /*
     * ======================================================================
     * Current enqueue physical address
     * ======================================================================
     *
     * If enqueue == Link TRB, this returns the Link TRB address.
     *
     * This is useful internally for modeling Linux's enqueue pointer.
     * ======================================================================
     */

    #[inline(always)]
    pub fn current_phys_addr(&self) -> u64 {
        if self.index > self.capacity() {
            panic!("xHCI: invalid ring enqueue index",);
        }

        let offset = self
            .index
            .checked_mul(TRB_SIZE)
            .expect("xHCI: ring address overflow");

        self.phys_addr
            .checked_add(offset as u64)
            .expect("xHCI: ring physical address overflow")
    }

    /*
     * ======================================================================
     * Previous usable TRB physical address
     * ======================================================================
     */

    #[inline(always)]
    pub fn previous_phys_addr(&self) -> u64 {
        let index = if self.index == 0 || self.index == self.capacity() {
            self.capacity() - 1
        } else {
            self.index - 1
        };

        let offset = index
            .checked_mul(TRB_SIZE)
            .expect("xHCI: previous TRB offset overflow");

        self.phys_addr
            .checked_add(offset as u64)
            .expect("xHCI: previous TRB address overflow")
    }

    /*
     * ======================================================================
     * Push one TRB
     * ======================================================================
     *
     * This now follows Linux's important enqueue behavior.
     *
     * Linux's inc_enq():
     *
     *     enqueue++;
     *
     * If enqueue lands on a Link TRB, Linux only traverses that Link TRB
     * immediately when the current operation requires it.
     *
     * For this simplified API, every push represents an independent TRB
     * submission, so if the enqueue pointer is already on the Link TRB,
     * we first call inc_enq_past_link().
     *
     * The key difference from the previous implementation:
     *
     *     OLD:
     *         push TRB 254
     *         immediately cross Link
     *
     *     NEW:
     *         push TRB 254
     *         enqueue = Link TRB
     *
     *         next push:
     *             cross Link
     *             toggle Link cycle
     *             toggle ring cycle
     *             enqueue = TRB 0
     *
     * This matches Linux's enqueue-pointer semantics much more closely.
     * ======================================================================
     */

    pub fn push(&mut self, mut trb: Trb) -> u64 {
        /*
         * If enqueue currently points at the Link TRB, Linux traverses the
         * Link before writing the next ordinary TRB.
         */
        if self.index == self.capacity() {
            /*
             * Preserve the preceding TRB's Chain state when traversing the
             * Link TRB. A TD can cross a segment boundary.
             */
            let chain = unsafe {
                read_volatile(self.buffer.as_ptr().add(self.capacity() - 1))
            }
            .chain();

            self.inc_enq_past_link(chain);
        }

        /*
         * We must now point at an ordinary TRB.
         */
        if self.index >= self.capacity() {
            panic!("xHCI: failed to advance past Link TRB",);
        }

        let submitted_phys = self.current_phys_addr();

        /*
         * Hardware sees the TRB only with the current Producer Cycle State.
         */
        trb.set_cycle(self.cycle);

        /*
         * Write the TRB to DMA memory.
         */
        unsafe {
            write_volatile(self.buffer.as_ptr().add(self.index), trb);
        }

        /*
         * Equivalent ordering requirement to Linux's wmb() before hardware
         * is allowed to consume the newly written TRB.
         */
        fence(Ordering::Release);

        /*
         * Advance enqueue pointer exactly once.
         *
         * We intentionally do NOT automatically cross the Link TRB here.
         *
         * This is the important Linux behavior.
         */
        self.index = self
            .index
            .checked_add(1)
            .expect("xHCI: enqueue index overflow");

        /*
         * At this point index may equal capacity(), meaning the enqueue
         * pointer is sitting on the Link TRB.
         *
         * That is valid Linux ring state.
         */
        submitted_phys
    }

    /*
     * ======================================================================
     * Move enqueue past Link TRB
     * ======================================================================
     *
     * This corresponds to Linux's:
     *
     *     inc_enq_past_link()
     *
     * Linux:
     *
     *     - modifies chain bit if required
     *     - wmb()
     *     - toggles Link TRB cycle bit
     *     - toggles ring cycle state if Link has Toggle Cycle
     *     - moves enqueue to the next segment
     *
     * Our ring has one segment, so "next segment" is TRB 0.
     * ======================================================================
     */

    fn inc_enq_past_link(&mut self, chain: bool) {
        if self.index != self.capacity() {
            return;
        }

        let link_index = self.capacity();

        let link_ptr = unsafe { self.buffer.as_ptr().add(link_index) };

        /*
         * Read the existing Link TRB.
         */
        let mut link = unsafe { read_volatile(link_ptr) };

        /*
         * Linux normally keeps or modifies the Link TRB's Chain field
         * depending on ring/quirk state.
         *
         * We have no xHCI Link-chain quirk layer yet, so use the caller's
         * requested chain state directly.
         */
        link.control &= !CONTROL_CHAIN;

        if chain {
            link.control |= CONTROL_CHAIN;
        }

        /*
         * Make all preceding TRB writes visible before handing the Link TRB
         * to the controller.
         */
        fence(Ordering::Release);

        /*
         * Linux:
         *
         *     ring->enqueue->link.control ^=
         *         cpu_to_le32(TRB_CYCLE);
         */
        link.control ^= CONTROL_CYCLE;

        /*
         * Write the updated Link TRB back to DMA memory.
         */
        unsafe {
            write_volatile(link_ptr, link);
        }

        /*
         * Ensure the modified Link TRB is visible before changing software
         * cycle state / enqueue position.
         */
        fence(Ordering::Release);

        /*
         * Linux:
         *
         *     if (link_trb_toggles_cycle(...))
         *         ring->cycle_state ^= 1;
         *
         * Our Link TRB always carries Toggle Cycle.
         */
        if (link.control & LINK_TOGGLE_CYCLE) != 0 {
            self.cycle = !self.cycle;
        }

        /*
         * Single segment -> next segment is our own segment.
         */
        self.index = 0;
    }

    /*
     * ======================================================================
     * Initialize Link TRB
     * ======================================================================
     *
     * Equivalent to Linux:
     *
     *     xhci_set_link_trb()
     *     ...
     *     last_seg->trbs[TRBS_PER_SEGMENT - 1].link.control |=
     *         LINK_TOGGLE;
     *
     * ======================================================================
     */

    fn initialize_link_trb(&mut self) {
        let link_index = self.capacity();

        /*
         * Single-segment ring points to itself.
         */
        let mut link = Trb::new(self.phys_addr, 0, TrbType::Link.control_bits());

        /*
         * Linux sets Toggle Cycle on the LAST segment's Link TRB.
         *
         * It does NOT set the Link TRB's Cycle bit during initialization.
         */
        link.control |= LINK_TOGGLE_CYCLE;

        unsafe {
            write_volatile(self.buffer.as_ptr().add(link_index), link);
        }
    }

    /*
     * ======================================================================
     * Producer cycle state
     * ======================================================================
     */

    #[inline(always)]
    pub const fn cycle(&self) -> bool {
        self.cycle
    }

    /*
     * ======================================================================
     * Enqueue index
     * ======================================================================
     */

    #[inline(always)]
    pub const fn index(&self) -> usize {
        self.index
    }

    /*
     * ======================================================================
     * Whether enqueue currently points at the Link TRB
     * ======================================================================
     */

    #[inline(always)]
    pub const fn on_link(&self) -> bool {
        self.index == self.capacity()
    }

    /*
     * ======================================================================
     * Reset ring
     * ======================================================================
     *
     * Matches Linux's xhci_clear_command_ring() / ring reinitialization
     * behavior for the single-segment case:
     *
     *     clear ordinary TRBs
     *     clear Link Cycle bit
     *     initialize ring cycle state = 1
     *     enqueue = first TRB
     *
     * ======================================================================
     */

    pub fn reset(&mut self) {
        let byte_count = self
            .size
            .checked_mul(TRB_SIZE)
            .expect("xHCI: ring reset size overflow");

        unsafe {
            /*
             * Clear all TRBs, including the Link TRB.
             */
            core::ptr::write_bytes(self.buffer.as_ptr() as *mut u8, 0, byte_count);
        }

        /*
         * Linux ring initialization:
         *
         *     ring->cycle_state = 1;
         */
        self.cycle = true;

        /*
         * Enqueue starts at the first TRB.
         */
        self.index = 0;

        /*
         * Restore the Link TRB.
         *
         * Link Cycle remains 0 initially.
         * Toggle Cycle remains set.
         */
        self.initialize_link_trb();

        fence(Ordering::SeqCst);
    }

    /*
     * ======================================================================
     * Read TRB
     * ======================================================================
     */

    #[inline(always)]
    pub unsafe fn read(&self, index: usize) -> Trb {
        if index >= self.size {
            panic!("xHCI: ring TRB index out of bounds",);
        }

        read_volatile(self.buffer.as_ptr().add(index))
    }

    /*
     * ======================================================================
     * Write TRB
     * ======================================================================
     */

    #[inline(always)]
    pub unsafe fn write(&self, index: usize, trb: Trb) {
        if index >= self.size {
            panic!("xHCI: ring TRB index out of bounds",);
        }

        write_volatile(self.buffer.as_ptr().add(index), trb);
    }

    /*
     * ======================================================================
     * Link TRB access
     * ======================================================================
     */

    #[inline(always)]
    pub unsafe fn link_trb(&self) -> Trb {
        self.read(self.capacity())
    }
}

/*
 * ==========================================================================
 * Compile-time layout checks
 * ==========================================================================
 */

const _: () = {
    assert!(core::mem::size_of::<Trb>() == 16);

    assert!(core::mem::align_of::<Trb>() == 16);

    assert!(TRB_SEGMENT_SIZE == 4096);
};
