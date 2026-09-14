use core::{
    alloc::{
        GlobalAlloc,
        Layout,
    },
    cell::UnsafeCell,
    mem::MaybeUninit,
    ptr::null_mut,
    sync::atomic::{
        AtomicU8,
        AtomicU64,
        AtomicUsize,
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
// Constants
// ============================================================

const PAGE_SIZE: u64 =
    0x1000;

const LARGE_PAGE_SIZE: u64 =
    0x20_0000;

const MIN_ALLOCATOR_ADDRESS: u64 =
    0x10_0000;

const PAGE_TABLE_RESERVE_SIZE: u64 =
    4 * 1024 * 1024;

const DMA_REGION_SIZE: u64 =
    128 * 1024 * 1024;

// ============================================================
// Userspace address limits
// ============================================================

pub const USER_MIN: u64 =
    0x0000_0000_0000_1000;

pub const USER_MAX: u64 =
    0x0000_0080_0000_0000;

// ============================================================
// Global kernel allocator
// ============================================================

struct KernelAllocator {
    start: AtomicUsize,
    end: AtomicUsize,
    initialized: AtomicU8,
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
                Ok(_) => {
                    return aligned
                        as *mut u8;
                }

                Err(actual) => {
                    current =
                        actual;
                }
            }
        }
    }

    unsafe fn dealloc(
        &self,
        _ptr: *mut u8,
        _layout: Layout,
    ) {
        // Bump allocator.
        // Nothing is reclaimed yet.
    }
}

#[global_allocator]
static ALLOCATOR:
KernelAllocator =
    KernelAllocator::new();

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
// Dedicated page-table allocator
// ============================================================
//
// This is ONLY for:
//
// - PML4
// - PDPT
// - PD
// - PT
//
// It uses the first 4 MiB of the selected physical region.
//
// The normal kernel heap never touches this region.
// ============================================================

static PAGE_TABLE_NEXT:
AtomicU64 =
    AtomicU64::new(0);

static PAGE_TABLE_END:
AtomicU64 =
    AtomicU64::new(0);

struct PageTableFrameAllocator;

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

            //
            // Page tables must always start zeroed.
            //

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
// Helpers
// ============================================================

