#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

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
mod terminal;
mod user;
mod wasm;
mod wm;
mod delay;

use bootloader_api::{
    config::{BootloaderConfig, Mapping},
    entry_point, BootInfo,
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


    let selectors =
        cpu::gdt::init();


    programs::test_ring3::run(
        selectors,
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
fn panic(_info: &core::panic::PanicInfo) -> ! {
    serial::write_str("\nRUSTY PANIC\n");
    halt();
}