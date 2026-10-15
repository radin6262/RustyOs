use core::{
    arch::x86_64::__cpuid,
    cell::UnsafeCell,
    mem::MaybeUninit,
    ptr::{read_volatile, write_volatile},
    sync::atomic::{AtomicBool, Ordering},
};

use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::{
    set_general_handler,
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame},
};

use crate::{memory, serial, syscall};

// ============================================================
// PIC configuration
// ============================================================

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

// Vector for the local APIC spurious-interrupt handler. It must stay
// outside the remapped 8259 PIC range 32..47.
pub const APIC_SPURIOUS_VECTOR: u8 = 0xFF;

// ============================================================
// PS/2 interrupt lines
// ============================================================

/// IRQ1: PS/2 keyboard.
pub const IRQ_KEYBOARD: u8 = PIC_1_OFFSET + 1;

/// IRQ2: chains the slave PIC into the master. IRQ12 never reaches the CPU
/// while this line is masked.
const IRQ_CASCADE: u8 = PIC_1_OFFSET + 2;

/// IRQ12: PS/2 mouse (line 4 on the slave PIC).
pub const IRQ_MOUSE: u8 = PIC_2_OFFSET + 4;

/// Masks written to the 8259 PICs. Bit N of the master mask covers IRQ N;
/// bit N of the slave mask covers IRQ N + 8. Everything else, including the
/// unused PIT line, stays masked.
const MASTER_IRQ_MASK: u8 = 0xFF
    & !(1u8 << (IRQ_KEYBOARD - PIC_1_OFFSET))
    & !(1u8 << (IRQ_CASCADE - PIC_1_OFFSET));

const SLAVE_IRQ_MASK: u8 = 0xFF & !(1u8 << (IRQ_MOUSE - PIC_2_OFFSET));

pub static PICS: Mutex<ChainedPics> = Mutex::new(unsafe {
    ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET)
});

// ============================================================
// Local APIC state
// ============================================================
//
// PS/2 input is delivered through the 8259 PIC, but the local APIC is still
// enabled so that its spurious-interrupt vector remains a known, harmless
// IDT entry.
// ============================================================

static LOCAL_APIC_READY: AtomicBool = AtomicBool::new(false);
static LOCAL_APIC_X2APIC: AtomicBool = AtomicBool::new(false);
static LOCAL_APIC_ID: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0);
static LOCAL_APIC_BASE: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

// ============================================================
// IDT static storage
// ============================================================

struct IdtStorage {
    idt: UnsafeCell<MaybeUninit<InterruptDescriptorTable>>,
}

unsafe impl Sync for IdtStorage {}

impl IdtStorage {
    const fn new() -> Self {
        Self {
            idt: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
}

static IDT_STORAGE: IdtStorage = IdtStorage::new();

// ============================================================
// Local APIC constants
// ============================================================

const IA32_APIC_BASE_MSR: u32 = 0x1B;
const IA32_X2APIC_APIC_ID: u32 = 0x802;
const IA32_X2APIC_SIVR: u32 = 0x80F;

const APIC_BASE_GLOBAL_ENABLE: u64 = 1 << 11;
const APIC_BASE_X2APIC_ENABLE: u64 = 1 << 10;
const APIC_BASE_ADDRESS_MASK: u64 = 0xFFFF_FFFF_FFFF_F000;

const APIC_ID_OFFSET: usize = 0x020;
const APIC_SVR_OFFSET: usize = 0x0F0;

const APIC_SVR_ENABLE: u32 = 1 << 8;
const APIC_SVR_VECTOR_MASK: u32 = 0xFF;

// ============================================================
// Raw MSR helpers
// ============================================================

#[inline(always)]
unsafe fn read_msr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;

    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags),
        );
    }

    ((high as u64) << 32) | low as u64
}

#[inline(always)]
unsafe fn write_msr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;

    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
            options(nomem, nostack, preserves_flags),
        );
    }
}

// ============================================================
// Local APIC initialization
// ============================================================

