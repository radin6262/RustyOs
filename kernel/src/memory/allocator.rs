use core::{
    alloc::{
        GlobalAlloc,
        Layout,
    },
    ptr::null_mut,
    sync::atomic::{
        AtomicU8,
        AtomicU64,
        AtomicUsize,
        Ordering,
    },
};

use x86_64::{
    PhysAddr,
    VirtAddr,
    structures::paging::{
        FrameAllocator,
        PageTable,
        PageTableFlags,
        PhysFrame,
        Size4KiB,
        Translate,
    },
};

use super::{
    align_up,
    mapper_mut,
    mapper_ready,
    PAGE_SIZE,
};

// ============================================================
// Kernel allocator
// ============================================================

struct KernelAllocator {
    start:
        AtomicUsize,

    end:
        AtomicUsize,

    initialized:
        AtomicU8,
}

impl KernelAllocator {
    const fn new() -> Self {
        Self {
            start:
            AtomicUsize::new(0),

            end:
            AtomicUsize::new(0),

            initialized:
            AtomicU8::new(0),
        }
    }
}

unsafe impl GlobalAlloc
for KernelAllocator {
    unsafe fn alloc(
        &self,
        layout: Layout,
    ) -> *mut u8 {
        if self.initialized.load(
            Ordering::Acquire,
        ) == 0 {
            return null_mut();
        }

        let alignment =
            layout.align();

        let mut current =
            self.start.load(
                Ordering::Relaxed,
            );

        let end =
            self.end.load(
                Ordering::Acquire,
            );

        loop {
            let aligned =
                match current.checked_add(
                    alignment.saturating_sub(1),
                ) {
                    Some(value) =>
                        value & !(alignment - 1),

                    None =>
                        return null_mut(),
                };

            let next =
                match aligned.checked_add(
                    layout.size(),
                ) {
                    Some(value) =>
                        value,

                    None =>
                        return null_mut(),
                };

            if next > end {
                return null_mut();
            }

            match self.start.compare_exchange(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) =>
                    return aligned as *mut u8,

                Err(actual) =>
                    current = actual,
            }
        }
    }

    unsafe fn dealloc(
        &self,
        _ptr: *mut u8,
        _layout: Layout,
    ) {
        // Monotonic allocator.
        //
        // Reclamation is not implemented yet.
    }
}

#[global_allocator]
static ALLOCATOR:
KernelAllocator =
    KernelAllocator::new();

// ============================================================
// DMA allocator state
// ============================================================
//
// Dedicated USB DMA region:
//
//     DMA_ALLOC_START
//              |
//              v
//     +----------------------+
//     | allocated DMA blocks |
//     +----------------------+
//                        |
//                        v
//                   DMA_ALLOC_NEXT
//                        |
//                        v
//                    DMA_ALLOC_END
//
// DMA_ALLOC_START is important because DMA_ALLOC_NEXT moves.
// Checking addresses against DMA_ALLOC_NEXT would incorrectly
// reject already allocated buffers.
//

static DMA_ALLOC_START:
AtomicU64 =
    AtomicU64::new(0);

static DMA_ALLOC_NEXT:
AtomicU64 =
    AtomicU64::new(0);

static DMA_ALLOC_END:
AtomicU64 =
    AtomicU64::new(0);

// ============================================================
// Page-table allocator state
// ============================================================

static PAGE_TABLE_NEXT:
AtomicU64 =
    AtomicU64::new(0);

static PAGE_TABLE_END:
AtomicU64 =
    AtomicU64::new(0);

// ============================================================
// Page-table frame allocator
// ============================================================
//
// Used ONLY for:
//
//     PML4
//     PDPT
//     PD
//     PT
//
// It never touches the kernel heap or DMA allocator.
//

pub(super) struct PageTableFrameAllocator;

unsafe impl FrameAllocator<Size4KiB>
for PageTableFrameAllocator {
    fn allocate_frame(
        &mut self,
    ) -> Option<
        PhysFrame<Size4KiB>
    > {
        loop {
            let current =
                PAGE_TABLE_NEXT.fetch_add(
                    PAGE_SIZE,
                    Ordering::AcqRel,
                );

            let end =
                PAGE_TABLE_END.load(
                    Ordering::Acquire,
                );

            if current >= end {
                return None;
            }

            let next =
                match current.checked_add(
                    PAGE_SIZE,
                ) {
                    Some(value) =>
                        value,

                    None =>
                        return None,
                };

            if next > end {
                return None;
            }

            let frame =
                PhysFrame::<Size4KiB>
                ::from_start_address(
                    PhysAddr::new(
                        current,
                    ),
                )
                    .ok()?;

            // Page-table frames must start zeroed.

            zero_frame(
                frame,
            );

            return Some(
                frame,
            );
        }
    }
}

