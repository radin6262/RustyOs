#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

extern crate alloc;

mod cpu;
mod font;
mod graphics;
mod input;
mod interrupts;
mod memory;
mod process;
mod programs;
mod serial;
mod syscall;
mod user;
mod wm;
mod delay;
mod elf;
mod launch_app;
mod xhci_pci;
mod usb;
mod ax_sync;

use bootloader_api::{BootInfo,
                     config::{BootloaderConfig, Mapping},
                     entry_point,
};

// ============================================================
// Bootloader configuration
// ============================================================

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();

    config.mappings.physical_memory = Some(Mapping::Dynamic);

    config
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

// ============================================================
// Kernel entry
// ============================================================

fn kernel_main(
    boot_info: &'static mut BootInfo,
) -> ! {
    programs::boothandler::run_boot_sequence(
        boot_info,
    );

    // if let Some(mouse) = crate::input::poll_mouse() {
    //     crate::serial::write_str("MOUSE EVENT\n");
    //
    //     crate::serial::write_str("dx=");
    //     crate::serial::write_hex(
    //         mouse.dx as i64 as u64,
    //     );
    //
    //     crate::serial::write_str(" dy=");
    //     crate::serial::write_hex(
    //         mouse.dy as i64 as u64,
    //     );
    //
    //     crate::serial::write_str(" buttons=");
    //     crate::serial::write_usize(
    //         mouse.buttons as usize,
    //     );
    //
    //     crate::serial::write_str("\n");
    // }

    let selectors =
        cpu::gdt::init();

    launch_app::run(
        selectors,
        include_bytes!("../../user/terminal/Terminal"),
    );



    // Kernel idle loop once boot sequence completes
    halt();
}

// ============================================================
// Halt
// ============================================================

fn halt() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

// ============================================================
// Panic
// ============================================================

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    crate::serial::write_str(
        "\n\nRUSTY PANIC\n",
    );

    if let Some(location) = info.location() {
        crate::serial::write_str(
            "file: ",
        );
        crate::serial::write_str(
            location.file(),
        );

        crate::serial::write_str(
            "\nline: ",
        );
        crate::serial::write_usize(
            location.line() as usize,
        );

        crate::serial::write_str(
            "\ncolumn: ",
        );
        crate::serial::write_usize(
            location.column() as usize,
        );

        crate::serial::write_str(
            "\n",
        );
    }

    loop {
        core::hint::spin_loop();
    }
}