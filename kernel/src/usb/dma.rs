// ============================================================
// DMA buffers for the xHCI driver
// ============================================================
//
// The xHCI controller reads and writes memory by physical address. The
// kernel reserves a physically contiguous USB DMA region in
// crate::memory, and that region is identity mapped:
//
//     physical address == virtual address
//
// allocate_dma_region() returns the physical address of zeroed,
// page-aligned memory, so the same value can be used as a pointer from the
// CPU and as a DMA address for the controller.
// ============================================================

use crate::memory;

/// A physically contiguous, identity-mapped DMA allocation.
pub struct DmaBuffer {
    physical: u64,
    size: usize,
}

impl DmaBuffer {
    /// Allocate a zeroed DMA buffer.
    ///
    /// `size` is rounded up to a whole page by the allocator. `alignment`
    /// must be a power of two; the allocator raises it to at least one page.
    pub fn new(size: usize, alignment: usize) -> Option<Self> {
        if size == 0 {
            return None;
        }

        let physical = memory::allocate_dma_region(size, alignment, None)?;

        // The USB DMA region is identity mapped (see memory.rs), so the
        // physical address is directly addressable. Refuse an obviously
        // unusable address instead of dereferencing it later.
        if physical == 0 {
            return None;
        }

        Some(Self {
            physical,
            size,
        })
    }

    /// Physical (and virtual) base address of the buffer.
    pub fn physical(&self) -> u64 {
        self.physical
    }

    /// Size that was requested from the allocator.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Base pointer for CPU access.
    pub fn as_ptr(&self) -> *mut u8 {
        self.physical as *mut u8
    }

    /// Typed pointer for CPU access.
    ///
    /// # Safety
    ///
    /// `T` must be readable and writable at this buffer with the requested
    /// alignment, and the caller must uphold Rust aliasing rules.
    pub unsafe fn as_mut<T>(&self) -> *mut T {
        self.physical as *mut T
    }

    /// Read a 32-bit field at `offset`.
    pub fn read_u32(&self, offset: usize) -> u32 {
        debug_assert!(offset + 4 <= self.size);

        unsafe { core::ptr::read_volatile((self.physical as usize + offset) as *const u32) }
    }

    /// Write a 32-bit field at `offset`.
    pub fn write_u32(&self, offset: usize, value: u32) {
        debug_assert!(offset + 4 <= self.size);

        unsafe {
            core::ptr::write_volatile((self.physical as usize + offset) as *mut u32, value);
        }
    }

    /// Read a 64-bit field at `offset`.
    pub fn read_u64(&self, offset: usize) -> u64 {
        debug_assert!(offset + 8 <= self.size);

        unsafe { core::ptr::read_volatile((self.physical as usize + offset) as *const u64) }
    }

    /// Write a 64-bit field at `offset`.
    pub fn write_u64(&self, offset: usize, value: u64) {
        debug_assert!(offset + 8 <= self.size);

        unsafe {
            core::ptr::write_volatile((self.physical as usize + offset) as *mut u64, value);
        }
    }

    /// Zero the whole buffer.
    pub fn clear(&self) {
        unsafe {
            core::ptr::write_bytes(self.physical as *mut u8, 0, self.size);
        }
    }

    /// Copy bytes into the buffer.
    pub fn write_bytes(&self, offset: usize, bytes: &[u8]) {
        debug_assert!(offset + bytes.len() <= self.size);

        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                (self.physical as usize + offset) as *mut u8,
                bytes.len(),
            );
        }
    }

    /// Copy bytes out of the buffer.
    pub fn read_bytes(&self, offset: usize, out: &mut [u8]) {
        debug_assert!(offset + out.len() <= self.size);

        unsafe {
            core::ptr::copy_nonoverlapping(
                (self.physical as usize + offset) as *const u8,
                out.as_mut_ptr(),
                out.len(),
            );
        }
    }
}

impl core::fmt::Debug for DmaBuffer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "DmaBuffer {{ physical: {:#x}, size: {:#x} }}",
            self.physical, self.size
        )
    }
}
