use x86_64::{
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
        Size4KiB,
        Translate,
    },
};

use crate::memory;

// ============================================================
// Userspace virtual addresses
// ============================================================

pub const USER_CODE_ADDRESS: u64 =
    0x0000_4000_0000;

pub const USER_STACK_ADDRESS: u64 =
    0x0000_8000_0000;

// ============================================================
// User address space
// ============================================================

pub struct UserAddressSpace {
    level_4_frame:
        PhysFrame<Size4KiB>,

    mapper:
        OffsetPageTable<'static>,
}

impl UserAddressSpace {
    // ========================================================
    // Create
    // ========================================================

    pub fn new() -> Self {
        let level_4_frame =
            memory::allocate_page_table_frame()
                .expect(
                    "Rusty: failed to allocate user PML4",
                );

        let level_4_virtual =
            memory::physical_to_virtual(
                level_4_frame
                    .start_address()
                    .as_u64(),
            );

        let new_level_4:
            &'static mut PageTable =
            unsafe {
                &mut *(
                    level_4_virtual
                        as *mut PageTable
                )
            };

        // allocate_page_table_frame() returns
        // a zeroed page, so userspace starts empty.
        unsafe {
            copy_kernel_mappings(
                new_level_4,
            );
        }

        let mapper =
            unsafe {
                OffsetPageTable::new(
                    new_level_4,
                    VirtAddr::new(
                        memory::physical_memory_offset(),
                    ),
                )
            };

        Self {
            level_4_frame,
            mapper,
        }
    }

    // ========================================================
    // Map user code
    // ========================================================

    pub fn map_user_code(
        &mut self,
        address: u64,
        code: &[u8],
    ) {
        assert_eq!(
            address
                & (Size4KiB::SIZE - 1),
            0,
            "Rusty: user code must be page aligned",
        );

        assert!(
            !code.is_empty(),
            "Rusty: user code cannot be empty",
        );

        assert!(
            code.len()
                <= Size4KiB::SIZE as usize,
            "Rusty: user code exceeds one page",
        );

        let frame =
            memory::allocate_user_frame()
                .expect(
                    "Rusty: failed to allocate user code frame",
                );

        let virtual_address =
            VirtAddr::new(address);

        let page =
            Page::<Size4KiB>
            ::containing_address(
                virtual_address,
            );

        let flags =
            PageTableFlags::PRESENT
                | PageTableFlags::USER_ACCESSIBLE;

        let mut frame_allocator =
            UserPageTableAllocator;

        unsafe {
            self.mapper
                .map_to(
                    page,
                    frame,
                    flags,
                    &mut frame_allocator,
                )
                .expect(
                    "Rusty: failed to map user code page",
                )
                .flush();
        }

        memory::copy_to_frame(
            frame,
            code,
        );

        // ----------------------------------------------------
        // Verify the mapping immediately.
        // ----------------------------------------------------

        let translated =
            self.mapper
                .translate_addr(
                    virtual_address,
                );

        let expected =
            frame
                .start_address()
                .as_u64();

        match translated {
            Some(actual) => {
                if actual.as_u64()
                    != expected
                {
                    crate::serial::write_str(
                        "user map: WRONG PHYSICAL FRAME\n",
                    );

                    crate::serial::write_str(
                        "user map: expected=",
                    );
                    crate::serial::write_hex(
                        expected,
                    );

                    crate::serial::write_str(
                        " actual=",
                    );
                    crate::serial::write_hex(
                        actual.as_u64(),
                    );

                    crate::serial::write_str(
                        "\n",
                    );

                    panic!(
                        "Rusty: user code mapping translated to wrong frame",
                    );
                }
            }

            None => {
                crate::serial::write_str(
                    "user map: TRANSLATION FAILED\n",
                );

                crate::serial::write_str(
                    "user map: PML4=",
                );
                crate::serial::write_hex(
                    self.level_4_frame
                        .start_address()
                        .as_u64(),
                );

                crate::serial::write_str(
                    " VA=",
                );
                crate::serial::write_hex(
                    address,
                );

                crate::serial::write_str(
                    "\n",
                );

                panic!(
                    "Rusty: user code mapping disappeared immediately",
                );
            }
        }
    }

    // ========================================================
    // Map user stack
    // ========================================================

