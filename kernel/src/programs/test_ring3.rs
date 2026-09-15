#![no_std]

use crate::{
    process,
    user,
};

pub fn run(
    selectors: process::Selectors,
) -> ! {
    // ========================================================
    // Process manager
    // ========================================================

    process::init();

    // ========================================================
    // Process 1
    // ========================================================

    // let mut user_space_1 =
    //     user::UserAddressSpace::new();
    //
    // let user_code_1: [u8; 28] = [
    //     // SYS_TEST
    //     0xB8, 0x03, 0x00, 0x00, 0x00,
    //     0xBF, 0x11, 0x11, 0x00, 0x00,
    //     0xCD, 0x80,
    //
    //     // SYS_YIELD
    //     0xB8, 0x01, 0x00, 0x00, 0x00,
    //     0xCD, 0x80,
    //
    //     // SYS_EXIT
    //     0xB8, 0x00, 0x00, 0x00, 0x00,
    //     0x31, 0xFF,
    //     0xCD, 0x80,
    // ];
    //
    // user_space_1.map_user_code(
    //     user::address_space::USER_CODE_ADDRESS,
    //     &user_code_1,
    // );
    //
    // user_space_1.map_user_stack(
    //     user::address_space::USER_STACK_ADDRESS,
    // );
    //
    // let pid_1 =
    //     process::create_process(
    //         user_space_1,
    //         selectors,
    //     );

    // ========================================================
    // Process 2
    // ========================================================
    let mut user_space_2 = user::UserAddressSpace::new();

    let user_code_2: [u8; 227] = [
        // ==========================================
        // 1. SYS_TEST (Syscall #3)
        // ==========================================
        0xB8, 0x03, 0x00, 0x00, 0x00, // mov eax, 3
        0x48, 0xBF,                   // mov rdi, 0x1337
        0x37, 0x13, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0xCD, 0x80,                   // int 0x80

        // ==========================================
        // 2. SYS_CREATE_WINDOW (Syscall #100)
        // x = 50, y = 50, width = 200, height = 150
        // ==========================================
        0xB8, 0x64, 0x00, 0x00, 0x00, // mov eax, 100
        0x48, 0xBF,                   // mov rdi, 50 (x)
        0x32, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x48, 0xBE,                   // mov rsi, 50 (y)
        0x32, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x48, 0xBA,                   // mov rdx, 200 (width)
        0xC8, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x49, 0xC7, 0xC2,             // mov r10, 150 (height)
        0x96, 0x00, 0x00, 0x00,
        0xCD, 0x80,                   // int 0x80

        // Save returned window ID (RAX) into RBX for subsequent drawing calls
        0x48, 0x89, 0xC3,             // mov rbx, rax

        // ==========================================
        // 3. SYS_FILL_RECT (Syscall #7)
        // Fill window interior with dark grey (0xFF222222)
        // ==========================================
        0xB8, 0x07, 0x00, 0x00, 0x00, // mov eax, 7
        0x48, 0x89, 0xD7,             // mov rdi, rbx (window_id)
        0x48, 0xC7, 0xC6,             // mov rsi, 0 (x)
        0x00, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC2,             // mov rdx, 0 (y)
        0x00, 0x00, 0x00, 0x00,
        0x49, 0xC7, 0xC2,             // mov r10, 200 (width)
        0xC8, 0x00, 0x00, 0x00,
        0x49, 0xC7, 0xC0,             // mov r8, 150 (height)
        0x96, 0x00, 0x00, 0x00,
        0x41, 0xB9,                   // mov r9d, 0xFF222222 (color)
        0x22, 0x22, 0x22, 0xFF,
        0xCD, 0x80,                   // int 0x80

        // ==========================================
        // 4. SYS_DRAW_RECT (Syscall #9)
        // Draw green outline box (0xFF00FF00)
        // ==========================================
        0xB8, 0x09, 0x00, 0x00, 0x00, // mov eax, 9
        0x48, 0x89, 0xD7,             // mov rdi, rbx (window_id)
        0x48, 0xC7, 0xC6,             // mov rsi, 5 (x)
        0x05, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC2,             // mov rdx, 5 (y)
        0x05, 0x00, 0x00, 0x00,
        0x49, 0xC7, 0xC2,             // mov r10, 190 (width)
        0xBE, 0x00, 0x00, 0x00,
        0x49, 0xC7, 0xC0,             // mov r8, 140 (height)
        0x8C, 0x00, 0x00, 0x00,
        0x41, 0xB9,                   // mov r9d, 0xFF00FF00 (color)
        0x00, 0xFF, 0x00, 0xFF,
        0xCD, 0x80,                   // int 0x80

        // ==========================================
        // 5. SYS_DRAW_STRING (Syscall #8)
        // Draw "Rusty OS" text (scale 2)
        // ==========================================
        0xB8, 0x08, 0x00, 0x00, 0x00, // mov eax, 8
        0x48, 0x89, 0xD7,             // mov rdi, rbx (window_id)
        0x48, 0xC7, 0xC6,             // mov rsi, 20 (x)
        0x14, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC2,             // mov rdx, 20 (y)
        0x14, 0x00, 0x00, 0x00,

        // Position-independent string retrieval
        0xE8, 0x08, 0x00, 0x00, 0x00, // call +8 (pushes pointer to string onto stack)
        0x52, 0x75, 0x73, 0x74,       // "Rust"
        0x79, 0x20, 0x4F, 0x53,       // "y OS"
        0x41, 0x5A,                   // pop r10 (r10 = str_ptr)

        0x49, 0xC7, 0xC0,             // mov r8, 8 (str_len)
        0x08, 0x00, 0x00, 0x00,
        0x41, 0xB9,                   // mov r9d, 2 (scale)
        0x02, 0x00, 0x00, 0x00,
        0xCD, 0x80,                   // int 0x80

        // ==========================================
        // 6. SYS_FLUSH_SCREEN (Syscall #102)
        // Composite window changes to the framebuffer
        // ==========================================
        0xB8, 0x66, 0x00, 0x00, 0x00, // mov eax, 102
        0xCD, 0x80,                   // int 0x80

        // ==========================================
        // 7. SYS_YIELD (Syscall #1)
        // ==========================================
        0xB8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1
        0xCD, 0x80,                   // int 0x80

        // ==========================================
        // 8. SYS_EXIT (Syscall #0)
        // ==========================================
        0xB8, 0x00, 0x00, 0x00, 0x00, // mov eax, 0
        0x31, 0xFF,                   // xor edi, edi
        0xCD, 0x80,                   // int 0x80
    ];

    user_space_2.map_user_code(
        user::address_space::USER_CODE_ADDRESS,
        &user_code_2,
    );

    user_space_2.map_user_stack(
        user::address_space::USER_STACK_ADDRESS,
    );

    let _pid_2 =
        process::create_process(
            user_space_2,
            selectors,
        );

    // ========================================================
    // Select process 1
    // ========================================================

    if !process::set_current(
        _pid_2,
    ) {
        loop {
            core::hint::spin_loop();
        }
    }

    // ========================================================
    // Enter Ring 3
    // ========================================================

    unsafe {
        process::enter_current(
            selectors,
        );
    }
}