// ============================================================
// Initialize allocator regions
// ============================================================

pub(super) fn init_regions(
    page_table_start:
    u64,

    page_table_end:
    u64,

    kernel_heap_start:
    u64,

    kernel_heap_end:
    u64,

    dma_start:
    u64,

    dma_end:
    u64,
) {
    if page_table_start
        >= page_table_end
    {
        panic!(
            "Rusty: invalid page-table allocator range",
        );
    }

    if kernel_heap_start
        >= kernel_heap_end
    {
        panic!(
            "Rusty: invalid kernel heap range",
        );
    }

    if dma_start >= dma_end {
        panic!(
            "Rusty: invalid DMA allocator range",
        );
    }

    if kernel_heap_end
        > dma_start
    {
        panic!(
            "Rusty: kernel heap overlaps DMA region",
        );
    }

    // --------------------------------------------------------
    // Page tables
    // --------------------------------------------------------

    PAGE_TABLE_NEXT.store(
        page_table_start,
        Ordering::Release,
    );

    PAGE_TABLE_END.store(
        page_table_end,
        Ordering::Release,
    );

    // --------------------------------------------------------
    // Kernel heap
    // --------------------------------------------------------

    ALLOCATOR.start.store(
        kernel_heap_start as usize,
        Ordering::Release,
    );

    ALLOCATOR.end.store(
        kernel_heap_end as usize,
        Ordering::Release,
    );

    ALLOCATOR.initialized.store(
        1,
        Ordering::Release,
    );

    // --------------------------------------------------------
    // USB DMA
    // --------------------------------------------------------

    DMA_ALLOC_START.store(
        dma_start,
        Ordering::Release,
    );

    DMA_ALLOC_NEXT.store(
        dma_start,
        Ordering::Release,
    );

    DMA_ALLOC_END.store(
        dma_end,
        Ordering::Release,
    );

    let kernel_size =
        kernel_heap_end
            .checked_sub(
                kernel_heap_start,
            )
            .unwrap_or(0);

    let dma_size =
        dma_end
            .checked_sub(
                dma_start,
            )
            .unwrap_or(0);

    if kernel_size == 0
        || dma_size == 0
    {
        panic!(
            "Rusty: allocator region size is zero",
        );
    }
}

// ============================================================
// Allocator status
// ============================================================

pub fn allocator_ready()
    -> bool
{
    ALLOCATOR
        .initialized
        .load(
            Ordering::Acquire,
        ) != 0
}

// ============================================================
// Physical memory offset
// ============================================================

pub fn physical_memory_offset()
    -> u64
{
    super::PHYSICAL_MEMORY_OFFSET.load(
        Ordering::Acquire,
    )
}

// ============================================================
// Physical -> virtual
// ============================================================

pub fn physical_to_virtual(
    physical: u64,
) -> *mut u8 {
    let offset =
        physical_memory_offset();

    let virtual_address =
        offset
            .checked_add(
                physical,
            )
            .expect(
                "Rusty: physical-to-virtual overflow",
            );

    virtual_address as *mut u8
}

// ============================================================
// Virtual -> physical
// ============================================================

pub fn virtual_to_physical(
    virtual_address: u64,
) -> Option<u64> {
    if !mapper_ready() {
        return None;
    }

    let mapper =
        unsafe {
            mapper_mut()
        };

    mapper
        .translate_addr(
            VirtAddr::new(
                virtual_address,
            ),
        )
        .map(|physical|
            physical.as_u64()
        )
}

// ============================================================
// Allocate normal physical frame
// ============================================================
//
// This comes from the kernel heap region.
//
// It is NOT used for page tables.
//

pub fn allocate_frame()
    -> Option<PhysFrame<Size4KiB>>
{
    let layout =
        Layout::from_size_align(
            PAGE_SIZE as usize,
            PAGE_SIZE as usize,
        )
            .ok()?;

    let pointer =
        unsafe {
            ALLOCATOR.alloc(
                layout,
            )
        };

    if pointer.is_null() {
        return None;
    }

    unsafe {
        core::ptr::write_bytes(
            pointer,
            0,
            PAGE_SIZE as usize,
        );
    }

    PhysFrame::<Size4KiB>
    ::from_start_address(
        PhysAddr::new(
            pointer as u64,
        ),
    )
        .ok()
}