    pub fn map_user_stack(
        &mut self,
        address: u64,
    ) {
        assert_eq!(
            address
                & (Size4KiB::SIZE - 1),
            0,
            "Rusty: user stack must be page aligned",
        );

        let frame =
            memory::allocate_user_frame()
                .expect(
                    "Rusty: failed to allocate user stack frame",
                );

        let virtual_address =
            VirtAddr::new(address);

        let page =
            Page::<Size4KiB>
            ::containing_address(
                virtual_address,
            );

        let flags =
            PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::USER_ACCESSIBLE
                | PageTableFlags::NO_EXECUTE;

        let mut frame_allocator =
            UserPageTableAllocator;

        unsafe {
            self.mapper
                .map_to(
                    page,
                    frame,
                    flags,
                    &mut frame_allocator,
                )
                .expect(
                    "Rusty: failed to map user stack page",
                )
                .flush();
        }

        // ----------------------------------------------------
        // Verify stack mapping too.
        // ----------------------------------------------------

        let translated =
            self.mapper
                .translate_addr(
                    virtual_address,
                );

        let expected =
            frame
                .start_address()
                .as_u64();

        match translated {
            Some(actual) => {
                if actual.as_u64()
                    != expected
                {
                    panic!(
                        "Rusty: user stack mapping translated to wrong frame",
                    );
                }
            }

            None => {
                panic!(
                    "Rusty: user stack mapping disappeared immediately",
                );
            }
        }
    }

    // ========================================================
    // Accessors
    // ========================================================

    pub fn level_4_frame(
        &self,
    ) -> PhysFrame<Size4KiB> {
        self.level_4_frame
    }

    pub fn mapper_mut(
        &mut self,
    ) -> &mut OffsetPageTable<'static> {
        &mut self.mapper
    }

    pub fn user_code_address(
        &self,
    ) -> VirtAddr {
        VirtAddr::new(
            USER_CODE_ADDRESS,
        )
    }

    pub fn user_stack_top(
        &self,
    ) -> VirtAddr {
        VirtAddr::new(
            USER_STACK_ADDRESS
                + Size4KiB::SIZE,
        )
    }
}

// ============================================================
// Copy kernel mappings
// ============================================================

unsafe fn copy_kernel_mappings(
    new_level_4: &mut PageTable,
) {
    let (current_l4_frame, _) = Cr3::read();

    let current_l4_virtual = memory::physical_to_virtual(
        current_l4_frame.start_address().as_u64(),
    );

    let current_level_4: &'static PageTable = unsafe {
        &*(current_l4_virtual as *const PageTable)
    };

    // 1. Copy upper kernel entries (PML4 entries 1..512)
    for index in 1..512 {
        new_level_4[index] = current_level_4[index].clone();
    }

    // 2. Deep-copy PML4 entry 0 so kernel low-memory mappings (DMA, heap, MMIO)
    //    remain present while granting isolated space for user code and stack.
    let p4_entry_0 = &current_level_4[0];
    if p4_entry_0.flags().contains(PageTableFlags::PRESENT) {
        if let Some(new_p3_frame) = memory::allocate_page_table_frame() {
            let new_p3_virt = memory::physical_to_virtual(
                new_p3_frame.start_address().as_u64(),
            );
            let new_p3_table: &'static mut PageTable = unsafe {
                &mut *(new_p3_virt as *mut PageTable)
            };

            let current_p3_frame = PhysFrame::<Size4KiB>::containing_address(
                p4_entry_0.addr(),
            );
            let current_p3_virt = memory::physical_to_virtual(
                current_p3_frame.start_address().as_u64(),
            );
            let current_p3_table: &'static PageTable = unsafe {
                &*(current_p3_virt as *const PageTable)
            };

            for index in 0..512 {
                new_p3_table[index] = current_p3_table[index].clone();
            }

            new_level_4[0].set_addr(
                new_p3_frame.start_address(),
                p4_entry_0.flags(),
            );
        }
    }
}

// ============================================================
// Frame allocator for page-table construction
// ============================================================

struct UserPageTableAllocator;

unsafe impl FrameAllocator<Size4KiB>
for UserPageTableAllocator
{
    fn allocate_frame(
        &mut self,
    ) -> Option<PhysFrame<Size4KiB>> {
        memory::allocate_page_table_frame()
    }
}