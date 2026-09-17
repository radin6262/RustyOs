use crate::memory;

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

// ============================================================
// Userspace virtual addresses
// ============================================================
//
// ELF linker:
//
//     . = 0x4000000000;
//
// Therefore the ELF code MUST start at:
//
//     0x40_0000_0000
//
// This is:
//
//     PML4[0]
//     PDPT[256]
//
// The old stack address:
//
//     0x80_0000_0000
//
// is PML4[1] / PDPT[0].
//
// That branch may contain the kernel's direct physical-memory
// mapping supplied by the bootloader.
//
// Therefore the user stack is moved to:
//
//     0x60_0000_0000
//
// which is:
//
//     PML4[0]
//     PDPT[384]
//
// Both userspace regions now live inside the same private
// PML4[0] branch.
//
// ============================================================

pub const USER_CODE_ADDRESS: u64 =
    0x0000_0040_0000_0000;

pub const USER_STACK_ADDRESS: u64 =
    0x0000_0060_0000_0000;

// ============================================================
// Constants
// ============================================================

const PAGE_SIZE: u64 =
    Size4KiB::SIZE;

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
        crate::serial::write_str(
            "USER AS: creating address space\n",
        );

        // ----------------------------------------------------
        // Allocate a completely new PML4.
        // ----------------------------------------------------

        let level_4_frame =
            memory::allocate_page_table_frame()
                .expect(
                    "Rusty: failed to allocate user PML4",
                );

        crate::serial::write_str(
            "USER AS: new PML4 PA=",
        );

        crate::serial::write_hex(
            level_4_frame
                .start_address()
                .as_u64(),
        );

        crate::serial::write_str(
            "\n",
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

        // ----------------------------------------------------
        // Start completely empty.
        // ----------------------------------------------------

        new_level_4.zero();

        // ----------------------------------------------------
        // Copy kernel mappings.
        //
        // IMPORTANT:
        //
        // PML4[1] is preserved exactly because it may contain
        // the kernel's direct physical-memory mapping.
        //
        // Only PML4[0] is rebuilt privately.
        // ----------------------------------------------------

        copy_kernel_mappings(
            new_level_4,
        );

        // ----------------------------------------------------
        // Build mapper.
        // ----------------------------------------------------

        let mapper =
            unsafe {
                OffsetPageTable::new(
                    new_level_4,
                    VirtAddr::new(
                        memory::physical_memory_offset(),
                    ),
                )
            };

        // ====================================================
        // Verify CODE is initially unmapped.
        // ====================================================

        let code_va =
            VirtAddr::new(
                USER_CODE_ADDRESS,
            );

        crate::serial::write_str(
            "USER AS: code VA=",
        );

        crate::serial::write_hex(
            USER_CODE_ADDRESS,
        );

        crate::serial::write_str(
            " PML4=0 PDPT=256\n",
        );

        if let Some(pa) =
            mapper.translate_addr(
                code_va,
            )
        {
            crate::serial::write_str(
                "USER AS: ERROR code already mapped PA=",
            );

            crate::serial::write_hex(
                pa.as_u64(),
            );

            crate::serial::write_str(
                "\n",
            );

            panic!(
                "Rusty: user code mapping leaked from previous process",
            );
        }

        crate::serial::write_str(
            "USER AS: code VA empty\n",
        );

        // ====================================================
        // Verify STACK is initially unmapped.
        // ====================================================

        let stack_va =
            VirtAddr::new(
                USER_STACK_ADDRESS,
            );

        crate::serial::write_str(
            "USER AS: stack VA=",
        );

        crate::serial::write_hex(
            USER_STACK_ADDRESS,
        );

        crate::serial::write_str(
            " PML4=0 PDPT=384\n",
        );

        if let Some(pa) =
            mapper.translate_addr(
                stack_va,
            )
        {
            crate::serial::write_str(
                "USER AS: ERROR stack already mapped PA=",
            );

            crate::serial::write_hex(
                pa.as_u64(),
            );

            crate::serial::write_str(
                "\n",
            );

            panic!(
                "Rusty: user stack mapping leaked from previous process",
            );
        }

        crate::serial::write_str(
            "USER AS: stack VA empty\n",
        );

        // ----------------------------------------------------
        // Print physical-memory offset so we can see exactly
        // where the bootloader mapped RAM.
        // ----------------------------------------------------

        crate::serial::write_str(
            "USER AS: physical memory offset=",
        );

        crate::serial::write_hex(
            memory::physical_memory_offset(),
        );

        crate::serial::write_str(
            "\n",
        );

        crate::serial::write_str(
            "USER AS: address space ready\n",
        );

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
            address & (PAGE_SIZE - 1),
            0,
            "Rusty: user code must be page aligned",
        );

        assert!(
            !code.is_empty(),
            "Rusty: user code cannot be empty",
        );

        assert!(
            code.len()
                <= PAGE_SIZE as usize,
            "Rusty: user code exceeds one page",
        );

        crate::serial::write_str(
            "USER CODE: VA=",
        );

        crate::serial::write_hex(
            address,
        );

        crate::serial::write_str(
            "\n",
        );

        // ----------------------------------------------------
        // Allocate a NEW physical frame.
        //
        // This must never come from an inherited user mapping.
        // ----------------------------------------------------

        let frame =
            memory::allocate_user_frame()
                .expect(
                    "Rusty: failed to allocate user code frame",
                );

        crate::serial::write_str(
            "USER CODE: NEW PA=",
        );

        crate::serial::write_hex(
            frame
                .start_address()
                .as_u64(),
        );

        crate::serial::write_str(
            "\n",
        );

        memory::zero_frame(
            frame,
        );

        let virtual_address =
            VirtAddr::new(
                address,
            );

        let page =
            Page::<Size4KiB>
            ::containing_address(
                virtual_address,
            );

        // ----------------------------------------------------
        // Executable user page.
        //
        // PRESENT
        // USER
        //
        // No WRITABLE.
        // No NX.
        // ----------------------------------------------------

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

        crate::serial::write_str(
            "USER CODE: page mapped\n",
        );

        // ----------------------------------------------------
        // Copy ELF contents.
        // ----------------------------------------------------

        memory::copy_to_frame(
            frame,
            code,
        );

        crate::serial::write_str(
            "USER CODE: copied\n",
        );

        // ----------------------------------------------------
        // Verify exact physical frame.
        // ----------------------------------------------------

        match self.mapper.translate_addr(
            virtual_address,
        ) {
            Some(actual) => {
                let expected =
                    frame
                        .start_address()
                        .as_u64();

                if actual.as_u64()
                    != expected
                {
                    crate::serial::write_str(
                        "USER CODE: WRONG PHYSICAL FRAME\n",
                    );

                    crate::serial::write_str(
                        "  expected=",
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
                        "Rusty: user code mapped to wrong frame",
                    );
                }
            }

            None => {
                crate::serial::write_str(
                    "USER CODE: TRANSLATION FAILED\n",
                );

                panic!(
                    "Rusty: user code mapping disappeared",
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
            address & (PAGE_SIZE - 1),
            0,
            "Rusty: user stack must be page aligned",
        );

        crate::serial::write_str(
            "USER STACK: VA=",
        );

        crate::serial::write_hex(
            address,
        );

        crate::serial::write_str(
            "\n",
        );

        let virtual_address =
            VirtAddr::new(
                address,
            );

        let page =
            Page::<Size4KiB>
            ::containing_address(
                virtual_address,
            );

        // ----------------------------------------------------
        // A fresh process MUST NOT inherit a stack mapping.
        // ----------------------------------------------------

        if let Some(existing) =
            self.mapper.translate_addr(
                virtual_address,
            )
        {
            crate::serial::write_str(
                "USER STACK: ERROR INHERITED MAPPING PA=",
            );

            crate::serial::write_hex(
                existing.as_u64(),
            );

            crate::serial::write_str(
                "\n",
            );

            panic!(
                "Rusty: user stack leaked from previous process",
            );
        }

        // ----------------------------------------------------
        // Allocate completely fresh physical stack frame.
        // ----------------------------------------------------

        let frame =
            memory::allocate_user_frame()
                .expect(
                    "Rusty: failed to allocate user stack frame",
                );

        crate::serial::write_str(
            "USER STACK: NEW PA=",
        );

        crate::serial::write_hex(
            frame
                .start_address()
                .as_u64(),
        );

        crate::serial::write_str(
            "\n",
        );

        memory::zero_frame(
            frame,
        );

        // ----------------------------------------------------
        // Stack:
        //
        // PRESENT
        // WRITABLE
        // USER
        // NX
        // ----------------------------------------------------

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
        // Verify exact physical frame.
        // ----------------------------------------------------

        match self.mapper.translate_addr(
            virtual_address,
        ) {
            Some(actual) => {
                let expected =
                    frame
                        .start_address()
                        .as_u64();

                if actual.as_u64()
                    != expected
                {
                    crate::serial::write_str(
                        "USER STACK: WRONG PHYSICAL FRAME\n",
                    );

                    crate::serial::write_str(
                        "  expected=",
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
                        "Rusty: user stack mapped to wrong frame",
                    );
                }
            }

            None => {
                crate::serial::write_str(
                    "USER STACK: TRANSLATION FAILED\n",
                );

                panic!(
                    "Rusty: user stack mapping disappeared",
                );
            }
        }

        crate::serial::write_str(
            "USER STACK: mapped successfully\n",
        );
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
                + PAGE_SIZE,
        )
    }
}