unsafe fn init_local_apic() {
    let cpuid = unsafe { __cpuid(1) };

    // CPUID.01H:EDX[5] = MSR instruction support.
    if cpuid.edx & (1 << 5) == 0 {
        panic!("Rusty: CPU does not advertise MSR instructions");
    }

    // CPUID.01H:EDX[9] = local APIC.
    if cpuid.edx & (1 << 9) == 0 {
        panic!("Rusty: CPU does not advertise a local APIC");
    }

    let mut apic_base_msr = unsafe { read_msr(IA32_APIC_BASE_MSR) };

    // Ensure the local APIC is globally enabled.
    if apic_base_msr & APIC_BASE_GLOBAL_ENABLE == 0 {
        apic_base_msr |= APIC_BASE_GLOBAL_ENABLE;
        unsafe { write_msr(IA32_APIC_BASE_MSR, apic_base_msr) };
    }

    let x2apic = (apic_base_msr & APIC_BASE_X2APIC_ENABLE) != 0;
    LOCAL_APIC_X2APIC.store(x2apic, Ordering::Release);

    let base = apic_base_msr & APIC_BASE_ADDRESS_MASK;
    LOCAL_APIC_BASE.store(base, Ordering::Release);

    if x2apic {
        // In x2APIC mode the APIC register file is MSR-based.
        let mut sivr = unsafe { read_msr(IA32_X2APIC_SIVR) as u32 };
        sivr |= APIC_SVR_ENABLE;
        sivr = (sivr & !APIC_SVR_VECTOR_MASK) | APIC_SPURIOUS_VECTOR as u32;
        unsafe { write_msr(IA32_X2APIC_SIVR, sivr as u64) };

        let apic_id = unsafe { read_msr(IA32_X2APIC_APIC_ID) as u32 };
        LOCAL_APIC_ID.store(apic_id, Ordering::Release);

        serial::write_str("APIC: enabled in x2APIC mode, ID=");
        serial::write_hex(apic_id as u64);
        serial::write_str("\n");
    } else {
        // xAPIC mode exposes the register file at IA32_APIC_BASE.
        if base == 0 {
            panic!("Rusty: local APIC base is zero");
        }

        // Identity mapping is required because the CPU's xAPIC registers are
        // accessed as physical MMIO.
        unsafe {
            memory::identity_map_mmio(base as usize, 0x1000);
        }

        let apic_base = base as usize;

        let mut sivr = unsafe { read_volatile((apic_base + APIC_SVR_OFFSET) as *const u32) };
        sivr |= APIC_SVR_ENABLE;
        sivr = (sivr & !APIC_SVR_VECTOR_MASK) | APIC_SPURIOUS_VECTOR as u32;
        unsafe { write_volatile((apic_base + APIC_SVR_OFFSET) as *mut u32, sivr) };

        let apic_id = unsafe { read_volatile((apic_base + APIC_ID_OFFSET) as *const u32) >> 24 };
        LOCAL_APIC_ID.store(apic_id, Ordering::Release);

        serial::write_str("APIC: enabled in xAPIC mode, base=");
        serial::write_hex(base);
        serial::write_str(" ID=");
        serial::write_hex(apic_id as u64);
        serial::write_str("\n");
    }

    LOCAL_APIC_READY.store(true, Ordering::Release);
}

// ============================================================
// Initialize IDT + PIC + LAPIC
// ============================================================

pub fn init() {
    let mut idt = InterruptDescriptorTable::new();

    set_general_handler!(&mut idt, general_exception_handler, 12..15);

    idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
    idt.double_fault.set_handler_fn(double_fault_handler);

    idt[PIC_1_OFFSET].set_handler_fn(timer_interrupt_handler);

    // PS/2 input lines.
    idt[IRQ_KEYBOARD].set_handler_fn(keyboard_interrupt_handler);
    idt[IRQ_MOUSE].set_handler_fn(mouse_interrupt_handler);

    idt[APIC_SPURIOUS_VECTOR].set_handler_fn(apic_spurious_interrupt_handler);

    unsafe {
        syscall::install(&mut idt);
    }

    unsafe {
        (*IDT_STORAGE.idt.get()).write(idt);
    }

    let idt_ref: &'static InterruptDescriptorTable = unsafe {
        &*((*IDT_STORAGE.idt.get()).as_ptr())
    };

    unsafe {
        idt_ref.load();
    }

    unsafe {
        let mut pics = PICS.lock();

        pics.initialize();

        // initialize() restores the mask the firmware left behind, so the
        // lines Rusty uses are programmed explicitly instead.
        pics.write_masks(MASTER_IRQ_MASK, SLAVE_IRQ_MASK);
    }

    unsafe {
        init_local_apic();
    }
}

// ============================================================
// Explicit CPU interrupt enable
// ============================================================

pub fn enable() {
    x86_64::instructions::interrupts::enable();
}

