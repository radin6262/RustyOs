mod allocator;
pub mod dma;
mod user;

use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{
        AtomicU64,
        AtomicU8,
        Ordering,
    },
};

use bootloader_api::info::{
    BootInfo,
    MemoryRegionKind,
};

use x86_64::{
    PhysAddr,
    VirtAddr,
    registers::control::Cr3,
    structures::paging::{
        FrameAllocator,
        Mapper,
        OffsetPageTable,
        Page,
        PageSize,
        PageTable,
        PageTableFlags,
        PhysFrame,
        Size2MiB,
        Size4KiB,
        Translate,
    },
};

// ============================================================
// Public exports
// ============================================================

pub use allocator::{
    allocate_dma_region,
    allocate_page_table_frame,
    allocate_user_frame,
    check_contiguous_physical,
    copy_to_frame,
    physical_memory_offset,
    physical_to_virtual,
    zero_frame,
};

pub use user::{
    map_user_page,
    validate_user_range,
};

// ============================================================
// Constants
// ============================================================

pub(crate) const PAGE_SIZE: u64 =
    0x1000;

pub(crate) const LARGE_PAGE_SIZE: u64 =
    0x20_0000;

const MIN_ALLOCATOR_ADDRESS: u64 =
    0x10_0000;

const PAGE_TABLE_RESERVE_SIZE: u64 =
    4 * 1024 * 1024;

pub const DMA_REGION_SIZE: u64 =
    128 * 1024 * 1024;

// Dedicated DMA space inside the reserved 128 MiB region.
//
// Layout:
//
//     4 MiB  page tables
//    96 MiB  kernel heap
//    28 MiB  USB DMA
//
// Total:
//
//    128 MiB
//
const USB_DMA_SIZE: u64 =
    28 * 1024 * 1024;

// ============================================================
// Userspace address limits
// ============================================================

pub const USER_MIN: u64 =
    0x0000_0000_0000_1000;

pub const USER_MAX: u64 =
    0x0000_8000_0000_0000;

// ============================================================
// Physical memory offset
// ============================================================

static PHYSICAL_MEMORY_OFFSET:
AtomicU64 =
    AtomicU64::new(0);

// ============================================================
// Global kernel mapper
// ============================================================

struct MapperStorage {
    mapper:
        UnsafeCell<
            MaybeUninit<
                OffsetPageTable<'static>,
            >,
        >,
}

unsafe impl Sync
for MapperStorage {
}

impl MapperStorage {
    const fn new() -> Self {
        Self {
            mapper:
            UnsafeCell::new(
                MaybeUninit::uninit(),
            ),
        }
    }
}

static MAPPER:
MapperStorage =
    MapperStorage::new();

static MAPPER_READY:
AtomicU8 =
    AtomicU8::new(0);

// ============================================================
// Global mapper access
// ============================================================

pub unsafe fn mapper_mut()
    -> &'static mut OffsetPageTable<'static>
{
    if MAPPER_READY.load(
        Ordering::Acquire,
    ) == 0 {
        panic!(
            "Rusty: page mapper not initialized",
        );
    }

    unsafe {
        &mut *(
            (*MAPPER.mapper.get())
                .as_mut_ptr()
        )
    }
}

// ============================================================
// Alignment helper
// ============================================================

pub(crate) fn align_up(
    value: u64,
    alignment: u64,
) -> Option<u64> {
    if alignment == 0 {
        return None;
    }

    value
        .checked_add(
            alignment - 1,
        )
        .map(|value|
            value
                & !(alignment - 1)
        )
}

// ============================================================
// Find reserved physical memory region
// ============================================================
//
// We reserve:
//
//     4 MiB  page tables
//   128 MiB  kernel/DMA area
//
// Total:
//
//   132 MiB
//
// The 128 MiB area is split later into:
//
//     kernel heap + USB DMA
//
// ============================================================

