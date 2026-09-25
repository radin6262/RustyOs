// ============================================================
// Rusty custom USB polling
// ============================================================
//
// This is the normal-context side of the xHCI MSI/MSI-X interrupt path.
//
// The xHCI IDT handler does NOT consume the event ring. It only records the
// interrupt and sends LAPIC EOI. The custom xHCI driver remains the sole event
// ring consumer.
//
// ============================================================

use super::{hid, init};

#[inline]
pub fn poll() {
    if !init::is_initialized() {
        return;
    }

    unsafe {
        let state = init::state_mut();

        // First consume xHCI command/port/transfer events.
        state.xhci.service_interrupt();

        // Then consume HID transfer completions and re-arm the endpoints.
        state.hid.poll(&mut state.xhci);
    }
}