// ============================================================
// Copy kernel mappings
// ============================================================
//
// IMPORTANT:
//
// DO NOT destroy PML4[1].
//
// PML4[1] may contain:
//
//     physical_memory_offset
//
// from the bootloader's direct physical-memory mapping.
//
// We therefore:
//
//     PML4[0] -> PRIVATE COPY
//     PML4[1] -> SHARED KERNEL MAPPING
//     PML4[2..511] -> SHARED KERNEL MAPPINGS
//
// Userspace is entirely inside PML4[0]:
//
//     CODE:
//         PML4[0] / PDPT[256]
//
//     STACK:
//         PML4[0] / PDPT[384]
//
// ============================================================

fn copy_kernel_mappings(
    new_level_4: &mut PageTable,
) {
    // --------------------------------------------------------
    // Read current kernel CR3.
    // --------------------------------------------------------

    let (
        current_l4_frame,
        _,
    ) =
        Cr3::read();

    crate::serial::write_str(
        "USER AS: source CR3=",
    );

    crate::serial::write_hex(
        current_l4_frame
            .start_address()
            .as_u64(),
    );

    crate::serial::write_str(
        "\n",
    );

    // --------------------------------------------------------
    // Access current PML4 through the physical-memory mapping.
    // --------------------------------------------------------

    let current_l4_virtual =
        memory::physical_to_virtual(
            current_l4_frame
                .start_address()
                .as_u64(),
        );

    let current_level_4:
        &'static PageTable =
        unsafe {
            &*(
                current_l4_virtual
                    as *const PageTable
            )
        };

    // --------------------------------------------------------
    // PML4[1..511]
    //
    // Copy these directly.
    //
    // MOST IMPORTANTLY:
    //
    // PML4[1] is NOT cleared or rebuilt.
    //
    // This keeps the kernel's physical-memory mapping alive
    // after CR3 switches.
    // --------------------------------------------------------

    for index in 1..512 {
        new_level_4[index] =
            current_level_4[index]
                .clone();
    }

    crate::serial::write_str(
        "USER AS: preserved PML4[1..511]\n",
    );

    // --------------------------------------------------------
    // PML4[0]
    //
    // This branch contains our userspace regions.
    //
    // Give the process a PRIVATE copy.
    // --------------------------------------------------------

    rebuild_user_pml4_zero(
        new_level_4,
        current_level_4,
    );

    crate::serial::write_str(
        "USER AS: PML4[0] isolated\n",
    );
}

