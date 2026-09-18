//! Rusty platform implementation for ax-sync 0.6.x.

use core::{
    arch::asm,
    hint::spin_loop,
    panic::Location,
    sync::atomic::{
        AtomicBool,
        AtomicUsize,
        Ordering,
    },
};

use ax_crate_interface::impl_interface;

use ax_sync::interface::{
    AcquireResult,
    ContextOps,
    ContextState,
    LockMetadata,
    RwLockOps,
    SpinOps,
    CONTEXT_IRQSAVE,
    CONTEXT_PREEMPT,
    CONTEXT_PREEMPT_IRQSAVE,
    CONTEXT_RAW,
    LOCK_MODE_READ,
    LOCK_MODE_WRITE,
};

/// Implementation of ax-sync's execution-context interface.
pub struct RustyContextOps;

/// Implementation of ax-sync's spin-lock interface.
pub struct RustySpinOps;

/// Implementation of ax-sync's spin RW-lock interface.
pub struct RustyRwLockOps;

//
// x86_64 interrupt state
//

#[inline(always)]
fn irq_is_enabled() -> bool {
    let flags: u64;

    // SAFETY: Reading RFLAGS is valid in ring 0.
    unsafe {
        asm!(
        "pushfq",
        "pop {}",
        out(reg) flags,
        options(nomem, preserves_flags),
        );
    }

    (flags & (1 << 9)) != 0
}

#[inline(always)]
fn irq_disable() {
    // SAFETY: Rusty executes this in kernel mode.
    unsafe {
        asm!(
        "cli",
        options(nomem, nostack, preserves_flags),
        );
    }
}

#[inline(always)]
fn irq_enable() {
    // SAFETY: Rusty executes this in kernel mode.
    unsafe {
        asm!(
        "sti",
        options(nomem, nostack, preserves_flags),
        );
    }
}

//
// Execution context
//

#[inline(always)]
fn context_enter(context: u8) -> ContextState {
    match context {
        CONTEXT_RAW => {
            ContextState::new(0, 0)
        }

        CONTEXT_PREEMPT => {
            // Rusty does not currently have preemptive scheduling.
            ContextState::new(0, 0)
        }

        CONTEXT_IRQSAVE => {
            let was_enabled = irq_is_enabled();

            irq_disable();

            ContextState::new(
                0,
                was_enabled as usize,
            )
        }

        CONTEXT_PREEMPT_IRQSAVE => {
            // Preemption is currently a no-op.
            let was_enabled = irq_is_enabled();

            irq_disable();

            ContextState::new(
                0,
                was_enabled as usize,
            )
        }

        _ => {
            panic!("Rusty: invalid ax-sync context");
        }
    }
}

#[inline(always)]
fn context_exit(
    context: u8,
    state: ContextState,
) {
    match context {
        CONTEXT_RAW => {}

        CONTEXT_PREEMPT => {
            // Preemption is currently a no-op.
            let _ = state.preempt();
        }

        CONTEXT_IRQSAVE => {
            if state.irq() != 0 {
                irq_enable();
            }
        }

        CONTEXT_PREEMPT_IRQSAVE => {
            // Restore IRQs first, then preemption state.
            if state.irq() != 0 {
                irq_enable();
            }

            let _ = state.preempt();
        }

        _ => {
            panic!("Rusty: invalid ax-sync context");
        }
    }
}

//
// ContextOps
//

#[impl_interface]
impl ContextOps for RustyContextOps {
    fn enter(context: u8) -> ContextState {
        context_enter(context)
    }

    fn exit(
        context: u8,
        state: ContextState,
    ) {
        context_exit(context, state);
    }

    fn irq_return_preempt_enter() -> usize {
        // Rusty has no preemption counter yet.
        0
    }

    fn irq_return_preempt_exit(_state: usize) {
        // No-op until preemption is implemented.
    }

    fn hardirq_enter() {
        // No-op for now.
    }

    fn hardirq_exit() {
        // No-op for now.
    }
}

//
// SpinOps
//

#[impl_interface]
impl SpinOps for RustySpinOps {
    fn acquire(
        locked: &AtomicBool,
        _metadata: &LockMetadata,
        _lock_addr: usize,
        context: u8,
        _subclass: u32,
        _caller: &'static Location<'static>,
    ) -> ContextState {
        let context_state = context_enter(context);

        loop {
            if locked
                .compare_exchange(
                    false,
                    true,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return context_state;
            }

            spin_loop();
        }
    }

    fn try_acquire(
        locked: &AtomicBool,
        _metadata: &LockMetadata,
        _lock_addr: usize,
        context: u8,
        _subclass: u32,
        _caller: &'static Location<'static>,
    ) -> AcquireResult {
        let context_state = context_enter(context);

        match locked.compare_exchange(
            false,
            true,
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            Ok(_) => AcquireResult::new(
                true,
                context_state,
            ),

            Err(_) => {
                context_exit(
                    context,
                    context_state,
                );

                AcquireResult::new(
                    false,
                    ContextState::new(0, 0),
                )
            }
        }
    }

