// ============================================================
// Idle desktop
// ============================================================
//
// The compositor is serviced from syscall entry, which means the
// desktop froze as soon as the last userspace process exited.
//
// This loop takes over on the syscall HALT path:
//
//     - it polls USB HID and services the compositor, so the cursor,
//       dock magnification and window controls keep working
//     - clicking the Terminal dock icon starts a fresh Terminal
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
        crate::syscall::service_idle_input();

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

        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}
