use core::arch::global_asm;

use x86_64::{
    structures::{
        idt::InterruptDescriptorTable,
        paging::PageTableFlags,
    },
    PrivilegeLevel,
    VirtAddr,
};

use crate::process::{
    SavedUserContext,
    ScheduleResult,
};

// ============================================================
// Syscall numbers
// ============================================================

pub const SYS_EXIT: u64 = 0;
pub const SYS_YIELD: u64 = 1;
pub const SYS_GETPID: u64 = 2;
pub const SYS_TEST: u64 = 3;
pub const SYS_WRITE: u64 = 4;
pub const SYS_READ: u64 = 5;
pub const SYS_SLEEP: u64 = 6;

const ENOSYS: u64 = u64::MAX;
const SUCCESS: u64 = 0;

const SYSCALL_SWITCH: u64 =
    u64::MAX - 1;

const SYSCALL_HALT: u64 =
    u64::MAX - 2;

// ============================================================
// Saved register frame
// ============================================================

#[repr(C)]
pub struct SyscallFrame {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
}

// ============================================================
// User interrupt frame
// ============================================================

#[repr(C)]
pub struct UserInterruptFrame {
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

// ============================================================
// Save context
// ============================================================

fn save_context(
    frame: &SyscallFrame,
    interrupt: &UserInterruptFrame,
) -> SavedUserContext {
    SavedUserContext {
        rax: frame.rax,
        rbx: frame.rbx,
        rcx: frame.rcx,
        rdx: frame.rdx,
        rsi: frame.rsi,
        rdi: frame.rdi,
        rbp: frame.rbp,
        r8: frame.r8,
        r9: frame.r9,
        r10: frame.r10,
        r11: frame.r11,
        r12: frame.r12,
        r13: frame.r13,
        r14: frame.r14,
        r15: frame.r15,

        rip: interrupt.rip,
        cs: interrupt.cs,
        rflags: interrupt.rflags,
        rsp: interrupt.rsp,
        ss: interrupt.ss,
    }
}

// ============================================================
// Load context
// ============================================================

fn load_context(
    context: SavedUserContext,
    frame: &mut SyscallFrame,
    interrupt: &mut UserInterruptFrame,
) {
    frame.rax = context.rax;
    frame.rbx = context.rbx;
    frame.rcx = context.rcx;
    frame.rdx = context.rdx;
    frame.rsi = context.rsi;
    frame.rdi = context.rdi;
    frame.rbp = context.rbp;
    frame.r8 = context.r8;
    frame.r9 = context.r9;
    frame.r10 = context.r10;
    frame.r11 = context.r11;
    frame.r12 = context.r12;
    frame.r13 = context.r13;
    frame.r14 = context.r14;
    frame.r15 = context.r15;

    interrupt.rip =
        context.rip;

    interrupt.cs =
        context.cs;

    interrupt.rflags =
        context.rflags;

    interrupt.rsp =
        context.rsp;

    interrupt.ss =
        context.ss;
}

// ============================================================
// Syscall assembly entry
// ============================================================

global_asm!(
    r#"
    .global rusty_syscall_entry
    .type rusty_syscall_entry, @function

rusty_syscall_entry:
    // --------------------------------------------------------
    // Save registers in reverse order.
    //
    // RSP points directly at saved RAX.
    // --------------------------------------------------------

    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rbp
    push rdi
    push rsi
    push rdx
    push rcx
    push rbx
    push rax

    // --------------------------------------------------------
    // R12 = SyscallFrame*
    // --------------------------------------------------------

    mov r12, rsp

    // --------------------------------------------------------
    // Align stack for Rust.
    // --------------------------------------------------------

    and rsp, -16

    // First argument:
    //     SyscallFrame*
    //
    mov rdi, r12

    // Second argument:
    //     UserInterruptFrame*
    //
    // 15 saved registers * 8 bytes = 120 bytes.
    //
    lea rsi, [r12 + 120]

    call rusty_syscall_dispatch

    // --------------------------------------------------------
    // A process was switched.
    // --------------------------------------------------------

    cmp rax, 0xFFFFFFFFFFFFFFFE
    je rusty_syscall_switch

    // --------------------------------------------------------
    // No process remains.
    // --------------------------------------------------------

    cmp rax, 0xFFFFFFFFFFFFFFFD
    je rusty_syscall_halt

    // --------------------------------------------------------
    // Normal syscall return.
    // --------------------------------------------------------

    mov rsp, r12

    mov [rsp], rax

    pop rax
    pop rbx
    pop rcx
    pop rdx
    pop rsi
    pop rdi
    pop rbp
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15

    iretq

    // --------------------------------------------------------
    // Process switch.
    //
    // Rust has already replaced the saved register frame
    // and IRET frame with the next process's context.
    // --------------------------------------------------------

rusty_syscall_switch:
    mov rsp, r12

    pop rax
    pop rbx
    pop rcx
    pop rdx
    pop rsi
    pop rdi
    pop rbp
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15

    iretq

    // --------------------------------------------------------
    // No process remains.
    // --------------------------------------------------------

rusty_syscall_halt:
    cli

rusty_syscall_halt_loop:
    hlt
    jmp rusty_syscall_halt_loop
"#
);

unsafe extern "C" {
    fn rusty_syscall_entry();
}

// ============================================================
// Dispatcher
// ============================================================

#[unsafe(no_mangle)]
extern "C" fn rusty_syscall_dispatch(
    frame: *mut SyscallFrame,
    interrupt: *mut UserInterruptFrame,
) -> u64 {
    let frame =
        unsafe {
            &mut *frame
        };

    let interrupt =
        unsafe {
            &mut *interrupt
        };

    match frame.rax {
        SYS_EXIT =>
            syscall_exit(
                frame,
                interrupt,
            ),

        SYS_YIELD =>
            syscall_yield(
                frame,
                interrupt,
            ),

        SYS_GETPID =>
            syscall_getpid(),

        SYS_TEST =>
            syscall_test(
                frame.rdi,
            ),

        SYS_WRITE =>
            syscall_write(
                frame,
            ),

        SYS_READ =>
            syscall_read(
                frame,
                interrupt,
            ),

        SYS_SLEEP =>
            syscall_sleep(
                frame,
            ),

        _ => ENOSYS,
    }
}

// ============================================================
// SYS_EXIT
// ============================================================

fn syscall_exit(
    frame: &mut SyscallFrame,
    interrupt: &mut UserInterruptFrame,
) -> u64 {
    let status =
        frame.rdi;

    match crate::process::exit_current(
        status,
    ) {
        ScheduleResult::Switched(
            context,
        ) => {
            load_context(
                context,
                frame,
                interrupt,
            );

            SYSCALL_SWITCH
        }

        ScheduleResult::NoProcess =>
            SYSCALL_HALT,
    }
}

// ============================================================
// SYS_YIELD
// ============================================================

fn syscall_yield(
    frame: &mut SyscallFrame,
    interrupt: &mut UserInterruptFrame,
) -> u64 {
    let current_context =
        save_context(
            frame,
            interrupt,
        );

    match crate::process::yield_current(
        current_context,
    ) {
        ScheduleResult::Switched(
            context,
        ) => {
            load_context(
                context,
                frame,
                interrupt,
            );

            SYSCALL_SWITCH
        }

        ScheduleResult::NoProcess =>
            SUCCESS,
    }
}

// ============================================================
// SYS_GETPID
// ============================================================

fn syscall_getpid() -> u64 {
    match crate::process::current_pid() {
        Some(pid) => {
            crate::serial::write_str(
                "sys_getpid() -> ",
            );

            crate::serial::write_usize(
                pid as usize,
            );

            crate::serial::write_str(
                "\n",
            );

            pid
        }

        None => {
            crate::serial::write_str(
                "sys_getpid() -> no current process\n",
            );

            0
        }
    }
}

// ============================================================
// SYS_TEST
// ============================================================

fn syscall_test(
    value: u64,
) -> u64 {
    crate::serial::write_str(
        "sys_test(",
    );

    crate::serial::write_usize(
        value as usize,
    );

    crate::serial::write_str(
        ")\n",
    );

    value
}

// ============================================================
// SYS_WRITE
// ============================================================

fn syscall_write(
    frame: &SyscallFrame,
) -> u64 {
    let fd =
        frame.rdi;

    let address =
        frame.rsi;

    let length =
        match usize::try_from(
            frame.rdx,
        ) {
            Ok(length) =>
                length,

            Err(_) =>
                return u64::MAX,
        };

    if fd != 1 {
        crate::serial::write_str(
            "sys_write: unsupported file descriptor\n",
        );
        return u64::MAX;
    }

    if !unsafe {
        crate::memory::validate_user_range(
            crate::memory::current_level_4_frame(),
            address,
            length,
            false,
        )
    } {
        crate::serial::write_str(
            "sys_write: invalid user memory range or unmapped page\n",
        );
        return u64::MAX;
    }

    let bytes =
        unsafe {
            core::slice::from_raw_parts(
                address as *const u8,
                length,
            )
        };

    for &byte in bytes {
        crate::serial::write_byte(
            byte,
        );
    }

    length as u64
}

// ============================================================
// SYS_READ
// ============================================================

fn syscall_read(
    frame: &mut SyscallFrame,
    interrupt: &mut UserInterruptFrame,
) -> u64 {
    let fd =
        frame.rdi;

    let address =
        frame.rsi;

    let length =
        match usize::try_from(
            frame.rdx,
        ) {
            Ok(length) =>
                length,

            Err(_) =>
                return u64::MAX,
        };

    if fd != 0 {
        crate::serial::write_str(
            "sys_read: unsupported file descriptor\n",
        );
        return u64::MAX;
    }

    if !unsafe {
        crate::memory::validate_user_range(
            crate::memory::current_level_4_frame(),
            address,
            length,
            true,
        )
    } {
        crate::serial::write_str(
            "sys_read: invalid user memory range, unmapped page, or read-only destination\n",
        );
        return u64::MAX;
    }

    // Attempt to poll keyboard input non-blocking
    if let Some(key) = crate::input::read_key() {
        let ascii_byte = match key {
            crate::input::Key::Character(c) => c as u8,
            crate::input::Key::Enter => b'\n',
            crate::input::Key::Space => b' ',
            crate::input::Key::Backspace => 0x08,
            _ => 0,
        };

        if ascii_byte != 0 {
            unsafe {
                *(address as *mut u8) = ascii_byte;
            }
            return 1;
        } else {
            return 0;
        }
    } else {
        // Save original RIP prior to rewinding for yield execution
        let original_rip = interrupt.rip;

        // No keystroke available: block process by rewinding RIP past `int 0x80` (2 bytes)
        // and yielding CPU execution to other tasks.
        interrupt.rip =
            interrupt.rip.checked_sub(2).unwrap_or(interrupt.rip);

        let current_context =
            save_context(
                frame,
                interrupt,
            );

        match crate::process::yield_current(
            current_context,
        ) {
            ScheduleResult::Switched(
                context,
            ) => {
                load_context(
                    context,
                    frame,
                    interrupt,
                );

                SYSCALL_SWITCH
            }

            ScheduleResult::NoProcess => {
                // If no context switch took place, restore original RIP so the process
                // doesn't re-execute int 0x80 with RAX = 0 (SYS_EXIT).
                interrupt.rip = original_rip;
                0
            }
        }
    }
}

// ============================================================
// SYS_SLEEP
// ============================================================

fn syscall_sleep(
    _frame: &SyscallFrame,
) -> u64 {
    SUCCESS
}

// ============================================================
// Install syscall gate
// ============================================================

pub unsafe fn install(
    idt: &mut InterruptDescriptorTable,
) {
    unsafe {
        let entry =
            idt[0x80].set_handler_addr(
                VirtAddr::from_ptr(
                    rusty_syscall_entry
                        as *const (),
                ),
            );

        entry.set_privilege_level(
            PrivilegeLevel::Ring3,
        );
    }
}