fn find_dma_region(
    boot_info: &BootInfo,
) -> Option<(
    u64,
    u64,
)> {
    let required =
        PAGE_TABLE_RESERVE_SIZE
            .checked_add(
                DMA_REGION_SIZE,
            )?;

    for region
    in boot_info.memory_regions.iter()
    {
        if region.kind
            != MemoryRegionKind::Usable
        {
            continue;
        }

        let minimum_start =
            region.start.max(
                MIN_ALLOCATOR_ADDRESS,
            );

        let start =
            align_up(
                minimum_start,
                LARGE_PAGE_SIZE,
            )?;

        let end =
            start.checked_add(
                required,
            )?;

        if end <= region.end {
            return Some((
                start,
                end,
            ));
        }
    }

    None
}

// ============================================================
// Kernel mapper initialization
// ============================================================

unsafe fn init_mapper(
    boot_info: &BootInfo,
) {
    let physical_offset =
        boot_info
            .physical_memory_offset
            .into_option()
            .expect(
                "Rusty: physical memory mapping unavailable",
            );

    PHYSICAL_MEMORY_OFFSET.store(
        physical_offset,
        Ordering::Release,
    );

    let (
        level_4_frame,
        _,
    ) =
        Cr3::read();

    let level_4_physical =
        level_4_frame
            .start_address()
            .as_u64();

    let level_4_virtual =
        physical_offset
            .checked_add(
                level_4_physical,
            )
            .expect(
                "Rusty: L4 virtual address overflow",
            );

    let level_4_table =
        level_4_virtual
            as *mut PageTable;

    let level_4_table:
        &'static mut PageTable =
        unsafe {
            &mut *level_4_table
        };

    let mapper =
        unsafe {
            OffsetPageTable::new(
                level_4_table,
                VirtAddr::new(
                    physical_offset,
                ),
            )
        };

    unsafe {
        (*MAPPER.mapper.get())
            .write(
                mapper,
            );
    }

    MAPPER_READY.store(
        1,
        Ordering::Release,
    );
}

// ============================================================
// Identity-map one 2 MiB page
// ============================================================

fn identity_map_2m(
    physical: u64,
) -> bool {
    let virtual_address =
        VirtAddr::new(
            physical,
        );

    unsafe {
        let mapper =
            mapper_mut();

        // Already mapped?

        if let Some(mapped) =
            mapper.translate_addr(
                virtual_address,
            )
        {
            return mapped.as_u64()
                == physical;
        }

        let page =
            Page::<Size2MiB>
            ::from_start_address(
                virtual_address,
            )
                .expect(
                    "Rusty: invalid 2 MiB virtual page",
                );

        let frame =
            PhysFrame::<Size2MiB>
            ::from_start_address(
                PhysAddr::new(
                    physical,
                ),
            )
                .expect(
                    "Rusty: invalid 2 MiB physical frame",
                );

        let flags =
            PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::NO_EXECUTE;

        let mut frame_allocator =
            allocator::PageTableFrameAllocator;

        match mapper.identity_map(
            frame,
            flags,
            &mut frame_allocator,
        ) {
            Ok(flush) => {
                flush.flush();
                true
            }

            Err(_) =>
                false,
        }
    }
}

// ============================================================
// Identity-map MMIO
// ============================================================

