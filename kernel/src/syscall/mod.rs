use core::arch::global_asm;

use x86_64::{
    structures::{
        idt::InterruptDescriptorTable,
    },
    PrivilegeLevel,
    VirtAddr,
};

use crate::graphics::Color;
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

// Primitive Window UI Syscalls
pub const SYS_FILL_RECT: u64 = 7;
pub const SYS_DRAW_STRING: u64 = 8;
pub const SYS_DRAW_RECT: u64 = 9;

// Window Manager Syscalls
pub const SYS_CREATE_WINDOW: u64 = 100;
pub const SYS_UPDATE_WINDOW: u64 = 101;
pub const SYS_FLUSH_SCREEN: u64 = 102;
pub const SYS_CLICK: u64 = 103;
pub const SYS_DESTROY_WINDOW: u64 = 104;
pub const SYS_LAUNCH_APP: u64 = 105;
pub const SYS_CLICK_RECT: u64 = 106;

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
    // --------------------------------------------------------
    // Pump USB before servicing input.
    //
    // This keeps CrabUSB running while userspace is executing.
    // There is intentionally NO permanent USB loop in the boot
    // handler, otherwise Ring 3 would never be reached.
    // --------------------------------------------------------

    service_usb();

    // --------------------------------------------------------
    // Service physical mouse input.
    //
    // USB polling above produces input events. The compositor
    // consumes mouse events and updates only the cursor region.
    // --------------------------------------------------------

    service_mouse();

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

        SYS_FILL_RECT =>
            syscall_fill_rect(
                frame,
            ),

        SYS_DRAW_STRING =>
            syscall_draw_string(
                frame,
            ),

        SYS_DRAW_RECT =>
            syscall_draw_rect(
                frame,
            ),

        SYS_CREATE_WINDOW =>
            syscall_create_window(
                frame,
            ),

        SYS_UPDATE_WINDOW =>
            syscall_update_window(
                frame,
            ),

        SYS_FLUSH_SCREEN =>
            syscall_flush_screen(),

        SYS_CLICK =>
            syscall_click(
                frame,
            ),

        SYS_DESTROY_WINDOW =>
            syscall_destroy_window(
                frame,
            ),

        SYS_LAUNCH_APP =>
            syscall_launch_app(
                frame,
            ),

        SYS_CLICK_RECT =>
            syscall_click_rect(
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
    // --------------------------------------------------------
    // IMPORTANT:
    //
    // DO NOT call wm.draw() here.
    //
    // SYS_YIELD is used constantly by userspace programs.
    // Redrawing here would recomposite the entire desktop and
    // copy the entire framebuffer on every scheduler yield.
    //
    // Mouse movement is already handled by service_mouse()
    // at the beginning of rusty_syscall_dispatch(), which uses
    // update_cursor_only().
    // --------------------------------------------------------

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
// fd  = rdi
// buf = rsi
// len = rdx
//
// Reads keyboard input from the kernel input event queue.
//
// Returns:
//   1  = one byte read
//   0  = no input available / yielded
//   u64::MAX = error
// ============================================================

fn syscall_read(
    frame: &mut SyscallFrame,
    interrupt: &mut UserInterruptFrame,
) -> u64 {
    let fd = frame.rdi;
    let address = frame.rsi;

    let length = match usize::try_from(frame.rdx) {
        Ok(length) => length,
        Err(_) => {
            return u64::MAX;
        }
    };

    // --------------------------------------------------------
    // stdin
    // --------------------------------------------------------

    if fd != 0 {
        crate::serial::write_str(
            "sys_read: unsupported file descriptor\n",
        );

        return u64::MAX;
    }

    // --------------------------------------------------------
    // Nothing to read.
    // --------------------------------------------------------

    if length == 0 {
        return 0;
    }

    // --------------------------------------------------------
    // Validate userspace destination.
    // --------------------------------------------------------

    if !unsafe {
        crate::memory::validate_user_range(
            crate::memory::current_level_4_frame(),
            address,
            1,
            true,
        )
    } {
        crate::serial::write_str(
            "sys_read: invalid user memory range, unmapped page, or read-only destination\n",
        );

        return u64::MAX;
    }

    // --------------------------------------------------------
    // Look for a keyboard event.
    //
    // The new input system uses:
    //
    //     InputEvent::KeyDown
    //     InputEvent::KeyUp
    //
    // We only want KeyDown here.
    // --------------------------------------------------------

    while let Some(event) = crate::input::poll_keyboard_event() {
        match event {
            crate::input::InputEvent::KeyDown {
                key,
                modifiers,
            } => {
                let ascii = key_to_ascii(
                    key,
                    modifiers,
                );

                if ascii == 0 {
                    // Non-printable key.
                    //
                    // Keep looking for another keyboard event.
                    continue;
                }

                unsafe {
                    *(address as *mut u8) = ascii;
                }

                return 1;
            }

            // KeyUp is not input for stdin.
            crate::input::InputEvent::KeyUp { .. } => {
                continue;
            }

            // Mouse events are not stdin data.
            //
            // NOTE:
            // With the current single InputEvent queue, consuming
            // these here means they are removed from the queue.
            // The compositor should therefore not compete with
            // SYS_READ for the same queue.
            crate::input::InputEvent::MouseMove { .. } => {
                continue;
            }

            crate::input::InputEvent::MouseButtonDown(_) => {
                continue;
            }

            crate::input::InputEvent::MouseButtonUp(_) => {
                continue;
            }

            crate::input::InputEvent::MouseWheel { .. } => {
                continue;
            }
        }
    }

    // --------------------------------------------------------
    // No keyboard input available.
    //
    // Rewind RIP so that when this process is scheduled again,
    // the same INT 0x80 SYS_READ instruction executes again.
    // --------------------------------------------------------

    let original_rip = interrupt.rip;

    interrupt.rip = interrupt
        .rip
        .checked_sub(2)
        .unwrap_or(interrupt.rip);

    let current_context = save_context(
        frame,
        interrupt,
    );

    match crate::process::yield_current(
        current_context,
    ) {
        ScheduleResult::Switched(context) => {
            load_context(
                context,
                frame,
                interrupt,
            );

            SYSCALL_SWITCH
        }

        ScheduleResult::NoProcess => {
            // Nothing else to schedule.
            // Restore the original RIP so we do not accidentally
            // re-execute the INT 0x80 instruction.
            interrupt.rip = original_rip;

            0
        }
    }
}

// ============================================================
// Convert a physical keyboard key + modifiers into ASCII.
//
// Returns 0 when the key has no ASCII representation.
// ============================================================

fn key_to_ascii(
    key: crate::input::Key,
    modifiers: crate::input::Modifiers,
) -> u8 {
    let shift =
        modifiers.left_shift ||
            modifiers.right_shift;

    match key {
        crate::input::Key::A =>
            if shift { b'A' } else { b'a' },

        crate::input::Key::B =>
            if shift { b'B' } else { b'b' },

        crate::input::Key::C =>
            if shift { b'C' } else { b'c' },

        crate::input::Key::D =>
            if shift { b'D' } else { b'd' },

        crate::input::Key::E =>
            if shift { b'E' } else { b'e' },

        crate::input::Key::F =>
            if shift { b'F' } else { b'f' },

        crate::input::Key::G =>
            if shift { b'G' } else { b'g' },

        crate::input::Key::H =>
            if shift { b'H' } else { b'h' },

        crate::input::Key::I =>
            if shift { b'I' } else { b'i' },

        crate::input::Key::J =>
            if shift { b'J' } else { b'j' },

        crate::input::Key::K =>
            if shift { b'K' } else { b'k' },

        crate::input::Key::L =>
            if shift { b'L' } else { b'l' },

        crate::input::Key::M =>
            if shift { b'M' } else { b'm' },

        crate::input::Key::N =>
            if shift { b'N' } else { b'n' },

        crate::input::Key::O =>
            if shift { b'O' } else { b'o' },

        crate::input::Key::P =>
            if shift { b'P' } else { b'p' },

        crate::input::Key::Q =>
            if shift { b'Q' } else { b'q' },

        crate::input::Key::R =>
            if shift { b'R' } else { b'r' },

        crate::input::Key::S =>
            if shift { b'S' } else { b's' },

        crate::input::Key::T =>
            if shift { b'T' } else { b't' },

        crate::input::Key::U =>
            if shift { b'U' } else { b'u' },

        crate::input::Key::V =>
            if shift { b'V' } else { b'v' },

        crate::input::Key::W =>
            if shift { b'W' } else { b'w' },

        crate::input::Key::X =>
            if shift { b'X' } else { b'x' },

        crate::input::Key::Y =>
            if shift { b'Y' } else { b'y' },

        crate::input::Key::Z =>
            if shift { b'Z' } else { b'z' },

        crate::input::Key::Num1 =>
            if shift { b'!' } else { b'1' },

        crate::input::Key::Num2 =>
            if shift { b'@' } else { b'2' },

        crate::input::Key::Num3 =>
            if shift { b'#' } else { b'3' },

        crate::input::Key::Num4 =>
            if shift { b'$' } else { b'4' },

        crate::input::Key::Num5 =>
            if shift { b'%' } else { b'5' },

        crate::input::Key::Num6 =>
            if shift { b'^' } else { b'6' },

        crate::input::Key::Num7 =>
            if shift { b'&' } else { b'7' },

        crate::input::Key::Num8 =>
            if shift { b'*' } else { b'8' },

        crate::input::Key::Num9 =>
            if shift { b'(' } else { b'9' },

        crate::input::Key::Num0 =>
            if shift { b')' } else { b'0' },

        crate::input::Key::Enter =>
            b'\n',

        crate::input::Key::Space =>
            b' ',

        crate::input::Key::Tab =>
            b'\t',

        crate::input::Key::Backspace =>
            0x08,

        crate::input::Key::Minus =>
            if shift { b'_' } else { b'-' },

        crate::input::Key::Equal =>
            if shift { b'+' } else { b'=' },

        crate::input::Key::LeftBracket =>
            if shift { b'{' } else { b'[' },

        crate::input::Key::RightBracket =>
            if shift { b'}' } else { b']' },

        crate::input::Key::Backslash =>
            if shift { b'|' } else { b'\\' },

        crate::input::Key::Semicolon =>
            if shift { b':' } else { b';' },

        crate::input::Key::Apostrophe =>
            if shift { b'"' } else { b'\'' },

        crate::input::Key::Grave =>
            if shift { b'~' } else { b'`' },

        crate::input::Key::Comma =>
            if shift { b'<' } else { b',' },

        crate::input::Key::Dot =>
            if shift { b'>' } else { b'.' },

        crate::input::Key::Slash =>
            if shift { b'?' } else { b'/' },

        _ => 0,
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
// SYS_FILL_RECT (#7)
// rdi = window_id
// rsi = x
// rdx = y
// r10 = width
// r8  = height
// r9  = color
// ============================================================

fn syscall_fill_rect(
    frame: &SyscallFrame,
) -> u64 {
    let window_id =
        frame.rdi;

    let x =
        frame.rsi as usize;

    let y =
        frame.rdx as usize;

    let width =
        frame.r10 as usize;

    let height =
        frame.r8 as usize;

    let color_argb =
        frame.r9 as u32;

    let mut wm_lock =
        crate::wm::WM.lock();

    if let Some(ref mut wm) =
        *wm_lock
    {
        if let Some(win) =
            wm.windows.get_mut(
                &window_id,
            )
        {
            win.fill_rect(
                x,
                y,
                width,
                height,
                color_argb,
            );

            return 1;
        }
    }

    0
}

// ============================================================
// SYS_DRAW_STRING (#8)
// rdi = window_id
// rsi = x
// rdx = y
// r10 = str_ptr
// r8  = str_len
// r9  = scale
// ============================================================

fn syscall_draw_string(
    frame: &SyscallFrame,
) -> u64 {
    let window_id =
        frame.rdi;

    let x =
        frame.rsi as usize;

    let y =
        frame.rdx as usize;

    let str_ptr =
        frame.r10;

    let str_len =
        frame.r8 as usize;

    let scale =
        frame.r9 as usize;

    if !unsafe {
        crate::memory::validate_user_range(
            crate::memory::current_level_4_frame(),
            str_ptr,
            str_len,
            false,
        )
    } {
        return 0;
    }

    let bytes =
        unsafe {
            core::slice::from_raw_parts(
                str_ptr as *const u8,
                str_len,
            )
        };

    let text =
        match core::str::from_utf8(
            bytes,
        ) {
            Ok(text) =>
                text,

            Err(_) =>
                return 0,
        };

    let mut wm_lock =
        crate::wm::WM.lock();

    if let Some(ref mut wm) =
        *wm_lock
    {
        if let Some(win) =
            wm.windows.get_mut(
                &window_id,
            )
        {
            win.draw_string(
                x,
                y,
                text,
                Color::WHITE,
                scale,
            );

            return 1;
        }
    }

    0
}

// ============================================================
// SYS_DRAW_RECT (#9)
// rdi = window_id
// rsi = x
// rdx = y
// r10 = width
// r8  = height
// r9  = color
// ============================================================

fn syscall_draw_rect(
    frame: &SyscallFrame,
) -> u64 {
    let window_id =
        frame.rdi;

    let x =
        frame.rsi as usize;

    let y =
        frame.rdx as usize;

    let width =
        frame.r10 as usize;

    let height =
        frame.r8 as usize;

    let color_argb =
        frame.r9 as u32;

    if width == 0
        || height == 0
    {
        return 0;
    }

    let mut wm_lock =
        crate::wm::WM.lock();

    if let Some(ref mut wm) =
        *wm_lock
    {
        if let Some(win) =
            wm.windows.get_mut(
                &window_id,
            )
        {
            // Draw rectangle outline:
            // top, bottom, left, right.
            win.fill_rect(
                x,
                y,
                width,
                1,
                color_argb,
            );

            win.fill_rect(
                x,
                y + height - 1,
                width,
                1,
                color_argb,
            );

            win.fill_rect(
                x,
                y,
                1,
                height,
                color_argb,
            );

            win.fill_rect(
                x + width - 1,
                y,
                1,
                height,
                color_argb,
            );

            return 1;
        }
    }

    0
}

// ============================================================
// SYS_CREATE_WINDOW
// ============================================================

fn syscall_create_window(
    frame: &SyscallFrame,
) -> u64 {
    let x =
        frame.rdi as i32;

    let y =
        frame.rsi as i32;

    let width =
        frame.rdx as usize;

    let height =
        frame.r10 as usize;

    let mut wm_lock =
        crate::wm::WM.lock();

    if let Some(ref mut compositor) =
        *wm_lock
    {
        compositor.create_window(
            x,
            y,
            width,
            height,
        )
    } else {
        0
    }
}

// ============================================================
// SYS_UPDATE_WINDOW
// ============================================================

fn syscall_update_window(
    frame: &SyscallFrame,
) -> u64 {
    let window_id =
        frame.rdi;

    let buffer_address =
        frame.rsi;

    let pixel_count =
        frame.rdx as usize;

    // Validate that the user buffer is mapped and accessible.
    //
    // u32 = 4 bytes per pixel.
    // Checked multiplication prevents integer overflow.
    let byte_length =
        match pixel_count.checked_mul(4) {
            Some(length) =>
                length,

            None => {
                crate::serial::write_str(
                    "sys_update_window: pixel count overflow\n",
                );

                return 0;
            }
        };

    if !unsafe {
        crate::memory::validate_user_range(
            crate::memory::current_level_4_frame(),
            buffer_address,
            byte_length,
            false,
        )
    } {
        crate::serial::write_str(
            "sys_update_window: invalid user memory range\n",
        );

        return 0;
    }

    let user_pixels =
        unsafe {
            core::slice::from_raw_parts(
                buffer_address as *const u32,
                pixel_count,
            )
        };

    let mut wm_lock =
        crate::wm::WM.lock();

    if let Some(ref mut compositor) =
        *wm_lock
    {
        if compositor.update_window_pixels(
            window_id,
            user_pixels,
        ) {
            1
        } else {
            0
        }
    } else {
        0
    }
}

// ============================================================
// SYS_FLUSH_SCREEN
// ============================================================

fn syscall_flush_screen() -> u64 {
    let mut wm_lock =
        crate::wm::WM.lock();

    if let Some(ref mut compositor) =
        *wm_lock
    {
        compositor.draw();

        1
    } else {
        0
    }
}

// ============================================================
// SYS_CLICK (#103)
// rdi = screen x
// rsi = screen y
// ============================================================

fn syscall_click(
    frame: &SyscallFrame,
) -> u64 {
    let x =
        frame.rdi as usize;

    let y =
        frame.rsi as usize;

    let mut wm_lock =
        crate::wm::WM.lock();

    if let Some(ref mut compositor) =
        *wm_lock
    {
        if compositor.sys_click(
            x,
            y,
        ) {
            1
        } else {
            0
        }
    } else {
        0
    }
}

// ============================================================
// SYS_CLICK_RECT (#106)
// rdi = screen x
// rsi = screen y
// rdx = width
// r10 = height
// ============================================================
//
// Performs hit-testing against a rectangular screen area.
//
// The click is considered inside when:
//
//     x <= click_x < x + width
//     y <= click_y < y + height
//
// This avoids requiring userspace to predict the exact pixel
// where the physical mouse button was pressed.
//

fn syscall_click_rect(
    frame: &SyscallFrame,
) -> u64 {
    let x =
        frame.rdi as usize;

    let y =
        frame.rsi as usize;

    let width =
        frame.rdx as usize;

    let height =
        frame.r10 as usize;

    crate::serial::write_str(
        "CLICK DEBUG: SYS_CLICK_RECT query (",
    );

    crate::serial::write_usize(
        x,
    );

    crate::serial::write_str(
        ", ",
    );

    crate::serial::write_usize(
        y,
    );

    crate::serial::write_str(
        ", ",
    );

    crate::serial::write_usize(
        width,
    );

    crate::serial::write_str(
        ", ",
    );

    crate::serial::write_usize(
        height,
    );

    crate::serial::write_str(
        ")\n",
    );

    let mut wm_lock =
        crate::wm::WM.lock();

    let Some(ref mut compositor) =
        *wm_lock
    else {
        crate::serial::write_str(
            "CLICK DEBUG: SYS_CLICK_RECT -> NO COMPOSITOR\n",
        );

        return 0;
    };

    if compositor.sys_click_rect(
        x,
        y,
        width,
        height,
    ) {
        1
    } else {
        0
    }
}

// ============================================================
// SYS_DESTROY_WINDOW (#104)
// rdi = window_id
// ============================================================

fn syscall_destroy_window(
    frame: &SyscallFrame,
) -> u64 {
    let window_id =
        frame.rdi;

    let mut wm_lock =
        crate::wm::WM.lock();

    let Some(ref mut wm) =
        *wm_lock
    else {
        return 0;
    };

    if wm.destroy_window(
        window_id,
    ) {
        // The window changed, so a full redraw is appropriate
        // here. This is NOT part of SYS_YIELD.
        wm.draw();

        1
    } else {
        0
    }
}

// ============================================================
// SYS_LAUNCH_APP (#105)
// rdi = userspace ELF pointer
// rsi = ELF length
// ============================================================
//
// The userspace process supplies the ELF bytes.
//
// Example userspace ABI:
//
//     rax = SYS_LAUNCH_APP
//     rdi = elf.as_ptr()
//     rsi = elf.len()
//
// On success this function does not return.
// launch_app::run() creates the new process and enters Ring 3.
//
// IMPORTANT:
// This syscall does NOT destroy any specific window.
// Window destruction is handled independently by
// SYS_DESTROY_WINDOW.
//

fn syscall_launch_app(
    frame: &SyscallFrame,
) -> ! {
    let elf_address =
        frame.rdi;

    let elf_length =
        match usize::try_from(
            frame.rsi,
        ) {
            Ok(length) =>
                length,

            Err(_) => {
                crate::serial::write_str(
                    "SYS_LAUNCH_APP: ELF length overflow\n",
                );

                loop {
                    core::hint::spin_loop();
                }
            }
        };

    // --------------------------------------------------------
    // Validate ELF buffer.
    // --------------------------------------------------------

    if elf_address == 0
        || elf_length == 0
    {
        crate::serial::write_str(
            "SYS_LAUNCH_APP: invalid ELF buffer\n",
        );

        loop {
            core::hint::spin_loop();
        }
    }

    if !unsafe {
        crate::memory::validate_user_range(
            crate::memory::current_level_4_frame(),
            elf_address,
            elf_length,
            false,
        )
    } {
        crate::serial::write_str(
            "SYS_LAUNCH_APP: invalid user ELF range\n",
        );

        loop {
            core::hint::spin_loop();
        }
    }

    // --------------------------------------------------------
    // Convert the userspace buffer into an ELF byte slice.
    //
    // The range was validated above against the current
    // process address space.
    // --------------------------------------------------------

    let elf =
        unsafe {
            core::slice::from_raw_parts(
                elf_address as *const u8,
                elf_length,
            )
        };

    crate::serial::write_str(
        "SYS_LAUNCH_APP: launching ELF\n",
    );

    // --------------------------------------------------------
    // Obtain the GDT selectors used by Ring 3 processes.
    // --------------------------------------------------------

    let selectors =
        crate::cpu::gdt::init();

    // --------------------------------------------------------
    // Launch application.
    //
    // launch_app::run() is a diverging function:
    //
    //     -> !
    //
    // It creates the address space, loads the ELF, maps the
    // user stack, creates the process, selects it, and finally
    // enters Ring 3.
    // --------------------------------------------------------

    crate::launch_app::run(
        selectors,
        elf,
    );
}

// ============================================================
// Mouse/compositor tick
// ============================================================
//
// IMPORTANT PERFORMANCE PATH:
//
// Mouse movement:
//
//     USB mouse
//         ↓
//     service_mouse()
//         ↓
//     update_mouse()
//         ↓
//     update_cursor_only()
//         ↓
//     restore tiny old cursor region
//         ↓
//     draw tiny cursor
//
// There is NO full-screen compositor redraw here.
//

// ============================================================
// USB/input service
// ============================================================
//
// USB is deliberately NOT polled from the boot handler.
//
// Userspace enters the kernel through syscalls, and every
// syscall entry pumps CrabUSB once. This allows:
//
//     Ring 3
//        ↓
//     syscall
//        ↓
//     USB poll
//        ↓
//     input events
//        ↓
//     return to Ring 3
//
// SYS_READ can therefore yield when no keyboard input exists,
// and the next syscall/kernel entry will pump USB again.
//

fn service_usb() {
    // crate::usb::poll::poll();
}

fn service_mouse() {
    let mut wm_lock =
        crate::wm::WM.lock();

    let Some(ref mut wm) =
        *wm_lock
    else {
        return;
    };

    if wm.update_mouse() {
        wm.update_cursor_only();
    }
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