// ============================================================
// Rebuild PML4[0]
// ============================================================
//
// Keeps kernel mappings from the existing P3, but removes:
//
//     1. every inherited USER_ACCESSIBLE P3 entry
//     2. PDPT[256] -> user code
//     3. PDPT[384] -> user stack
//
// This guarantees a new process gets fresh userspace page
// tables instead of inheriting the previous process's pages.
//
// ============================================================

fn rebuild_user_pml4_zero(
    new_level_4: &mut PageTable,
    current_level_4: &PageTable,
) {
    let original_entry =
        current_level_4[0].clone();

    // --------------------------------------------------------
    // Allocate a PRIVATE P3.
    // --------------------------------------------------------

    let new_p3_frame =
        memory::allocate_page_table_frame()
            .expect(
                "Rusty: failed to allocate private user P3",
            );

    crate::serial::write_str(
        "USER AS: private P3 PA=",
    );

    crate::serial::write_hex(
        new_p3_frame
            .start_address()
            .as_u64(),
    );

    crate::serial::write_str(
        "\n",
    );

    let new_p3_virtual =
        memory::physical_to_virtual(
            new_p3_frame
                .start_address()
                .as_u64(),
        );

    let new_p3_table:
        &'static mut PageTable =
        unsafe {
            &mut *(
                new_p3_virtual
                    as *mut PageTable
            )
        };

    new_p3_table.zero();

    // --------------------------------------------------------
    // Copy kernel P3 entries if the old PML4[0] exists.
    // --------------------------------------------------------

    if original_entry
        .flags()
        .contains(
            PageTableFlags::PRESENT,
        )
    {
        let current_p3_frame =
            PhysFrame::<Size4KiB>
            ::containing_address(
                original_entry.addr(),
            );

        let current_p3_virtual =
            memory::physical_to_virtual(
                current_p3_frame
                    .start_address()
                    .as_u64(),
            );

        let current_p3_table:
            &'static PageTable =
            unsafe {
                &*(
                    current_p3_virtual
                        as *const PageTable
                )
            };

        // ----------------------------------------------------
        // Copy only kernel mappings.
        //
        // Any USER_ACCESSIBLE P3 entry belongs to the previous
        // process and must NOT leak into this address space.
        // ----------------------------------------------------

        for index in 0..512 {
            // -----------------------------------------------
            // CODE region.
            // -----------------------------------------------

            if index
                == 256
            {
                continue;
            }

            // -----------------------------------------------
            // STACK region.
            // -----------------------------------------------

            if index
                == 384
            {
                continue;
            }

            let entry =
                &current_p3_table[
                    index
                    ];

            // -----------------------------------------------
            // Do not inherit any userspace branch.
            // -----------------------------------------------

            if entry
                .flags()
                .contains(
                    PageTableFlags::USER_ACCESSIBLE,
                )
            {
                continue;
            }

            new_p3_table[index] =
                entry.clone();
        }
    }

    // --------------------------------------------------------
    // PML4 flags.
    //
    // Userspace requires USER_ACCESSIBLE at PML4.
    // --------------------------------------------------------

    let mut pml4_flags =
        original_entry.flags();

    pml4_flags.insert(
        PageTableFlags::PRESENT
            | PageTableFlags::USER_ACCESSIBLE,
    );

    // --------------------------------------------------------
    // The userspace code is beneath this PML4 branch.
    //
    // Therefore PML4[0] must NOT have NX.
    // --------------------------------------------------------

    pml4_flags.remove(
        PageTableFlags::NO_EXECUTE,
    );

    // --------------------------------------------------------
    // Install private P3.
    // --------------------------------------------------------

    new_level_4[0]
        .set_addr(
            new_p3_frame
                .start_address(),
            pml4_flags,
        );

    crate::serial::write_str(
        "USER AS: PML4[0] user regions cleared\n",
    );

    crate::serial::write_str(
        "USER AS: code PDPT[256] cleared\n",
    );

    crate::serial::write_str(
        "USER AS: stack PDPT[384] cleared\n",
    );
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