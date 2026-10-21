// ============================================================
// Idle desktop
// ============================================================
//
// The compositor is serviced from syscall entry, which means the
// desktop froze as soon as the last userspace process exited.
//
// This loop takes over on the syscall HALT path:
//
//     - it sleeps in `hlt` and wakes on the periodic kernel tick
//     - once per tick it polls USB HID and services the compositor, so
//       the cursor, dock magnification and window controls keep working
//     - clicking the Terminal dock icon starts a fresh Terminal
//
// The tick is what lets the desktop sleep at all: USB is polled rather
// than interrupt driven, so without a periodic wake-up an `hlt` would
// never read another USB report. PS/2 reports still wake it sooner.
//
// The kernel page table is already active again by the time this
// runs, so the dead process's memory has been handed back.
//
// ============================================================

/// Embedded terminal used when the dock starts a fresh app.
pub static TERMINAL_ELF: &[u8] =
    include_bytes!("../../user/terminal/Terminal");

#[unsafe(no_mangle)]
pub extern "C" fn rusty_desktop_idle() -> ! {
    crate::serial::write_str("DESKTOP: idle\n");

    // The syscall halt path arrives with interrupts disabled.
    crate::interrupts::enable();

    // Present the final state once: the exiting process may have owned
    // windows that were removed without a composite.
    {
        let mut wm_lock =
            crate::wm::WM.lock();

        if let Some(compositor) =
            wm_lock.as_mut()
        {
            compositor.draw();
        }
    }

    loop {
        // ----------------------------------------------------
        // Input
        // ----------------------------------------------------
        //
        // Service input on every wake-up. The APIC timer (with the PIT as a
        // fallback) guarantees a wake-up even when no PS/2 interrupt arrives,
        // because USB HID is polled and the xHCI driver leaves its interrupter
        // disabled. Servicing unconditionally also means a PS/2 report wakes
        // the loop even if the timer tick is unavailable.
        //
        // Interrupts are disabled while the input queues are touched: an IRQ
        // arriving in the middle of a poll would try to take the same locks.

        crate::interrupts::disable();

        crate::syscall::service_idle_input();

        crate::interrupts::enable();

        let launch =
            match crate::wm::WM.lock().as_mut() {
                Some(compositor) =>
                    compositor.take_launch_request(),

                None =>
                    false,
            };

        if launch {
            crate::serial::write_str(
                "DESKTOP: launching terminal\n",
            );

            crate::launch_app::run(
                crate::cpu::gdt::init(),
                TERMINAL_ELF,
            );
        }

        // ----------------------------------------------------
        // Sleep
        // ----------------------------------------------------
        //
        // `hlt` until the next interrupt: the kernel tick within a
        // millisecond, or a PS/2 report sooner. No busy polling.

        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}