    fn release(
        locked: &AtomicBool,
        _lock_addr: usize,
        context: u8,
        context_state: ContextState,
    ) {
        locked.store(
            false,
            Ordering::Release,
        );

        context_exit(
            context,
            context_state,
        );
    }

    fn force_release(
        locked: &AtomicBool,
        _lock_addr: usize,
        _context: u8,
    ) {
        locked.store(
            false,
            Ordering::Release,
        );
    }

    fn is_locked(
        locked: &AtomicBool,
    ) -> bool {
        locked.load(Ordering::Acquire)
    }
}

//
// RwLockOps
//
// State:
//
// bit 63 = writer owns lock
// bits 0..62 = reader count
//

const RW_WRITER_BIT: usize = 1usize << (usize::BITS - 1);
const RW_READER_MASK: usize = RW_WRITER_BIT - 1;

#[inline(always)]
fn rw_read_try_acquire(
    state: &AtomicUsize,
) -> bool {
    loop {
        let current = state.load(Ordering::Acquire);

        if (current & RW_WRITER_BIT) != 0 {
            return false;
        }

        let readers = current & RW_READER_MASK;

        if readers == RW_READER_MASK {
            panic!("Rusty: ax-sync rwlock reader overflow");
        }

        let next = current + 1;

        match state.compare_exchange_weak(
            current,
            next,
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            Ok(_) => return true,
            Err(_) => spin_loop(),
        }
    }
}

#[inline(always)]
fn rw_write_try_acquire(
    state: &AtomicUsize,
) -> bool {
    state
        .compare_exchange(
            0,
            RW_WRITER_BIT,
            Ordering::Acquire,
            Ordering::Relaxed,
        )
        .is_ok()
}

#[impl_interface]
impl RwLockOps for RustyRwLockOps {
    fn acquire(
        state: &AtomicUsize,
        _metadata: &LockMetadata,
        _lock_addr: usize,
        context: u8,
        mode: u8,
        _caller: &'static Location<'static>,
    ) -> ContextState {
        let context_state = context_enter(context);

        match mode {
            LOCK_MODE_READ => {
                loop {
                    if rw_read_try_acquire(state) {
                        return context_state;
                    }

                    spin_loop();
                }
            }

            LOCK_MODE_WRITE => {
                loop {
                    if rw_write_try_acquire(state) {
                        return context_state;
                    }

                    spin_loop();
                }
            }

            _ => {
                context_exit(
                    context,
                    context_state,
                );

                panic!("Rusty: invalid ax-sync rwlock mode");
            }
        }
    }

    fn try_acquire(
        state: &AtomicUsize,
        _metadata: &LockMetadata,
        _lock_addr: usize,
        context: u8,
        mode: u8,
        _caller: &'static Location<'static>,
    ) -> AcquireResult {
        let context_state = context_enter(context);

        let acquired = match mode {
            LOCK_MODE_READ => rw_read_try_acquire(state),

            LOCK_MODE_WRITE => rw_write_try_acquire(state),

            _ => {
                context_exit(
                    context,
                    context_state,
                );

                panic!("Rusty: invalid ax-sync rwlock mode");
            }
        };

        if acquired {
            AcquireResult::new(
                true,
                context_state,
            )
        } else {
            context_exit(
                context,
                context_state,
            );

            AcquireResult::new(
                false,
                ContextState::new(0, 0),
            )
        }
    }

    fn release(
        state: &AtomicUsize,
        _lock_addr: usize,
        context: u8,
        context_state: ContextState,
        mode: u8,
    ) {
        match mode {
            LOCK_MODE_READ => {
                let previous = state.fetch_sub(
                    1,
                    Ordering::Release,
                );

                if previous == 0
                    || (previous & RW_WRITER_BIT) != 0
                {
                    panic!(
                        "Rusty: invalid ax-sync rwlock read release"
                    );
                }
            }

            LOCK_MODE_WRITE => {
                let previous = state.swap(
                    0,
                    Ordering::Release,
                );

                if previous != RW_WRITER_BIT {
                    panic!(
                        "Rusty: invalid ax-sync rwlock write release"
                    );
                }
            }

            _ => {
                panic!(
                    "Rusty: invalid ax-sync rwlock release mode"
                );
            }
        }

        context_exit(
            context,
            context_state,
        );
    }

    fn force_read_decrement(
        state: &AtomicUsize,
        _lock_addr: usize,
        _context: u8,
    ) {
        let previous = state.fetch_sub(
            1,
            Ordering::Release,
        );

        if previous == 0
            || (previous & RW_WRITER_BIT) != 0
        {
            panic!(
                "Rusty: invalid ax-sync rwlock force read decrement"
            );
        }
    }
}