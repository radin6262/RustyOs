use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
};

use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::{
    set_general_handler,
    structures::idt::{
        InterruptDescriptorTable,
        InterruptStackFrame,
    },
};

use crate::{
    serial,
    syscall,
};

// ============================================================
// Programmable Interrupt Controller (PIC) Configuration
// ============================================================

pub const PIC_1_OFFSET: u8 =
    32;

pub const PIC_2_OFFSET: u8 =
    PIC_1_OFFSET + 8;

pub static PICS:
Mutex<ChainedPics> =
    Mutex::new(
        unsafe {
            ChainedPics::new(
                PIC_1_OFFSET,
                PIC_2_OFFSET,
            )
        },
    );

// ============================================================
// Static IDT storage
// ============================================================

struct IdtStorage {
    idt:
        UnsafeCell<
            MaybeUninit<
                InterruptDescriptorTable,
            >,
        >,
}

unsafe impl Sync
for IdtStorage {
}

impl IdtStorage {
    const fn new() -> Self {
        Self {
            idt:
            UnsafeCell::new(
                MaybeUninit::uninit(),
            ),
        }
    }
}

static IDT_STORAGE:
IdtStorage =
    IdtStorage::new();

// ============================================================
// Initialize IDT
// ============================================================

pub fn init() {
    let mut idt =
        InterruptDescriptorTable::new();

    // --------------------------------------------------------
    // General exception handler
    //
    // #SS = 12
    // #GP = 13
    // #PF = 14
    // --------------------------------------------------------

    set_general_handler!(
        &mut idt,
        general_exception_handler,
        12..15
    );

    // --------------------------------------------------------
    // Invalid opcode (#UD)
    // --------------------------------------------------------

    idt.invalid_opcode
        .set_handler_fn(
            invalid_opcode_handler,
        );

    // --------------------------------------------------------
    // Double fault (#DF)
    // --------------------------------------------------------

    idt.double_fault
        .set_handler_fn(
            double_fault_handler,
        );

    // --------------------------------------------------------
    // Hardware Timer
    //
    // IRQ 0 -> vector 32
    // --------------------------------------------------------

    idt[PIC_1_OFFSET]
        .set_handler_fn(
            timer_interrupt_handler,
        );

    // --------------------------------------------------------
    // Syscall
    // --------------------------------------------------------

    unsafe {
        syscall::install(
            &mut idt,
        );
    }

    // --------------------------------------------------------
    // Store IDT permanently.
    // --------------------------------------------------------

    unsafe {
        (*IDT_STORAGE.idt.get())
            .write(idt);
    }

    let idt_ref:
        &'static InterruptDescriptorTable =
        unsafe {
            &*(
                (*IDT_STORAGE.idt.get())
                    .as_ptr()
            )
        };

    // --------------------------------------------------------
    // Load IDT
    // --------------------------------------------------------

    unsafe {
        idt_ref.load();
    }

    // --------------------------------------------------------
    // Initialize PIC
    // --------------------------------------------------------

    unsafe {
        PICS
            .lock()
            .initialize();
    }
}

// ============================================================
// Hardware Timer Interrupt
// ============================================================
//
// For now this only acknowledges the timer IRQ.
//
// We deliberately do NOT call process scheduling here yet.
//
// Why?
//
// `SavedUserContext` contains:
//
//     rax rbx rcx rdx
//     rsi rdi rbp
//     r8-r15
//     rip cs rflags rsp ss
//
// A normal `extern "x86-interrupt"` handler does not give us
// those general-purpose registers as a `SavedUserContext`.
//
// Our syscall path already has custom assembly that saves them.
//
// The correct preemptive implementation will therefore be:
//
//     timer IRQ
//        ↓
//     assembly entry
//        ↓
//     save GPRs
//        ↓
//     build SavedUserContext
//        ↓
//     scheduler
//        ↓
//     restore next context
//        ↓
//     iretq
//
// Do not fake this with a normal Rust function call.
//

extern "x86-interrupt" fn
timer_interrupt_handler(
    _stack_frame:
    InterruptStackFrame,
) {
    // --------------------------------------------------------
    // Acknowledge IRQ 0.
    // --------------------------------------------------------

    unsafe {
        PICS
            .lock()
            .notify_end_of_interrupt(
                PIC_1_OFFSET,
            );
    }
}

// ============================================================
// General exception handler
// ============================================================

