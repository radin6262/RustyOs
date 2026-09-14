#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod font;
mod graphics;
mod input;
mod memory;
mod programs;
mod terminal;
mod syscall;
mod user;
mod process;
mod cpu;
mod serial;
mod interrupts;
mod wasm;

use bootloader_api::{
    config::{
        BootloaderConfig,
        Mapping,
    },
    entry_point,
    BootInfo,
};

// ============================================================
// Bootloader configuration
// ============================================================

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config =
        BootloaderConfig::new_default();

    config.mappings.physical_memory =
        Some(Mapping::Dynamic);

    config
};

entry_point!(
    kernel_main,
    config = &BOOTLOADER_CONFIG
);

// ============================================================
// Kernel entry
// ============================================================

fn kernel_main(
    boot_info: &'static mut BootInfo,
) -> ! {
    // ========================================================
    // Memory
    // ========================================================

    memory::init(
        &*boot_info,
    );

    // ========================================================
    // Serial
    // ========================================================

    serial::init();

    serial::write_str(
        "Rusty kernel started\n",
    );

    // ========================================================
    // CPU / GDT / TSS
    // ========================================================

    serial::write_str(
        "Initializing GDT/TSS\n",
    );

    let _selectors =
        cpu::gdt::init();

    serial::write_str(
        "GDT + TSS initialized\n",
    );

    // ========================================================
    // IDT
    // ========================================================

    serial::write_str(
        "Initializing IDT\n",
    );

    interrupts::init();

    serial::write_str(
        "IDT initialized\n",
    );

    // ========================================================
    // Graphics
    // ========================================================

    let Some(framebuffer) =
        boot_info.framebuffer.as_mut()
    else {
        serial::write_str(
            "ERROR: framebuffer unavailable\n",
        );

        halt();
    };

    graphics::init(
        framebuffer,
    );

    // ========================================================
    // Initial debug screen
    // ========================================================

    graphics::begin_frame();

    graphics::draw_string(
        30,
        30,
        "RUSTY KERNEL",
        graphics::Color::WHITE,
        2,
    );

    graphics::draw_string(
        30,
        75,
        "MEMORY OK",
        graphics::Color::GREEN,
        1,
    );

    graphics::draw_string(
        30,
        100,
        "GRAPHICS OK",
        graphics::Color::GREEN,
        1,
    );

    graphics::present(
        framebuffer,
    );

    // ========================================================
    // PCI / xHCI
    // ========================================================

    let Some(bar0) =
        input::usb::find_xhci_bar0()
    else {
        graphics::begin_frame();

        graphics::draw_string(
            30,
            140,
            "XHCI NOT FOUND",
            graphics::Color::TEXT_DIM,
            1,
        );

        graphics::present(
            framebuffer,
        );

        serial::write_str(
            "ERROR: xHCI controller not found\n",
        );

        halt();
    };

    graphics::begin_frame();

    graphics::draw_string(
        30,
        140,
        "XHCI FOUND",
        graphics::Color::GREEN,
        1,
    );

    graphics::present(
        framebuffer,
    );

    serial::write_str(
        "xHCI found\n",
    );

    // ========================================================
    // USB
    // ========================================================

    input::init(
        bar0,
    );

    graphics::begin_frame();

    graphics::draw_string(
        30,
        170,
        "USB INITIALIZED",
        graphics::Color::GREEN,
        1,
    );

    if input::has_keyboard() {
        graphics::draw_string(
            30,
            195,
            "KEYBOARD DETECTED",
            graphics::Color::GREEN,
            1,
        );

        serial::write_str(
            "USB keyboard detected\n",
        );
    } else {
        graphics::draw_string(
            30,
            195,
            "KEYBOARD NOT FOUND",
            graphics::Color::TEXT_DIM,
            1,
        );

        serial::write_str(
            "USB keyboard not found\n",
        );
    }

    graphics::present(
        framebuffer,
    );

    // ========================================================
    // Cooperative process tests
    // ========================================================

    /*
    // ========================================================
    // Process manager
    // ========================================================

    serial::write_str(
        "Initializing process manager\n",
    );

    process::init();

    serial::write_str(
        "Process manager initialized\n",
    );

    // ========================================================
    // Process 1
    // ========================================================

    serial::write_str(
        "Creating process 1 address space\n",
    );

    let mut user_space_1 =
        user::UserAddressSpace::new();

    serial::write_str(
        "Process 1 address space created\n",
    );

    let user_code_1: [u8; 28] = [
        // SYS_TEST
        0xB8, 0x03, 0x00, 0x00, 0x00,
        0xBF, 0x11, 0x11, 0x00, 0x00,
        0xCD, 0x80,

        // SYS_YIELD
        0xB8, 0x01, 0x00, 0x00, 0x00,
        0xCD, 0x80,

        // SYS_EXIT
        0xB8, 0x00, 0x00, 0x00, 0x00,
        0x31, 0xFF,
        0xCD, 0x80,
    ];

    serial::write_str(
        "Mapping process 1 code\n",
    );

    user_space_1.map_user_code(
        user::address_space::USER_CODE_ADDRESS,
        &user_code_1,
    );

    serial::write_str(
        "Mapping process 1 stack\n",
    );

    user_space_1.map_user_stack(
        user::address_space::USER_STACK_ADDRESS,
    );

    serial::write_str(
        "Creating process 1\n",
    );

    let pid_1 =
        process::create_process(
            user_space_1,
            _selectors,
        );

    // ========================================================
    // Process 2
    // ========================================================

    serial::write_str(
        "Creating process 2 address space\n",
    );

    let mut user_space_2 =
        user::UserAddressSpace::new();

    serial::write_str(
        "Process 2 address space created\n",
    );

    let user_code_2: [u8; 127] = [
        // SYS_TEST
        0xB8, 0x03, 0x00, 0x00, 0x00,
        0x48, 0xBF,
        0x37, 0x13, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0xCD, 0x80,

        // SYS_WRITE
        0xB8, 0x04, 0x00, 0x00, 0x00,
        0x48, 0xBF,
        0x01, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x48, 0xBE,
        0x6D, 0x00, 0x00, 0x40,
        0x00, 0x00, 0x00, 0x00,
        0xBA, 0x12, 0x00, 0x00, 0x00,
        0xCD, 0x80,

        // SYS_YIELD
        0xB8, 0x01, 0x00, 0x00, 0x00,
        0xCD, 0x80,

        // SYS_READ
        0xB8, 0x05, 0x00, 0x00, 0x00,
        0x48, 0xBF,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x48, 0x89, 0xE6,
        0x48, 0x83, 0xEE, 0x01,
        0xBA, 0x01, 0x00, 0x00, 0x00,
        0xCD, 0x80,

        // Check key
        0x48, 0x83, 0xF8, 0x01,
        0x74, 0x09,

        // SYS_YIELD
        0xB8, 0x01, 0x00, 0x00, 0x00,
        0xCD, 0x80,

        // Loop
        0xEB, 0xD4,

        // SYS_EXIT
        0xB8, 0x00, 0x00, 0x00, 0x00,
        0x31, 0xFF,
        0xCD, 0x80,

        // String
        b'H', b'e', b'l', b'l', b'o',
        b' ', b'f', b'r', b'o', b'm',
        b' ', b'P', b'I', b'D', b' ',
        b'2', b'!', b'\n',
    ];

    serial::write_str(
        "Mapping process 2 code\n",
    );

    user_space_2.map_user_code(
        user::address_space::USER_CODE_ADDRESS,
        &user_code_2,
    );

    serial::write_str(
        "Mapping process 2 stack\n",
    );

    user_space_2.map_user_stack(
        user::address_space::USER_STACK_ADDRESS,
    );

    serial::write_str(
        "Creating process 2\n",
    );

    let pid_2 =
        process::create_process(
            user_space_2,
            _selectors,
        );

    serial::write_str(
        "Created process 1 PID=",
    );

    serial::write_usize(
        pid_1 as usize,
    );

    serial::write_str(
        "\n",
    );

    serial::write_str(
        "Created process 2 PID=",
    );

    serial::write_usize(
        pid_2 as usize,
    );

    serial::write_str(
        "\n",
    );

    serial::write_str(
        "Selecting process 1\n",
    );

    if !process::set_current(
        pid_1,
    ) {
        serial::write_str(
            "ERROR: failed to select process 1\n",
        );

        halt();
    }

    serial::write_str(
        "Current process PID=",
    );

    serial::write_usize(
        process::current_pid()
            .unwrap_or(0) as usize,
    );

    serial::write_str(
        "\n",
    );

    graphics::begin_frame();

    graphics::draw_string(
        30,
        300,
        "PROCESS 1 CREATED",
        graphics::Color::GREEN,
        1,
    );

    graphics::draw_string(
        30,
        325,
        "PROCESS 2 CREATED",
        graphics::Color::GREEN,
        1,
    );

    graphics::draw_string(
        30,
        350,
        "COOPERATIVE SCHEDULER READY",
        graphics::Color::GREEN,
        1,
    );

    graphics::draw_string(
        30,
        375,
        "SYS_YIELD ENABLED",
        graphics::Color::GREEN,
        1,
    );

    graphics::present(
        framebuffer,
    );

    serial::write_str(
        "ENTERING RING 3\n",
    );

    graphics::begin_frame();

    graphics::draw_string(
        30,
        415,
        "COOPERATIVE SCHEDULER TEST",
        graphics::Color::GREEN,
        2,
    );

    graphics::present(
        framebuffer,
    );

    unsafe {
        process::enter_current(
            _selectors,
        );
    }
    */

    // ========================================================
    // WASM runtime
    // ========================================================

    serial::write_str(
        "Starting WASM runtime\n",
    );

    wasm::run_demo();

    // ========================================================
    // WASM finished
    // ========================================================

    serial::write_str(
        "WASM demo complete\n",
    );

    // ========================================================
    // Kernel idle
    // ========================================================

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
fn panic(
    _info: &core::panic::PanicInfo,
) -> ! {
    serial::write_str(
        "\nRUSTY PANIC\n",
    );

    halt();
}