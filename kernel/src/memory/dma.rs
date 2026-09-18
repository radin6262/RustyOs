use core::{
    alloc::Layout,
    num::NonZeroUsize,
    ptr::NonNull,
};

use dma_api::{
    DeviceDma,
    DmaAddr,
    DmaAllocHandle,
    DmaCoherency,
    DmaConstraints,
    DmaDeviceInfo,
    DmaDirection,
    DmaDomainId,
    DmaError,
    DmaMapHandle,
    DmaOp,
};

use crate::memory;

// ============================================================
// Configuration
// ============================================================

const DEBUG_DMA: bool = true;

// ============================================================
// Static DMA backend
// ============================================================
//
// DeviceDma stores:
//
//     &'static dyn DmaOp
//
// so the backend itself must live for the entire kernel lifetime.
//
// ============================================================

static RUSTY_DMA_OP: RustyDma = RustyDma;

// ============================================================
// Rusty DMA backend
// ============================================================

pub struct RustyDma;

// ============================================================
// DmaOp implementation
// ============================================================

impl DmaOp for RustyDma {
    // --------------------------------------------------------
    // Page size
    // --------------------------------------------------------

    fn page_size(
        &self,
    ) -> usize {
        4096
    }

    // ========================================================
    // Allocate physically contiguous DMA memory
    // ========================================================

    unsafe fn alloc_contiguous(
        &self,
        constraints: DmaConstraints,
        layout: Layout,
    ) -> Option<DmaAllocHandle> {
        let size =
            layout.size();

        if size == 0 {
            return None;
        }

        // ----------------------------------------------------
        // Effective alignment
        // ----------------------------------------------------

        let align =
            layout
                .align()
                .max(constraints.align)
                .max(1);

        if !align.is_power_of_two() {
            return None;
        }

        // ----------------------------------------------------
        // Maximum segment size
        // ----------------------------------------------------

        if let Some(max_segment_size) =
            constraints.max_segment_size
        {
            if size > max_segment_size {
                return None;
            }
        }

        // ----------------------------------------------------
        // Allocate from Rusty's dedicated DMA region.
        //
        // This provides:
        //
        //     physical contiguity
        //     alignment
        //     boundary handling
        //
        // ----------------------------------------------------

        let phys_addr =
            memory::allocate_dma_region(
                size,
                align,
                constraints.boundary,
            )?;

        // ----------------------------------------------------
        // Verify DMA address mask.
        // ----------------------------------------------------

        let last_byte =
            phys_addr
                .checked_add(
                    size as u64,
                )
                .and_then(|end|
                    end.checked_sub(1)
                )?;

        if last_byte
            > constraints.addr_mask
        {
            return None;
        }

        // ----------------------------------------------------
        // Convert physical address to CPU-visible address.
        // ----------------------------------------------------

        let cpu_addr =
            memory::physical_to_virtual(
                phys_addr,
            );

        let cpu_ptr =
            NonNull::new(
                cpu_addr,
            )?;

        // ----------------------------------------------------
        // DMA address wrapper.
        // ----------------------------------------------------

        let dma_addr =
            DmaAddr::from(
                phys_addr,
            );

        // ----------------------------------------------------
        // Debug output.
        // ----------------------------------------------------

        if DEBUG_DMA {
            crate::serial::write_str(
                "USB DMA alloc: phys=",
            );

            crate::serial::write_hex(
                phys_addr,
            );

            crate::serial::write_str(
                " size=",
            );

            crate::serial::write_usize(
                size,
            );

            crate::serial::write_str(
                " align=",
            );

            crate::serial::write_usize(
                align,
            );

            crate::serial::write_str(
                "\n",
            );
        }

        // ----------------------------------------------------
        // Construct DMA allocation handle.
        //
        // CPU mapping and allocator-owned mapping point to the
        // same direct physical-memory mapping.
        // ----------------------------------------------------

        Some(
            unsafe {
                DmaAllocHandle::new(
                    cpu_ptr,
                    cpu_ptr,
                    dma_addr,
                    layout,
                )
            },
        )
    }

    // ========================================================
    // Deallocate physically contiguous memory
    // ========================================================

    unsafe fn dealloc_contiguous(
        &self,
        _handle: DmaAllocHandle,
    ) {
        // Rusty's allocator is currently monotonic.
        //
        // Physical DMA memory is not reclaimed yet.
    }

