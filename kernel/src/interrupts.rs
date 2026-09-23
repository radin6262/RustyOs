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
//
// PIC IRQ layout:
//
// PIC 1:
//   IRQ 0 -> vector 32 (0x20)
//   IRQ 1 -> vector 33 (0x21)
//   ...
//   IRQ 7 -> vector 39 (0x27)
//
// PIC 2:
//   IRQ 8  -> vector 40 (0x28)
//   ...
//   IRQ 15 -> vector 47 (0x2F)
//
// Therefore vectors 32..47 are reserved for the remapped PIC.
//
// xHCI uses a separate vector.
//
// ============================================================

pub const PIC_1_OFFSET: u8 = 32;

pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

// Dedicated xHCI interrupt vector.
//
// Keep this outside the remapped PIC range.
//
// 0x50 = 80 decimal.
//
// This is suitable as a dedicated MSI/MSI-X vector.
//
pub const XHCI_INTERRUPT_VECTOR: u8 = 0x50;

// ============================================================
// PIC
// ============================================================

pub static PICS: Mutex<ChainedPics> = Mutex::new(
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
//
// The IDT must remain alive after init() returns.
//
// We therefore construct it in static storage and load a
// &'static reference to it.
//
// ============================================================

struct IdtStorage {
    idt: UnsafeCell<
        MaybeUninit<InterruptDescriptorTable>,
    >,
}

unsafe impl Sync for IdtStorage {}

impl IdtStorage {
    const fn new() -> Self {
        Self {
            idt: UnsafeCell::new(
                MaybeUninit::uninit(),
            ),
        }
    }
}

static IDT_STORAGE: IdtStorage =
    IdtStorage::new();

// ============================================================
// Initialize IDT
// ============================================================

pub fn init() {
    // --------------------------------------------------------
    // Construct a completely new IDT.
    // --------------------------------------------------------

    let mut idt =
        InterruptDescriptorTable::new();

    // --------------------------------------------------------
    // General exception handler
    //
    // #SS = 12
    // #GP = 13
    // #PF = 14
    //
    // set_general_handler! generates the required wrapper
    // functions for the different exception types.
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
    // Hardware timer
    //
    // IRQ 0
    //     |
    //     +----> vector 32 / 0x20
    // --------------------------------------------------------

    idt[PIC_1_OFFSET]
        .set_handler_fn(
            timer_interrupt_handler,
        );

    // --------------------------------------------------------
    // xHCI interrupt
    //
    // IMPORTANT:
    //
    // InterruptDescriptorTable implements Index<u8>,
    // NOT Index<usize>.
    //
    // XHCI_INTERRUPT_VECTOR is already u8, so DO NOT do:
    //
    //     XHCI_INTERRUPT_VECTOR as usize
    //
    // Use the u8 directly.
    // --------------------------------------------------------

    idt[XHCI_INTERRUPT_VECTOR]
        .set_handler_fn(
            xhci_interrupt_handler,
        );

    // --------------------------------------------------------
    // Syscall
    // --------------------------------------------------------
    //
    // This installs the existing syscall entry point.
    //
    // The syscall implementation owns its own ABI/assembly
    // handling, so we leave that code untouched.
    // --------------------------------------------------------

    unsafe {
        syscall::install(
            &mut idt,
        );
    }

    // --------------------------------------------------------
    // Move IDT into permanent static storage.
    // --------------------------------------------------------

    unsafe {
        (*IDT_STORAGE.idt.get())
            .write(idt);
    }

    // --------------------------------------------------------
    // Obtain the permanent IDT reference.
    // --------------------------------------------------------

    let idt_ref:
        &'static InterruptDescriptorTable =
        unsafe {
            &*(
                (*IDT_STORAGE.idt.get())
                    .as_ptr()
            )
        };

    // --------------------------------------------------------
    // Load IDTR.
    // --------------------------------------------------------

    unsafe {
        idt_ref.load();
    }

    // --------------------------------------------------------
    // Initialize the remapped PIC.
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
// IRQ 0 -> PIC vector 32.
//
// We deliberately do NOT perform scheduling here.
//
// A normal x86-interrupt Rust handler only receives the CPU
// interrupt stack frame. It does not expose all GPRs:
//
//     rax rbx rcx rdx
//     rsi rdi rbp
//     r8-r15
//
// Your scheduler requires the complete SavedUserContext.
//
// Therefore:
//
//     timer IRQ
//          |
//          v
//     assembly entry
//          |
//          v
//     save GPRs
//          |
//          v
//     build SavedUserContext
//          |
//          v
//     scheduler
//          |
//          v
//     restore context
//          |
//          v
//        iretq
//
// Do not fake preemptive scheduling by calling the scheduler
// directly from this Rust handler.
//

extern "x86-interrupt" fn
timer_interrupt_handler(
    _stack_frame: InterruptStackFrame,
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
// xHCI Interrupt
// ============================================================
//
// This handler is intentionally lightweight.
//
// The xHCI controller should be configured to generate an
// interrupt using the XHCI_INTERRUPT_VECTOR vector.
//
// Do not perform large USB operations directly from the IDT
// handler.
//
// The normal design is:
//
//     xHCI hardware
//          |
//          v
//     IDT handler
//          |
//          v
//     acknowledge / mark pending
//          |
//          v
//     USB poll/service path
//          |
//          v
//     CrabUSB
//
// This is especially important because CrabUSB is asynchronous.
// Its event processing should remain in the normal USB service
// path rather than doing enumeration/control transfers from
// interrupt context.
//

extern "x86-interrupt" fn
xhci_interrupt_handler(
    _stack_frame: InterruptStackFrame,
) {
    // --------------------------------------------------------
    // Record that xHCI generated an interrupt.
    //
    // The actual xHCI event-ring processing remains in the
    // normal USB polling/service path.
    // --------------------------------------------------------

    serial::write_str(
        "INTERRUPT: xHCI\n",
    );
}

// ============================================================
// General exception handler
// ============================================================

fn general_exception_handler(
    stack_frame: InterruptStackFrame,
    index: u8,
    error_code: Option<u64>,
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

                // ------------------------------------------------
                // Common NX interpretation:
                //
                // protection violation
                // + user
                // + instruction fetch
                //
                // This is consistent with attempting to execute
                // from a page that is not executable.
                // ------------------------------------------------

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
    stack_frame: InterruptStackFrame,
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
    stack_frame: InterruptStackFrame,
    error_code: u64,
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