// ============================================================
// xHCI command, transfer and event rings
// ============================================================
//
// Command and transfer rings are producer rings: software writes TRBs and
// the controller consumes them. Each ring reserves its last entry for a Link
// TRB so the controller wraps back to the beginning and toggles its Producer
// Cycle State (PCS).
//
// The event ring is a consumer ring: the controller writes events using its
// own cycle state and software walks entries while the cycle bit matches the
// Consumer Cycle State (CCS).
//
// Reference: xHCI 1.1 sections 4.9.1 (command ring), 4.9.2 (event ring) and
// 6.4.6 (Link TRB).
// ============================================================

use core::sync::atomic::{fence, Ordering};

use super::trb::Trb;
use crate::usb::dma::DmaBuffer;

/// Entries per ring, including the Link TRB.
pub const TRBS_PER_RING: usize = 64;

const TRB_SIZE: usize = 16;

/// A software-produced TRB ring (command ring, endpoint transfer rings).
pub struct TrbRing {
    buffer: DmaBuffer,
    enqueue: usize,
    cycle: bool,
}

impl TrbRing {
    pub fn new() -> Option<Self> {
        let buffer = DmaBuffer::new(TRBS_PER_RING * TRB_SIZE, 64)?;

        let mut ring = Self {
            buffer,
            enqueue: 0,
            cycle: true,
        };

        // Seed the Link TRB. The controller stops at entry 0 until software
        // publishes a TRB with the correct cycle bit, so an initially empty
        // ring never executes stale data.
        ring.write_link(TRBS_PER_RING - 1);

        Some(ring)
    }

    pub fn physical(&self) -> u64 {
        self.buffer.physical()
    }

    /// Publish one TRB and return its physical address.
    ///
    /// The caller must ring the appropriate doorbell afterwards.
    pub fn enqueue(&mut self, mut trb: Trb) -> u64 {
        // Reserve the final entry for the wrap-around Link TRB.
        if self.enqueue == TRBS_PER_RING - 1 {
            self.write_link(self.enqueue);
            self.cycle = !self.cycle;
            self.enqueue = 0;
        }

        let index = self.enqueue;

        trb.control = (trb.control & !1) | self.cycle as u32;

        let address = self.buffer.physical() + (index * TRB_SIZE) as u64;

        unsafe {
            core::ptr::write_volatile(address as *mut Trb, trb);
        }

        self.enqueue += 1;

        // Make the TRB visible to the controller before the doorbell write.
        fence(Ordering::SeqCst);

        address
    }

    fn write_link(&mut self, index: usize) {
        let link = Trb::link(
            self.buffer.physical(),
            true,
            self.cycle,
        );

        let address = self.buffer.physical() + (index * TRB_SIZE) as u64;

        unsafe {
            core::ptr::write_volatile(address as *mut Trb, link);
        }

        fence(Ordering::SeqCst);
    }

    /// Number of entries already published but not wrapped yet.
    pub fn enqueued(&self) -> usize {
        self.enqueue
    }

    /// Rewind the ring after the controller reset its endpoint dequeue
    /// pointer (Address Device, Reset Device, slot reuse).
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.enqueue = 0;
        self.cycle = true;
        self.write_link(TRBS_PER_RING - 1);
    }
}

/// The single event ring segment used by the driver.
pub struct EventRing {
    buffer: DmaBuffer,
    erst: DmaBuffer,
    dequeue: usize,
    cycle: bool,
}

impl EventRing {
    pub fn new() -> Option<Self> {
        let buffer = DmaBuffer::new(TRBS_PER_RING * TRB_SIZE, 64)?;
        let erst = DmaBuffer::new(16, 64)?;

        // Event Ring Segment Table entry:
        //   u64 segment base, u16 segment size, u16 reserved, u32 reserved.
        erst.write_u64(0, buffer.physical());
        erst.write_u32(8, TRBS_PER_RING as u32);

        Some(Self {
            buffer,
            erst,
            dequeue: 0,
            cycle: true,
        })
    }

    pub fn erst_physical(&self) -> u64 {
        self.erst.physical()
    }

    pub fn segment_count(&self) -> u32 {
        1
    }

    pub fn ring_physical(&self) -> u64 {
        self.buffer.physical()
    }

    /// ERDP value for the next event to be consumed.
    pub fn erdp(&self) -> u64 {
        self.buffer.physical() + (self.dequeue * TRB_SIZE) as u64
    }

    /// Remove the next event, if the controller published one.
    pub fn pop(&mut self) -> Option<Trb> {
        let address = self.buffer.physical() + (self.dequeue * TRB_SIZE) as u64;

        let event = unsafe { core::ptr::read_volatile(address as *const Trb) };

        if event.cycle() != self.cycle {
            return None;
        }

        self.dequeue += 1;

        if self.dequeue == TRBS_PER_RING {
            self.dequeue = 0;
            self.cycle = !self.cycle;
        }

        Some(event)
    }

    /// True when the ring has wrapped to a fresh segment.
    pub fn wrapped(&self) -> bool {
        self.dequeue == 0
    }
}