fn align_up(
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

fn find_dma_region(
    boot_info: &BootInfo,
) -> Option<(
    u64,
    u64,
)> {
    let required =
        PAGE_TABLE_RESERVE_SIZE
            + DMA_REGION_SIZE;

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

        //
        // Already mapped?
        //

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
            PageTableFrameAllocator;

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
                    PageTableFrameAllocator;

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
    // Reserve page-table memory
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

    PAGE_TABLE_NEXT.store(
        page_table_start,
        Ordering::Release,
    );

    PAGE_TABLE_END.store(
        page_table_end,
        Ordering::Release,
    );

    // ========================================================
    // Identity-map normal kernel/DMA memory
    // ========================================================

    let dma_start =
        page_table_end;

    let dma_end =
        region_end;

    let mut physical =
        dma_start;

    while physical < dma_end {
        if !identity_map_2m(
            physical,
        ) {
            panic!(
                "Rusty: could not identity-map DMA memory",
            );
        }

        physical =
            physical
                .checked_add(
                    LARGE_PAGE_SIZE,
                )
                .expect(
                    "Rusty: DMA mapping overflow",
                );
    }

    // ========================================================
    // Start kernel heap
    // ========================================================

    ALLOCATOR.start.store(
        dma_start as usize,
        Ordering::Release,
    );

    ALLOCATOR.end.store(
        dma_end as usize,
        Ordering::Release,
    );

    ALLOCATOR.initialized.store(
        1,
        Ordering::Release,
    );
}

// ============================================================
// Physical memory API
// ============================================================

pub fn physical_memory_offset()
    -> u64
{
    PHYSICAL_MEMORY_OFFSET.load(
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

// ============================================================
// Allocate normal physical frame
// ============================================================
//
// This comes from the normal kernel heap region.
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
//
// IMPORTANT:
//
// This uses the dedicated page-table reserve.
//
// It does NOT use the global heap.
//
// Your existing address_space.rs calls this for:
//
//     PML4
//     PDPT
//     PD
//     PT
//
// ============================================================

pub fn allocate_page_table_frame()
    -> Option<PhysFrame<Size4KiB>>
{
    let mut allocator =
        PageTableFrameAllocator;

    allocator.allocate_frame()
}

// ============================================================
// Zero a physical frame
// ============================================================

pub fn zero_frame(
    frame: PhysFrame<Size4KiB>,
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
// Copy bytes into a physical frame
// ============================================================

pub fn copy_to_frame(
    frame: PhysFrame<Size4KiB>,
    data: &[u8],
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
// Map one user page
// ============================================================

pub unsafe fn map_user_page(
    mapper:
    &mut OffsetPageTable<'static>,

    virtual_address:
    VirtAddr,

    physical_frame:
    PhysFrame<Size4KiB>,

    writable:
    bool,

    executable:
    bool,
) {
    let mut flags =
        PageTableFlags::PRESENT
            | PageTableFlags::USER_ACCESSIBLE;

    if writable {
        flags |=
            PageTableFlags::WRITABLE;
    }

    if !executable {
        flags |=
            PageTableFlags::NO_EXECUTE;
    }

    let page =
        Page::<Size4KiB>
        ::containing_address(
            virtual_address,
        );

    let mut frame_allocator =
        PageTableFrameAllocator;

    unsafe {
        mapper
            .map_to(
                page,
                physical_frame,
                flags,
                &mut frame_allocator,
            )
            .expect(
                "Rusty: failed to map user page",
            )
            .flush();
    }
}

// ============================================================
// Unmap one page
// ============================================================

pub unsafe fn unmap_page(
    mapper:
    &mut OffsetPageTable<'static>,

    virtual_address:
    VirtAddr,
) {
    let page =
        Page::<Size4KiB>
        ::containing_address(
            virtual_address,
        );

    unsafe {
        if let Ok((
                      _frame,
                      flush,
                  )) =
            mapper.unmap(
                page,
            )
        {
            flush.flush();
        }
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
// Physical page-table access
// ============================================================

unsafe fn page_table_from_frame(
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

// ============================================================
// Direct userspace page walk
// ============================================================

pub unsafe fn user_page_frame(
    level_4_frame:
    PhysFrame<Size4KiB>,

    address:
    u64,
) -> Option<PhysFrame<Size4KiB>> {
    if address < USER_MIN
        || address >= USER_MAX
    {
        return None;
    }

    let page =
        Page::<Size4KiB>
        ::containing_address(
            VirtAddr::new(
                address,
            ),
        );

    // --------------------------------------------------------
    // PML4
    // --------------------------------------------------------

    let p4 =
        unsafe {
            page_table_from_frame(
                level_4_frame,
            )
        };

    let p4_entry =
        &p4[page.p4_index()];

    let p4_flags =
        p4_entry.flags();

    if !p4_flags.contains(
        PageTableFlags::PRESENT,
    ) {
        return None;
    }

    let p3_frame =
        PhysFrame::<Size4KiB>
        ::containing_address(
            p4_entry.addr(),
        );

    // --------------------------------------------------------
    // PDPT
    // --------------------------------------------------------

    let p3 =
        unsafe {
            page_table_from_frame(
                p3_frame,
            )
        };

    let p3_entry =
        &p3[page.p3_index()];

    let p3_flags =
        p3_entry.flags();

    if !p3_flags.contains(
        PageTableFlags::PRESENT,
    ) {
        return None;
    }

    // --------------------------------------------------------
    // 1 GiB huge page
    // --------------------------------------------------------

    if p3_flags.contains(
        PageTableFlags::HUGE_PAGE,
    ) {
        let physical =
            p3_entry
                .addr()
                .as_u64()
                .checked_add(
                    address
                        & 0x3FFF_FFFF,
                )?;

        return PhysFrame::<Size4KiB>
        ::from_start_address(
            PhysAddr::new(
                physical,
            ),
        )
            .ok();
    }

    // --------------------------------------------------------
    // PD
    // --------------------------------------------------------

    let p2_frame =
        PhysFrame::<Size4KiB>
        ::containing_address(
            p3_entry.addr(),
        );

    let p2 =
        unsafe {
            page_table_from_frame(
                p2_frame,
            )
        };

    let p2_entry =
        &p2[page.p2_index()];

    let p2_flags =
        p2_entry.flags();

    if !p2_flags.contains(
        PageTableFlags::PRESENT,
    ) {
        return None;
    }

    // --------------------------------------------------------
    // 2 MiB huge page
    // --------------------------------------------------------

    if p2_flags.contains(
        PageTableFlags::HUGE_PAGE,
    ) {
        let physical =
            p2_entry
                .addr()
                .as_u64()
                .checked_add(
                    address
                        & 0x1F_FFFF,
                )?;

        return PhysFrame::<Size4KiB>
        ::from_start_address(
            PhysAddr::new(
                physical,
            ),
        )
            .ok();
    }

    // --------------------------------------------------------
    // PT
    // --------------------------------------------------------

    let p1_frame =
        PhysFrame::<Size4KiB>
        ::containing_address(
            p2_entry.addr(),
        );

    let p1 =
        unsafe {
            page_table_from_frame(
                p1_frame,
            )
        };

    let p1_entry =
        &p1[page.p1_index()];

    if !p1_entry.flags().contains(
        PageTableFlags::PRESENT,
    ) {
        return None;
    }

    PhysFrame::<Size4KiB>
    ::from_start_address(
        p1_entry.addr(),
    )
        .ok()
}

// ============================================================
// Userspace page flags
// ============================================================

pub unsafe fn user_page_flags(
    level_4_frame:
    PhysFrame<Size4KiB>,

    address:
    u64,
) -> Option<PageTableFlags> {
    if address < USER_MIN
        || address >= USER_MAX
    {
        return None;
    }

    let page =
        Page::<Size4KiB>
        ::containing_address(
            VirtAddr::new(
                address,
            ),
        );

    // --------------------------------------------------------
    // PML4
    // --------------------------------------------------------

    let p4 =
        unsafe {
            page_table_from_frame(
                level_4_frame,
            )
        };

    let p4_entry =
        &p4[page.p4_index()];

    let p4_flags =
        p4_entry.flags();

    if !p4_flags.contains(
        PageTableFlags::PRESENT,
    ) {
        return None;
    }

    let mut effective_flags =
        p4_flags;

    // --------------------------------------------------------
    // PDPT
    // --------------------------------------------------------

    let p3_frame =
        PhysFrame::<Size4KiB>
        ::containing_address(
            p4_entry.addr(),
        );

    let p3 =
        unsafe {
            page_table_from_frame(
                p3_frame,
            )
        };

    let p3_entry =
        &p3[page.p3_index()];

    let p3_flags =
        p3_entry.flags();

    if !p3_flags.contains(
        PageTableFlags::PRESENT,
    ) {
        return None;
    }

    effective_flags &=
        p3_flags;

    // --------------------------------------------------------
    // 1 GiB
    // --------------------------------------------------------

    if p3_flags.contains(
        PageTableFlags::HUGE_PAGE,
    ) {
        return Some(
            effective_flags,
        );
    }

    // --------------------------------------------------------
    // PD
    // --------------------------------------------------------

    let p2_frame =
        PhysFrame::<Size4KiB>
        ::containing_address(
            p3_entry.addr(),
        );

    let p2 =
        unsafe {
            page_table_from_frame(
                p2_frame,
            )
        };

    let p2_entry =
        &p2[page.p2_index()];

    let p2_flags =
        p2_entry.flags();

    if !p2_flags.contains(
        PageTableFlags::PRESENT,
    ) {
        return None;
    }

    effective_flags &=
        p2_flags;

    // --------------------------------------------------------
    // 2 MiB
    // --------------------------------------------------------

    if p2_flags.contains(
        PageTableFlags::HUGE_PAGE,
    ) {
        return Some(
            effective_flags,
        );
    }

    // --------------------------------------------------------
    // PT
    // --------------------------------------------------------

    let p1_frame =
        PhysFrame::<Size4KiB>
        ::containing_address(
            p2_entry.addr(),
        );

    let p1 =
        unsafe {
            page_table_from_frame(
                p1_frame,
            )
        };

    let p1_entry =
        &p1[page.p1_index()];

    let p1_flags =
        p1_entry.flags();

    if !p1_flags.contains(
        PageTableFlags::PRESENT,
    ) {
        return None;
    }

    effective_flags &=
        p1_flags;

    Some(
        effective_flags,
    )
}

// ============================================================
// Validate one userspace page
// ============================================================

pub unsafe fn validate_user_page(
    level_4_frame:
    PhysFrame<Size4KiB>,

    page:
    Page<Size4KiB>,

    writable:
    bool,
) -> bool {
    let address =
        page.start_address()
            .as_u64();

    if address < USER_MIN
        || address >= USER_MAX
    {
        return false;
    }

    let flags =
        match unsafe {
            user_page_flags(
                level_4_frame,
                address,
            )
        } {
            Some(flags) =>
                flags,

            None =>
                return false,
        };

    if !flags.contains(
        PageTableFlags::PRESENT,
    ) {
        return false;
    }

    if !flags.contains(
        PageTableFlags::USER_ACCESSIBLE,
    ) {
        return false;
    }

    if writable
        && !flags.contains(
        PageTableFlags::WRITABLE,
    )
    {
        return false;
    }

    true
}

// ============================================================
// Check whether a userspace page is mapped
// ============================================================

pub unsafe fn user_page_mapped(
    level_4_frame:
    PhysFrame<Size4KiB>,

    page:
    Page<Size4KiB>,
) -> bool {
    unsafe {
        validate_user_page(
            level_4_frame,
            page,
            false,
        )
    }
}

// ============================================================
// Validate complete userspace range
// ============================================================

pub unsafe fn validate_user_range(
    level_4_frame:
    PhysFrame<Size4KiB>,

    ptr:
    u64,

    len:
    usize,

    writable:
    bool,
) -> bool {
    // --------------------------------------------------------
    // Empty range
    // --------------------------------------------------------

    if len == 0 {
        return ptr >= USER_MIN
            && ptr < USER_MAX;
    }

    // --------------------------------------------------------
    // Start
    // --------------------------------------------------------

    if ptr < USER_MIN {
        return false;
    }

    // --------------------------------------------------------
    // End
    // --------------------------------------------------------

    let end =
        match ptr.checked_add(
            len as u64,
        ) {
            Some(value) =>
                value,

            None =>
                return false,
        };

    if end <= ptr {
        return false;
    }

    if end > USER_MAX {
        return false;
    }

    // --------------------------------------------------------
    // First / last page
    // --------------------------------------------------------

    let first_page =
        ptr & !(PAGE_SIZE - 1);

    let last_page =
        (end - 1)
            & !(PAGE_SIZE - 1);

    // --------------------------------------------------------
    // Walk every touched page
    // --------------------------------------------------------

    let mut page_address =
        first_page;

    loop {
        let page =
            Page::<Size4KiB>
            ::containing_address(
                VirtAddr::new(
                    page_address,
                ),
            );

        if !unsafe {
            validate_user_page(
                level_4_frame,
                page,
                writable,
            )
        } {
            return false;
        }

        if page_address
            == last_page
        {
            break;
        }

        page_address =
            match page_address.checked_add(
                PAGE_SIZE,
            ) {
                Some(next) =>
                    next,

                None =>
                    return false,
            };
    }

    true
}

// ============================================================
// Copy from userspace
// ============================================================

pub unsafe fn copy_from_user(
    level_4_frame:
    PhysFrame<Size4KiB>,

    src:
    u64,

    destination:
    &mut [u8],
) -> bool {
    if destination.is_empty() {
        return true;
    }

    //
    // The process must still be the active CR3.
    //

    if current_level_4_frame()
        != level_4_frame
    {
        return false;
    }

    //
    // Validate before dereferencing user memory.
    //

    if !unsafe {
        validate_user_range(
            level_4_frame,
            src,
            destination.len(),
            false,
        )
    } {
        return false;
    }

    //
    // Range is validated.
    //

    unsafe {
        core::ptr::copy_nonoverlapping(
            src as *const u8,
            destination.as_mut_ptr(),
            destination.len(),
        );
    }

    true
}