// ============================================================
// Periodic kernel tick
// ============================================================
//
// The 8254 PIT (channel 0) raises IRQ0 at a fixed rate. The tick
// exists so idle code has a wake-up source that does not depend on
// an input device:
//
//   - PS/2 input is interrupt driven, so it wakes `hlt` by itself.
//   - USB HID is polled (the xHCI driver leaves its interrupter
//     disabled), so a CPU parked in `hlt` would never read another
//     report unless something else interrupts it.
//
// The desktop therefore sleeps in `hlt` and polls USB once per tick.
//
// `ticks()` counts interrupts since boot and is the base other
// periodic work can be scheduled against.
// ============================================================

use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::instructions::port::Port;

// ============================================================
// PIT configuration
// ============================================================

/// Ticks per second. 1 kHz gives the idle desktop a 1 ms input poll
/// while keeping the interrupt handler trivial.
pub const TICK_HZ: u64 = 1000;

/// The 8254 input clock.
const PIT_FREQUENCY: u32 = 1_193_182;

const PIT_CHANNEL0: u16 = 0x40;
const PIT_COMMAND: u16 = 0x43;

/// Channel 0, access lobyte/hibyte, mode 2 (rate generator).
///
/// Mode 2 reloads the counter automatically, which is what makes the
/// tick periodic without any further programming.
const PIT_COMMAND_RATE: u8 = 0x34;

// ============================================================
// State
// ============================================================

static TICKS: AtomicU64 = AtomicU64::new(0);

// ============================================================
// Initialize
// ============================================================

/// Programs PIT channel 0 to [`TICK_HZ`].
///
/// IRQ0 stays masked in the PIC until [`crate::interrupts::init`]
/// unmasks it, so this only has to install the rate.
pub fn init() {
    let divisor = (PIT_FREQUENCY as u64 / TICK_HZ).max(1) as u16;

    let mut command = Port::<u8>::new(PIT_COMMAND);
    let mut channel0 = Port::<u8>::new(PIT_CHANNEL0);

    unsafe {
        command.write(PIT_COMMAND_RATE);

        channel0.write((divisor & 0xFF) as u8);
        channel0.write((divisor >> 8) as u8);
    }

    crate::serial::write_str("TIMER: kernel tick at ");
    crate::serial::write_usize(TICK_HZ as usize);
    crate::serial::write_str(" Hz, divisor=");
    crate::serial::write_usize(divisor as usize);
    crate::serial::write_str("\n");
}

// ============================================================
// Tick
// ============================================================

/// Called from the IRQ0 handler.
pub fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

/// Number of ticks since boot.
pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}