pub unsafe fn identity_map_mmio(
    address: usize,
    size: usize,
) {
    if size == 0 {
        return;
    }

    let address_u64 =
        address as u64;

    let start =
        address_u64
            & !(PAGE_SIZE - 1);

    let end_unaligned =
        address_u64
            .checked_add(
                size as u64,
            )
            .expect(
                "Rusty: MMIO range overflow",
            );

    let end =
        align_up(
            end_unaligned,
            PAGE_SIZE,
        )
            .expect(
                "Rusty: MMIO alignment overflow",
            );

    let mut physical =
        start;

    while physical < end {
        unsafe {
            let mapper =
                mapper_mut();

            if let Some(mapped) =
                mapper.translate_addr(
                    VirtAddr::new(
                        physical,
                    ),
                )
            {
                if mapped.as_u64()
                    != physical
                {
                    panic!(
                        "Rusty: MMIO virtual address collision",
                    );
                }
            } else {
                let page =
                    Page::<Size4KiB>
                    ::from_start_address(
                        VirtAddr::new(
                            physical,
                        ),
                    )
                        .expect(
                            "Rusty: invalid MMIO page",
                        );

                let frame =
                    PhysFrame::<Size4KiB>
                    ::from_start_address(
                        PhysAddr::new(
                            physical,
                        ),
                    )
                        .expect(
                            "Rusty: invalid MMIO frame",
                        );

                let flags =
                    PageTableFlags::PRESENT
                        | PageTableFlags::WRITABLE
                        | PageTableFlags::NO_EXECUTE
                        | PageTableFlags::NO_CACHE;

                let mut frame_allocator =
                    allocator::PageTableFrameAllocator;

                mapper
                    .map_to(
                        page,
                        frame,
                        flags,
                        &mut frame_allocator,
                    )
                    .expect(
                        "Rusty: failed to identity-map xHCI MMIO",
                    )
                    .flush();
            }
        }

        physical =
            physical
                .checked_add(
                    PAGE_SIZE,
                )
                .expect(
                    "Rusty: MMIO physical address overflow",
                );
    }
}

// ============================================================
// Main initialization
// ============================================================

pub fn init(
    boot_info: &BootInfo,
) {
    unsafe {
        init_mapper(
            boot_info,
        );
    }

    let Some((
                 region_start,
                 region_end,
             )) =
        find_dma_region(
            boot_info,
        )
    else {
        panic!(
            "Rusty: no usable 128 MiB DMA region found",
        );
    };

    // ========================================================
    // Dedicated page-table region
    // ========================================================

    let page_table_start =
        region_start;

    let page_table_end =
        page_table_start
            .checked_add(
                PAGE_TABLE_RESERVE_SIZE,
            )
            .expect(
                "Rusty: page-table reserve overflow",
            );

    // ========================================================
    // Split remaining memory
    // ========================================================
    //
    //     [ 96 MiB kernel heap ][ 28 MiB USB DMA ]
    //
    // These allocators MUST NEVER overlap.
    //
    // ========================================================

    let kernel_heap_start =
        page_table_end;

    let kernel_heap_end =
        region_end
            .checked_sub(
                USB_DMA_SIZE,
            )
            .expect(
                "Rusty: USB DMA region underflow",
            );

    if kernel_heap_end <= kernel_heap_start {
        panic!(
            "Rusty: USB DMA region leaves no kernel heap",
        );
    }

    let dma_start =
        kernel_heap_end;

    let dma_end =
        region_end;

    allocator::init_regions(
        page_table_start,
        page_table_end,
        kernel_heap_start,
        kernel_heap_end,
        dma_start,
        dma_end,
    );

    // ========================================================
    // Identity-map kernel heap + USB DMA
    // ========================================================

    let mut physical =
        kernel_heap_start;

    while physical < dma_end {
        if !identity_map_2m(
            physical,
        ) {
            panic!(
                "Rusty: could not identity-map kernel/DMA memory",
            );
        }

        physical =
            physical
                .checked_add(
                    LARGE_PAGE_SIZE,
                )
                .expect(
                    "Rusty: kernel/DMA mapping overflow",
                );
    }
}

// ============================================================
// Mapper status
// ============================================================

pub fn mapper_ready()
    -> bool
{
    MAPPER_READY.load(
        Ordering::Acquire,
    ) != 0
}

// ============================================================
// Current PML4
// ============================================================

pub fn current_level_4_frame()
    -> PhysFrame<Size4KiB>
{
    let (
        frame,
        _,
    ) =
        Cr3::read();

    frame
}