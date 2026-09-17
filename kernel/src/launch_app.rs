#![no_std]

use crate::{
    process,
    user,
};

/// Launches an ELF application in a new Ring 3 process.
///
/// The ELF bytes are supplied by the caller, which means you can
/// launch any embedded ELF with:
///
/// let selectors = cpu::gdt::init();
///
/// launch_app::run(
///     selectors,
///     include_bytes!("path/to/app.elf"),
/// );
pub fn run(
    selectors: process::Selectors,
    elf: &[u8],
) -> ! {
    // ========================================================
    // Process manager
    // ========================================================

    process::init();

    // ========================================================
    // Create address space
    // ========================================================

    crate::serial::write_str(
        "ELF LAUNCH: creating address space\n",
    );

    let mut user_space =
        user::UserAddressSpace::new();

    crate::serial::write_str(
        "ELF LAUNCH: address space created\n",
    );

    // ========================================================
    // Load ELF
    // ========================================================

    crate::serial::write_str(
        "ELF LAUNCH: loading ELF\n",
    );

    let loaded =
        match crate::elf::load_elf(
            elf,
            &mut user_space,
        ) {
            Ok(loaded) => loaded,

            Err(error) => {
                crate::serial::write_str(
                    "ELF LAUNCH: load_elf FAILED\n",
                );

                match error {
                    crate::elf::ElfError::ParseError(
                        message,
                    ) => {
                        crate::serial::write_str(
                            "ELF error: ParseError: ",
                        );

                        crate::serial::write_str(
                            message,
                        );
                    }

                    crate::elf::ElfError::InvalidFormat(
                        message,
                    ) => {
                        crate::serial::write_str(
                            "ELF error: InvalidFormat: ",
                        );

                        crate::serial::write_str(
                            message,
                        );
                    }

                    crate::elf::ElfError::AllocationFailed => {
                        crate::serial::write_str(
                            "ELF error: AllocationFailed",
                        );
                    }
                }

                crate::serial::write_str(
                    "\n",
                );

                loop {
                    core::hint::spin_loop();
                }
            }
        };

    crate::serial::write_str(
        "ELF LAUNCH: ELF loaded\n",
    );

    // ========================================================
    // Entry point
    // ========================================================

    crate::serial::write_str(
        "ELF LAUNCH: entry=",
    );

    crate::serial::write_hex(
        loaded.entry_point,
    );

    crate::serial::write_str(
        "\n",
    );

    // ========================================================
    // User stack
    // ========================================================

    crate::serial::write_str(
        "ELF LAUNCH: mapping stack\n",
    );

    user_space.map_user_stack(
        user::address_space::USER_STACK_ADDRESS,
    );

    crate::serial::write_str(
        "ELF LAUNCH: stack mapped\n",
    );

    // ========================================================
    // Create process
    // ========================================================

    crate::serial::write_str(
        "ELF LAUNCH: creating process\n",
    );

    let pid =
        process::create_elf_process(
            user_space,
            loaded.entry_point,
            selectors,
        );

    crate::serial::write_str(
        "ELF LAUNCH: process created pid=",
    );

    crate::serial::write_usize(
        pid as usize,
    );

    crate::serial::write_str(
        "\n",
    );

    // ========================================================
    // Select ELF process
    // ========================================================

    crate::serial::write_str(
        "ELF LAUNCH: setting current\n",
    );

    if !process::set_current(
        pid,
    ) {
        crate::serial::write_str(
            "ELF LAUNCH: set_current FAILED\n",
        );

        loop {
            core::hint::spin_loop();
        }
    }

    crate::serial::write_str(
        "ELF LAUNCH: current process set\n",
    );

    // ========================================================
    // Enter Ring 3
    // ========================================================

    crate::serial::write_str(
        "ELF LAUNCH: entering ring3\n",
    );

    unsafe {
        process::enter_current(
            selectors,
        );
    }
}