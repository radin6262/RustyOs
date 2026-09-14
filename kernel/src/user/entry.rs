use x86_64::{
    instructions::interrupts,
    registers::{
        control::{
            Cr3,
            Cr3Flags,
        },
        rflags::RFlags,
    },
    structures::idt::InterruptStackFrameValue,
    PrivilegeLevel,
};

use crate::cpu::gdt::Selectors;

use super::address_space::UserAddressSpace;

// ============================================================
// Enter Ring 3
// ============================================================

pub unsafe fn enter(
    user_space: &UserAddressSpace,
    selectors: Selectors,
) -> ! {
    // --------------------------------------------------------
    // Disable interrupts until Rusty has a userspace IDT path.
    // --------------------------------------------------------

    interrupts::disable();

    // --------------------------------------------------------
    // User instruction pointer.
    // --------------------------------------------------------

    let instruction_pointer =
        user_space.user_code_address();

    // --------------------------------------------------------
    // User stack pointer.
    // --------------------------------------------------------

    let stack_pointer =
        user_space.user_stack_top();

    // --------------------------------------------------------
    // Verify Ring 3 selectors.
    // --------------------------------------------------------

    assert_eq!(
        selectors.user_code.rpl(),
        PrivilegeLevel::Ring3,
        "Rusty: user code selector is not Ring 3",
    );

    assert_eq!(
        selectors.user_data.rpl(),
        PrivilegeLevel::Ring3,
        "Rusty: user data selector is not Ring 3",
    );

    // --------------------------------------------------------
    // Build RFLAGS.
    //
    // Bit 1 must always be set.
    //
    // IF is intentionally clear for now.
    // --------------------------------------------------------

    let rflags =
        RFlags::from_bits_retain(
            1 << 1,
        );

    // --------------------------------------------------------
    // Construct the frame that IRETQ will consume.
    //
    // This MUST happen before changing CR3.
    // --------------------------------------------------------

    let frame =
        InterruptStackFrameValue::new(
            instruction_pointer,
            selectors.user_code,
            rflags,
            stack_pointer,
            selectors.user_data,
        );

    // --------------------------------------------------------
    // Switch to the user's page table.
    // --------------------------------------------------------

    unsafe {
        Cr3::write(
            user_space.level_4_frame(),
            Cr3Flags::empty(),
        );
    }

    // --------------------------------------------------------
    // Ring 0 -> Ring 3
    // --------------------------------------------------------

    unsafe {
        frame.iretq();
    }
}