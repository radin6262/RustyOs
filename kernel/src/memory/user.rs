use x86_64::{
    PhysAddr,
    VirtAddr,
    structures::paging::{
        Mapper,
        OffsetPageTable,
        Page,
        PageSize,
        PageTableFlags,
        PhysFrame,
        Size4KiB,
    },
};

use super::{
    allocator::page_table_from_frame,
    current_level_4_frame,
    USER_MAX,
    USER_MIN,
    PAGE_SIZE,
};

// ============================================================
// Map one userspace page
// ============================================================
//
// User ELF code pages are executable.
//
// For the current Rusty userspace ABI, this function deliberately
// does NOT apply NO_EXECUTE.
//
// User stacks should be mapped separately with their own NX flags.
//
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

    _executable:
    bool,
) {
    let mut flags =
        PageTableFlags::PRESENT
            | PageTableFlags::USER_ACCESSIBLE;

    if writable {
        flags |=
            PageTableFlags::WRITABLE;
    }

    // --------------------------------------------------------
    // Intentionally executable.
    //
    // Do NOT add:
    //
    //     PageTableFlags::NO_EXECUTE
    //
    // to ELF code pages.
    // --------------------------------------------------------

    let page =
        Page::<Size4KiB>
        ::containing_address(
            virtual_address,
        );

    let mut frame_allocator =
        super::allocator::PageTableFrameAllocator;

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
// Direct userspace page walk
// ============================================================

pub unsafe fn user_page_frame(
    level_4_frame:
    PhysFrame<Size4KiB>,

    address:
    u64,
) -> Option<
    PhysFrame<Size4KiB>
> {
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

    // ========================================================
    // PML4
    // ========================================================

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

    // ========================================================
    // PDPT
    // ========================================================

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

    // ========================================================
    // 1 GiB huge page
    // ========================================================

    if p3_flags.contains(
        PageTableFlags::HUGE_PAGE,
    ) {
        // IMPORTANT:
        //
        // Use page.start_address() rather than raw `address`.
        // Otherwise a non-page-aligned address would produce a
        // non-page-aligned physical frame address.
        let page_offset =
            page.start_address()
                .as_u64()
                & 0x3FFF_FFFF;

        let physical =
            p3_entry
                .addr()
                .as_u64()
                .checked_add(
                    page_offset,
                )?;

        return PhysFrame::<Size4KiB>
        ::from_start_address(
            PhysAddr::new(
                physical,
            ),
        )
            .ok();
    }

    // ========================================================
    // PD
    // ========================================================

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

    // ========================================================
    // 2 MiB huge page
    // ========================================================

    if p2_flags.contains(
        PageTableFlags::HUGE_PAGE,
    ) {
        let page_offset =
            page.start_address()
                .as_u64()
                & 0x1F_FFFF;

        let physical =
            p2_entry
                .addr()
                .as_u64()
                .checked_add(
                    page_offset,
                )?;

        return PhysFrame::<Size4KiB>
        ::from_start_address(
            PhysAddr::new(
                physical,
            ),
        )
            .ok();
    }

    // ========================================================
    // PT
    // ========================================================

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

    // ========================================================
    // PML4
    // ========================================================

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

    // ========================================================
    // PDPT
    // ========================================================

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

    // ========================================================
    // 1 GiB huge page
    // ========================================================

    if p3_flags.contains(
        PageTableFlags::HUGE_PAGE,
    ) {
        return Some(
            effective_flags,
        );
    }

    // ========================================================
    // PD
    // ========================================================

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

    // ========================================================
    // 2 MiB huge page
    // ========================================================

    if p2_flags.contains(
        PageTableFlags::HUGE_PAGE,
    ) {
        return Some(
            effective_flags,
        );
    }

    // ========================================================
    // PT
    // ========================================================

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
    // ========================================================
    // Empty range
    // ========================================================

    if len == 0 {
        return ptr >= USER_MIN
            && ptr < USER_MAX;
    }

    // ========================================================
    // Start
    // ========================================================

    if ptr < USER_MIN {
        return false;
    }

    // ========================================================
    // End
    // ========================================================

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

    // ========================================================
    // First / last page
    // ========================================================

    let first_page =
        ptr & !(PAGE_SIZE - 1);

    let last_page =
        (end - 1)
            & !(PAGE_SIZE - 1);

    // ========================================================
    // Walk every touched page
    // ========================================================

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

    // The target process must still be the active CR3.

    if current_level_4_frame()
        != level_4_frame
    {
        return false;
    }

    // Validate before dereferencing userspace.

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

    // Range has been validated.

    unsafe {
        core::ptr::copy_nonoverlapping(
            src as *const u8,
            destination.as_mut_ptr(),
            destination.len(),
        );
    }

    true
}