// ============================================================
// Allocate user physical frame
// ============================================================

pub fn allocate_user_frame()
    -> Option<PhysFrame<Size4KiB>>
{
    allocate_frame()
}

// ============================================================
// Allocate page-table frame
// ============================================================

pub fn allocate_page_table_frame()
    -> Option<PhysFrame<Size4KiB>>
{
    let mut allocator =
        PageTableFrameAllocator;

    allocator.allocate_frame()
}

// ============================================================
// Zero physical frame
// ============================================================

pub fn zero_frame(
    frame:
    PhysFrame<Size4KiB>,
) {
    let pointer =
        physical_to_virtual(
            frame
                .start_address()
                .as_u64(),
        );

    unsafe {
        core::ptr::write_bytes(
            pointer,
            0,
            PAGE_SIZE as usize,
        );
    }
}

// ============================================================
// Copy bytes into physical frame
// ============================================================

pub fn copy_to_frame(
    frame:
    PhysFrame<Size4KiB>,

    data:
    &[u8],
) {
    assert!(
        data.len()
            <= PAGE_SIZE as usize,
        "Rusty: frame copy overflow",
    );

    let destination =
        physical_to_virtual(
            frame
                .start_address()
                .as_u64(),
        );

    unsafe {
        core::ptr::copy_nonoverlapping(
            data.as_ptr(),
            destination,
            data.len(),
        );
    }
}

// ============================================================
// Allocate physically contiguous DMA memory
// ============================================================
//
// This allocator ONLY uses the dedicated USB DMA region.
//
// The returned address is:
//
//     physical address == DMA address
//
// because this memory is physically contiguous.
//
// ============================================================

pub fn allocate_dma_region(
    size: usize,
    alignment: usize,
    boundary: Option<usize>,
) -> Option<u64> {
    if size == 0 {
        return None;
    }

    if !alignment.is_power_of_two() {
        return None;
    }

    let alignment =
        alignment.max(PAGE_SIZE as usize);

    // --------------------------------------------------------
    // Convert everything to u64.
    //
    // Rusty is x86_64, so usize == u64 here.
    // Keeping the allocator entirely in u64 prevents mixing
    // usize/u64 in the atomic physical-address allocator.
    // --------------------------------------------------------

    let size_u64 =
        size as u64;

    let alignment_u64 =
        alignment as u64;

    let boundary_u64 =
        boundary.map(|value| value as u64);

    // DMA allocations are reserved in whole pages.
    let reserved_size =
        size_u64
            .checked_add(
                PAGE_SIZE - 1,
            )?
            & !(PAGE_SIZE - 1);

    let mut current =
        DMA_ALLOC_NEXT.load(
            Ordering::Relaxed,
        );

    let end =
        DMA_ALLOC_END.load(
            Ordering::Acquire,
        );

    loop {
        // ----------------------------------------------------
        // Align the allocation start.
        // ----------------------------------------------------

        let aligned =
            current
                .checked_add(
                    alignment_u64 - 1,
                )
                .map(|value|
                    value
                        & !(alignment_u64 - 1)
                )?;

        // ----------------------------------------------------
        // Actual requested end.
        // ----------------------------------------------------

        let used_end =
            aligned.checked_add(
                size_u64,
            )?;

        // ----------------------------------------------------
        // Whole amount reserved from the bump allocator.
        // ----------------------------------------------------

        let reservation_end =
            aligned.checked_add(
                reserved_size,
            )?;

        if reservation_end > end {
            return None;
        }

        // ----------------------------------------------------
        // DMA address mask is checked by dma.rs.
        //
        // This allocator only handles:
        //
        //   alignment
        //   boundary
        //   physical contiguity
        // ----------------------------------------------------

        // ----------------------------------------------------
        // Boundary constraint
        // ----------------------------------------------------
        //
        // A DMA buffer must not cross the specified boundary.
        //
        // Do not use:
        //
        //     address & !(boundary - 1)
        //
        // because that assumes boundary is a power of two.
        //

        if let Some(boundary) =
            boundary_u64
        {
            if boundary == 0 {
                return None;
            }

            let first_boundary =
                aligned / boundary;

            let last_byte =
                used_end.checked_sub(
                    1,
                )?;

            let last_boundary =
                last_byte / boundary;

            if first_boundary
                != last_boundary
            {
                // This allocation would cross the boundary.
                //
                // Skip the entire aligned reservation and try
                // again from the next position.
                current =
                    reservation_end;

                continue;
            }
        }

        // ----------------------------------------------------
        // Claim the region atomically.
        // ----------------------------------------------------

        match DMA_ALLOC_NEXT.compare_exchange(
            current,
            reservation_end,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                // The DMA region is identity-mapped.
                //
                // Zero the complete reserved page range.
                unsafe {
                    core::ptr::write_bytes(
                        aligned as *mut u8,
                        0,
                        reserved_size as usize,
                    );
                }

                return Some(
                    aligned,
                );
            }

            Err(actual) => {
                current =
                    actual;
            }
        }
    }
}

