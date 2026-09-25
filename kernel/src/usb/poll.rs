// ============================================================
// Rusty USB polling
// ============================================================
//
// The xHCI IDT handler in interrupts.rs deliberately does NOT process the
// xHCI event ring. It only:
//
//     1. records the interrupt in XHCI_INTERRUPT_PENDING
//     2. sends LAPIC EOI
//
// This file is the normal kernel-context side of that design.
//
// poll() asks the custom XhciDriver to service a pending interrupt. The
// driver consumes the software pending flag, checks the controller's own
// interrupt state as a fallback, and then drains the event ring from normal
// context.
//
// ============================================================

use super::init;

// ============================================================
// One USB polling pass
// ============================================================
//
// Call this repeatedly from Rusty's existing kernel/service loop.
//
// ============================================================

#[inline]
pub fn poll() {
    if !init::is_initialized() {
        return;
    }

    // SAFETY:
    //
    // Rusty's current USB state is accessed from the single normal kernel
    // service context. The xHCI interrupt handler never accesses this mutable
    // state; it only updates the atomic pending flag in interrupts.rs.
    unsafe {
        let state = init::state_mut();
        state.xhci.service_interrupt();
    }
}