pub fn disable() {
    x86_64::instructions::interrupts::disable();
}

// ============================================================
// Hardware timer interrupt
// ============================================================

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    unsafe {
        PICS.lock().notify_end_of_interrupt(PIC_1_OFFSET);
    }
}

// ============================================================
// PS/2 keyboard (IRQ1)
// ============================================================

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // The handler only moves bytes from the i8042 into the input queue.
    // Decoding and event delivery stay out of interrupt context.
    crate::input::ps2::handle_keyboard_interrupt();

    unsafe {
        PICS.lock().notify_end_of_interrupt(IRQ_KEYBOARD);
    }
}

// ============================================================
// PS/2 mouse (IRQ12)
// ============================================================

extern "x86-interrupt" fn mouse_interrupt_handler(_stack_frame: InterruptStackFrame) {
    crate::input::ps2::handle_mouse_interrupt();

    unsafe {
        PICS.lock().notify_end_of_interrupt(IRQ_MOUSE);
    }
}

// ============================================================
// APIC spurious interrupt
// ============================================================

extern "x86-interrupt" fn apic_spurious_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // Spurious APIC interrupts are not EOI'd.
}

// ============================================================
// General exception handler
// ============================================================

fn general_exception_handler(
    stack_frame: InterruptStackFrame,
    index: u8,
    error_code: Option<u64>,
) {
    serial::write_str("\n\n=== CPU EXCEPTION ===\n");
    serial::write_str("VECTOR: ");
    serial::write_usize(index as usize);

    serial::write_str("\nRIP: ");
    serial::write_hex(stack_frame.instruction_pointer.as_u64());

    serial::write_str("\nCS: ");
    serial::write_hex(stack_frame.code_segment.0 as u64);

    serial::write_str("\nRSP: ");
    serial::write_hex(stack_frame.stack_pointer.as_u64());

    serial::write_str("\nSS: ");
    serial::write_hex(stack_frame.stack_segment.0 as u64);

    if index == 14 {
        serial::write_str("\nCR2: ");
        serial::write_hex(x86_64::registers::control::Cr2::read().unwrap().as_u64());
    }

    serial::write_str("\nERROR: ");

    match error_code {
        Some(value) => {
            serial::write_hex(value);

            if index == 14 {
                serial::write_str("\nPAGE FAULT:");

                if value & 1 != 0 {
                    serial::write_str(" protection-violation");
                } else {
                    serial::write_str(" non-present");
                }

                if value & 2 != 0 {
                    serial::write_str(" write");
                } else {
                    serial::write_str(" read");
                }

                if value & 4 != 0 {
                    serial::write_str(" user");
                } else {
                    serial::write_str(" supervisor");
                }

                if value & 8 != 0 {
                    serial::write_str(" reserved-bit");
                }
                if value & 16 != 0 {
                    serial::write_str(" instruction-fetch");
                }
                if value & 32 != 0 {
                    serial::write_str(" protection-key");
                }
                if value & 64 != 0 {
                    serial::write_str(" shadow-stack");
                }
                if value & 128 != 0 {
                    serial::write_str(" rmp");
                }

                if value & 1 != 0 && value & 4 != 0 && value & 16 != 0 {
                    serial::write_str(
                        "\nLIKELY CAUSE: instruction fetch was rejected (possible NX/NO_EXECUTE page)",
                    );
                }
            }
        }
        None => {
            serial::write_str("<none>");
        }
    }

    serial::write_str("\n====================\n");
    halt();
}

// ============================================================
// Invalid opcode (#UD)
// ============================================================

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    serial::write_str("\n\n=== INVALID OPCODE ===\n");

    serial::write_str("RIP: ");
    serial::write_hex(stack_frame.instruction_pointer.as_u64());

    serial::write_str("\nCS: ");
    serial::write_hex(stack_frame.code_segment.0 as u64);

    serial::write_str("\n=======================\n");
    halt();
}

// ============================================================
// Double fault (#DF)
// ============================================================

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) -> ! {
    serial::write_str("\n\n=== DOUBLE FAULT ===\n");

    serial::write_str("ERROR: ");
    serial::write_hex(error_code);

    serial::write_str("\nRIP: ");
    serial::write_hex(stack_frame.instruction_pointer.as_u64());

    serial::write_str("\nCS: ");
    serial::write_hex(stack_frame.code_segment.0 as u64);

    serial::write_str("\n=====================\n");
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