// ============================================================
// Compatibility DMA allocator
// ============================================================
//
// Kept so callers that already pass Layout can continue doing:
//
//     memory::allocate_dma(layout)
//
// ============================================================

pub fn allocate_dma(
    layout:
    Layout,
) -> Option<(
    u64,
    usize,
)> {
    let size =
        layout.size();

    let alignment =
        layout.align();

    if size == 0 {
        return None;
    }

    let reserved_size =
        size
            .checked_add(
                PAGE_SIZE as usize - 1,
            )?
            & !(PAGE_SIZE as usize - 1);

    let physical =
        allocate_dma_region(
            size,
            alignment,
            None,
        )?;

    Some((
        physical,
        reserved_size,
    ))
}

// ============================================================
// Validate address inside DMA region
// ============================================================
//
// IMPORTANT:
//
// Do NOT compare against DMA_ALLOC_NEXT.
//
// DMA_ALLOC_NEXT moves forward, so that would reject all
// previously allocated DMA buffers.
//
// ============================================================

pub fn dma_address_valid(
    address:
    u64,

    size:
    usize,
) -> bool {
    if size == 0 {
        return false;
    }

    let end =
        match address.checked_add(
            size as u64,
        ) {
            Some(value) =>
                value,

            None =>
                return false,
        };

    let start =
        DMA_ALLOC_START
            .load(
                Ordering::Acquire,
            );

    let region_end =
        DMA_ALLOC_END
            .load(
                Ordering::Acquire,
            );

    address >= start
        && end <= region_end
}

// ============================================================
// Verify physically contiguous virtual memory
// ============================================================
//
// Used by dma-api streaming mappings.
//
// The check walks every touched 4 KiB page and verifies that
// each physical page is the next physical page in sequence.
//
// ============================================================

pub fn check_contiguous_physical(
    virtual_address:
    u64,

    size:
    usize,
) -> Option<u64> {
    if size == 0 {
        return None;
    }

    let end =
        virtual_address
            .checked_add(
                size as u64,
            )?;

    if end <= virtual_address {
        return None;
    }

    let first_page =
        virtual_address
            & !(PAGE_SIZE - 1);

    let last_page =
        (end - 1)
            & !(PAGE_SIZE - 1);

    let first_physical =
        virtual_to_physical(
            virtual_address,
        )?;

    let first_offset =
        virtual_address
            & (PAGE_SIZE - 1);

    let mut virtual_page =
        first_page;

    let mut expected_physical_page =
        first_physical
            & !(PAGE_SIZE - 1);

    loop {
        let physical =
            virtual_to_physical(
                virtual_page,
            )?;

        if physical
            != expected_physical_page
        {
            return None;
        }

        if virtual_page
            == last_page
        {
            break;
        }

        expected_physical_page =
            expected_physical_page
                .checked_add(
                    PAGE_SIZE,
                )?;

        virtual_page =
            virtual_page
                .checked_add(
                    PAGE_SIZE,
                )?;
    }

    (first_physical
        & !(PAGE_SIZE - 1))
        .checked_add(
            first_offset,
        )
}

// ============================================================
// Physical page-table access
// ============================================================

pub(super) unsafe fn page_table_from_frame(
    frame:
    PhysFrame<Size4KiB>,
) -> &'static PageTable {
    let virtual_address =
        physical_to_virtual(
            frame
                .start_address()
                .as_u64(),
        );

    unsafe {
        &*(
            virtual_address
                as *const PageTable
        )
    }
}