fn general_exception_handler(
    stack_frame:
    InterruptStackFrame,

    index:
    u8,

    error_code:
    Option<u64>,
) {
    serial::write_str(
        "\n\n=== CPU EXCEPTION ===\n",
    );

    serial::write_str(
        "VECTOR: ",
    );

    serial::write_usize(
        index as usize,
    );

    serial::write_str(
        "\nRIP: ",
    );

    serial::write_hex(
        stack_frame
            .instruction_pointer
            .as_u64(),
    );

    serial::write_str(
        "\nCS: ",
    );

    serial::write_hex(
        stack_frame
            .code_segment
            .0 as u64,
    );

    serial::write_str(
        "\nRSP: ",
    );

    serial::write_hex(
        stack_frame
            .stack_pointer
            .as_u64(),
    );

    serial::write_str(
        "\nSS: ",
    );

    serial::write_hex(
        stack_frame
            .stack_segment
            .0 as u64,
    );

    // --------------------------------------------------------
    // Page-fault specific information
    // --------------------------------------------------------

    if index == 14 {
        serial::write_str(
            "\nCR2: ",
        );

        serial::write_hex(
            x86_64::registers::control::Cr2::read()
                .unwrap()
                .as_u64(),
        );
    }

    // --------------------------------------------------------
    // Error code
    // --------------------------------------------------------

    serial::write_str(
        "\nERROR: ",
    );

    match error_code {
        Some(value) => {
            serial::write_hex(
                value,
            );

            // ------------------------------------------------
            // Decode page-fault error code
            //
            // Bit 0:
            //     0 = non-present page
            //     1 = protection violation
            //
            // Bit 1:
            //     0 = read
            //     1 = write
            //
            // Bit 2:
            //     0 = supervisor
            //     1 = user
            //
            // Bit 3:
            //     1 = reserved-bit violation
            //
            // Bit 4:
            //     1 = instruction fetch
            //
            // Bit 5:
            //     1 = protection-key violation
            //
            // Bit 6:
            //     1 = shadow-stack access
            //
            // Bit 7:
            //     1 = RMP violation
            //
            // ------------------------------------------------

            if index == 14 {
                serial::write_str(
                    "\nPAGE FAULT:",
                );

                if value & 1 != 0 {
                    serial::write_str(
                        " protection-violation",
                    );
                } else {
                    serial::write_str(
                        " non-present",
                    );
                }

                if value & 2 != 0 {
                    serial::write_str(
                        " write",
                    );
                } else {
                    serial::write_str(
                        " read",
                    );
                }

                if value & 4 != 0 {
                    serial::write_str(
                        " user",
                    );
                } else {
                    serial::write_str(
                        " supervisor",
                    );
                }

                if value & 8 != 0 {
                    serial::write_str(
                        " reserved-bit",
                    );
                }

                if value & 16 != 0 {
                    serial::write_str(
                        " instruction-fetch",
                    );
                }

                if value & 32 != 0 {
                    serial::write_str(
                        " protection-key",
                    );
                }

                if value & 64 != 0 {
                    serial::write_str(
                        " shadow-stack",
                    );
                }

                if value & 128 != 0 {
                    serial::write_str(
                        " rmp",
                    );
                }

                // --------------------------------------------
                // Convenient interpretation for the common
                // NX case:
                //
                //     protection violation
                //     user
                //     instruction fetch
                //
                // This usually means the target page is marked
                // NX / NO_EXECUTE.
                // --------------------------------------------

                if value & 1 != 0
                    && value & 4 != 0
                    && value & 16 != 0
                {
                    serial::write_str(
                        "\nLIKELY CAUSE: instruction fetch was rejected (possible NX/NO_EXECUTE page)",
                    );
                }
            }
        }

        None => {
            serial::write_str(
                "<none>",
            );
        }
    }

    serial::write_str(
        "\n====================\n",
    );

    halt();
}

// ============================================================
// Invalid opcode (#UD)
// ============================================================

extern "x86-interrupt" fn
invalid_opcode_handler(
    stack_frame:
    InterruptStackFrame,
) {
    serial::write_str(
        "\n\n=== INVALID OPCODE ===\n",
    );

    serial::write_str(
        "RIP: ",
    );

    serial::write_hex(
        stack_frame
            .instruction_pointer
            .as_u64(),
    );

    serial::write_str(
        "\nCS: ",
    );

    serial::write_hex(
        stack_frame
            .code_segment
            .0 as u64,
    );

    serial::write_str(
        "\n=======================\n",
    );

    halt();
}

// ============================================================
// Double fault (#DF)
// ============================================================

extern "x86-interrupt" fn
double_fault_handler(
    stack_frame:
    InterruptStackFrame,

    error_code:
    u64,
) -> ! {
    serial::write_str(
        "\n\n=== DOUBLE FAULT ===\n",
    );

    serial::write_str(
        "ERROR: ",
    );

    serial::write_hex(
        error_code,
    );

    serial::write_str(
        "\nRIP: ",
    );

    serial::write_hex(
        stack_frame
            .instruction_pointer
            .as_u64(),
    );

    serial::write_str(
        "\nCS: ",
    );

    serial::write_hex(
        stack_frame
            .code_segment
            .0 as u64,
    );

    serial::write_str(
        "\n=====================\n",
    );

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