    // ========================================================
    // Allocate coherent DMA memory
    // ========================================================

    unsafe fn alloc_coherent(
        &self,
        constraints: DmaConstraints,
        layout: Layout,
    ) -> Option<DmaAllocHandle> {
        // ----------------------------------------------------
        // x86_64 Rusty currently uses coherent memory.
        //
        // Therefore coherent allocation is identical to normal
        // physically contiguous allocation.
        // ----------------------------------------------------

        unsafe {
            self.alloc_contiguous(
                constraints,
                layout,
            )
        }
    }

    // ========================================================
    // Deallocate coherent DMA memory
    // ========================================================

    unsafe fn dealloc_coherent(
        &self,
        _handle: DmaAllocHandle,
    ) -> Result<(), DmaError> {
        // Monotonic allocator.
        //
        // Nothing is reclaimed yet.

        Ok(())
    }

    // ========================================================
    // Streaming DMA mapping
    // ========================================================

    unsafe fn map_streaming(
        &self,
        constraints: DmaConstraints,
        addr: NonNull<u8>,
        size: NonZeroUsize,
        _direction: DmaDirection,
    ) -> Result<DmaMapHandle, DmaError> {
        let virtual_address =
            addr.as_ptr() as u64;

        let size =
            size.get();

        // ====================================================
        // Maximum segment size
        // ====================================================

        if let Some(max_segment_size) =
            constraints.max_segment_size
        {
            if size > max_segment_size {
                return Err(
                    DmaError::SegmentTooLarge {
                        size,
                        max: max_segment_size,
                    },
                );
            }
        }

        // ====================================================
        // Fast path: directly map existing physical memory
        // ====================================================

        if let Some(phys_start) =
            memory::check_contiguous_physical(
                virtual_address,
                size,
            )
        {
            // ------------------------------------------------
            // Alignment
            // ------------------------------------------------

            let required_alignment =
                constraints
                    .align
                    .max(1);

            let alignment_ok =
                phys_start
                    % required_alignment as u64
                    == 0;

            // ------------------------------------------------
            // Calculate final byte.
            // ------------------------------------------------

            let last_byte =
                match phys_start
                    .checked_add(
                        size as u64,
                    )
                    .and_then(|end|
                        end.checked_sub(1)
                    )
                {
                    Some(value) =>
                        value,

                    None =>
                        return Err(
                            DmaError::DmaMaskNotMatch {
                                addr: DmaAddr::from(
                                    phys_start,
                                ),
                                mask:
                                constraints.addr_mask,
                            },
                        ),
                };

            // ------------------------------------------------
            // Address mask
            // ------------------------------------------------

            let mask_ok =
                last_byte
                    <= constraints.addr_mask;

            // ------------------------------------------------
            // Boundary
            // ------------------------------------------------

            let boundary_ok =
                match constraints.boundary {
                    None =>
                        true,

                    Some(boundary) => {
                        if boundary == 0 {
                            false
                        } else {
                            let first_boundary =
                                phys_start
                                    / boundary as u64;

                            let last_boundary =
                                last_byte
                                    / boundary as u64;

                            first_boundary
                                == last_boundary
                        }
                    }
                };

            // ------------------------------------------------
            // Existing buffer is DMA-compatible.
            // ------------------------------------------------

            if alignment_ok
                && mask_ok
                && boundary_ok
            {
                if DEBUG_DMA {
                    crate::serial::write_str(
                        "USB DMA fast-map: phys=",
                    );

                    crate::serial::write_hex(
                        phys_start,
                    );

                    crate::serial::write_str(
                        " size=",
                    );

                    crate::serial::write_usize(
                        size,
                    );

                    crate::serial::write_str(
                        "\n",
                    );
                }

                let dma_addr =
                    DmaAddr::from(
                        phys_start,
                    );

                let layout =
                    unsafe {
                        Layout::from_size_align_unchecked(
                            size,
                            1,
                        )
                    };

                return Ok(
                    unsafe {
                        DmaMapHandle::new(
                            addr,
                            dma_addr,
                            layout,
                            None,
                        )
                    },
                );
            }
        }

        // ====================================================
        // Bounce buffer
        // ====================================================
        //
        // The caller's original buffer cannot be used directly.
        //
        // Allocate a physically contiguous DMA-safe buffer from
        // Rusty's dedicated USB DMA region.
        //
        // IMPORTANT:
        //
        // Do NOT copy the buffer here.
        //
        // dma-api's StreamingMap synchronization methods handle
        // the direction-aware bounce-buffer synchronization.
        //
        // ====================================================

        let alignment =
            constraints
                .align
                .max(1);

        if !alignment.is_power_of_two() {
            return Err(
                DmaError::AlignMismatch {
                    required: alignment,
                    address:
                    DmaAddr::from(
                        virtual_address,
                    ),
                },
            );
        }

        let phys_bounce =
            memory::allocate_dma_region(
                size,
                alignment,
                constraints.boundary,
            )
                .ok_or(
                    DmaError::NoMemory,
                )?;

        // ----------------------------------------------------
        // Address mask
        // ----------------------------------------------------

        let bounce_last_byte =
            phys_bounce
                .checked_add(
                    size as u64,
                )
                .and_then(|end|
                    end.checked_sub(1)
                )
                .ok_or(
                    DmaError::DmaMaskNotMatch {
                        addr:
                        DmaAddr::from(
                            phys_bounce,
                        ),
                        mask:
                        constraints.addr_mask,
                    },
                )?;

        if bounce_last_byte
            > constraints.addr_mask
        {
            return Err(
                DmaError::DmaMaskNotMatch {
                    addr:
                    DmaAddr::from(
                        phys_bounce,
                    ),
                    mask:
                    constraints.addr_mask,
                },
            );
        }

        // ----------------------------------------------------
        // Boundary
        // ----------------------------------------------------

        if let Some(boundary) =
            constraints.boundary
        {
            if boundary == 0 {
                return Err(
                    DmaError::BoundaryCross {
                        addr:
                        DmaAddr::from(
                            phys_bounce,
                        ),
                        size,
                        boundary,
                    },
                );
            }

            let first_boundary =
                phys_bounce
                    / boundary as u64;

            let last_boundary =
                bounce_last_byte
                    / boundary as u64;

            if first_boundary
                != last_boundary
            {
                return Err(
                    DmaError::BoundaryCross {
                        addr:
                        DmaAddr::from(
                            phys_bounce,
                        ),
                        size,
                        boundary,
                    },
                );
            }
        }

        // ----------------------------------------------------
        // CPU mapping for bounce memory.
        // ----------------------------------------------------

        let cpu_bounce =
            memory::physical_to_virtual(
                phys_bounce,
            );

        let cpu_bounce_ptr =
            unsafe {
                NonNull::new_unchecked(
                    cpu_bounce,
                )
            };

        // ----------------------------------------------------
        // Debug
        // ----------------------------------------------------

        if DEBUG_DMA {
            crate::serial::write_str(
                "USB DMA bounce-map: phys=",
            );

            crate::serial::write_hex(
                phys_bounce,
            );

            crate::serial::write_str(
                " size=",
            );

            crate::serial::write_usize(
                size,
            );

            crate::serial::write_str(
                " align=",
            );

            crate::serial::write_usize(
                alignment,
            );

            crate::serial::write_str(
                "\n",
            );
        }

        let dma_addr =
            DmaAddr::from(
                phys_bounce,
            );

        let layout =
            unsafe {
                Layout::from_size_align_unchecked(
                    size,
                    1,
                )
            };

        Ok(
            unsafe {
                DmaMapHandle::new(
                    addr,
                    dma_addr,
                    layout,
                    Some(
                        cpu_bounce_ptr,
                    ),
                )
            },
        )
    }

    // ========================================================
    // Unmap streaming DMA
    // ========================================================

    unsafe fn unmap_streaming(
        &self,
        _handle: DmaMapHandle,
    ) {
        // ----------------------------------------------------
        // Nothing to reclaim yet.
        //
        // This is intentionally NOT:
        //
        //     copy bounce -> original
        //
        // because DmaMapHandle does not contain the transfer
        // direction.
        //
        // dma-api performs direction-aware synchronization
        // through its StreamingMap API.
        //
        // ----------------------------------------------------
    }
}

// ============================================================
// Create Rusty's USB DeviceDma
// ============================================================

pub fn usb_device_dma()
    -> DeviceDma
{
    let info =
        DmaDeviceInfo::new(
            DmaDomainId::Direct,
            DmaCoherency::Coherent,
            DmaConstraints::new(
                u64::MAX,
            ),
        );

    DeviceDma::new(
        info,
        &RUSTY_DMA_OP,
    )
}