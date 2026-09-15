use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{
    AtomicBool,
    AtomicU64,
    Ordering,
};

use x86_64::{
    registers::control::{
        Cr3,
        Cr3Flags,
    },
    structures::paging::{
        PhysFrame,
        Size4KiB,
    },
    VirtAddr,
};

pub use crate::{
    cpu::gdt::Selectors,
    user::{
        address_space::UserAddressSpace,
        entry,
    },
};

// ============================================================
// Configuration
// ============================================================

const MAX_PROCESSES: usize = 64;

// ============================================================
// Simple spin lock
// ============================================================

struct SimpleLock<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SimpleLock<T> {}

impl<T> SimpleLock<T> {
    const fn new(
        value: T,
    ) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(value),
        }
    }

    fn lock(
        &self,
    ) -> SimpleLockGuard<'_, T> {
        while self
            .locked
            .compare_exchange(
                false,
                true,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_err()
        {
            core::hint::spin_loop();
        }

        SimpleLockGuard {
            lock: self,
        }
    }
}

struct SimpleLockGuard<'a, T> {
    lock: &'a SimpleLock<T>,
}

impl<T> core::ops::Deref
for SimpleLockGuard<'_, T>
{
    type Target = T;

    fn deref(
        &self,
    ) -> &T {
        unsafe {
            &*self.lock.data.get()
        }
    }
}

impl<T> core::ops::DerefMut
for SimpleLockGuard<'_, T>
{
    fn deref_mut(
        &mut self,
    ) -> &mut T {
        unsafe {
            &mut *self.lock.data.get()
        }
    }
}

impl<T> Drop
for SimpleLockGuard<'_, T>
{
    fn drop(
        &mut self,
    ) {
        self.lock
            .locked
            .store(
                false,
                Ordering::Release,
            );
    }
}

// ============================================================
// Process state
// ============================================================

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
)]
pub enum ProcessState {
    Ready,
    Running,
    Exited,
}

// ============================================================
// Saved user context
// ============================================================

#[derive(
    Clone,
    Copy,
)]
#[repr(C)]
pub struct SavedUserContext {
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

    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

// ============================================================
// Process
// ============================================================

pub struct Process {
    pub pid: u64,

    /// Root page table used when this process runs.
    pub level_4_frame:
        PhysFrame<Size4KiB>,

    pub state:
        ProcessState,

    pub context:
        SavedUserContext,
}

impl Process {
    fn new(
        pid: u64,
        address_space: &UserAddressSpace,
        selectors: Selectors,
    ) -> Self {
        Self {
            pid,

            level_4_frame:
            address_space.level_4_frame(),

            state:
            ProcessState::Ready,

            context:
            SavedUserContext {
                rax: 0,
                rbx: 0,
                rcx: 0,
                rdx: 0,
                rsi: 0,
                rdi: 0,
                rbp: 0,
                r8: 0,
                r9: 0,
                r10: 0,
                r11: 0,
                r12: 0,
                r13: 0,
                r14: 0,
                r15: 0,

                rip:
                address_space
                    .user_code_address()
                    .as_u64(),

                cs:
                selectors
                    .user_code
                    .0 as u64,

                // Required RFLAGS bit.
                // Interrupts remain disabled
                // until the interrupt/timer path is
                // ready.
                rflags:
                1 << 1,

                rsp:
                address_space
                    .user_stack_top()
                    .as_u64(),

                ss:
                selectors
                    .user_data
                    .0 as u64,
            },
        }
    }
}

// ============================================================
// Static process slot
// ============================================================

struct ProcessSlot {
    occupied: bool,
    process:
        MaybeUninit<Process>,
}

impl ProcessSlot {
    const fn new() -> Self {
        Self {
            occupied: false,
            process:
            MaybeUninit::uninit(),
        }
    }

    unsafe fn process_ref(
        &self,
    ) -> &Process {
        unsafe {
            self.process
                .assume_init_ref()
        }
    }

    unsafe fn process_mut(
        &mut self,
    ) -> &mut Process {
        unsafe {
            self.process
                .assume_init_mut()
        }
    }
}

// ============================================================
// Process manager
// ============================================================
//
// IMPORTANT:
//
// This structure is constructed directly inside static
// kernel memory.
//
// It is NEVER first constructed as a local stack variable.
//
// That avoids blowing the tiny early kernel stack.
//

struct ProcessManager {
    processes:
        [ProcessSlot; MAX_PROCESSES],

    current:
        Option<u64>,
}

impl ProcessManager {
    const fn new() -> Self {
        Self {
            processes:
            [const {
                ProcessSlot::new()
            }; MAX_PROCESSES],

            current:
            None,
        }
    }

    fn find_process(
        &self,
        pid: u64,
    ) -> Option<&Process> {
        for slot
        in self.processes.iter()
        {
            if !slot.occupied {
                continue;
            }

            let process =
                unsafe {
                    slot.process_ref()
                };

            if process.pid == pid {
                return Some(process);
            }
        }

        None
    }

    fn find_process_mut(
        &mut self,
        pid: u64,
    ) -> Option<&mut Process> {
        for slot
        in self.processes.iter_mut()
        {
            if !slot.occupied {
                continue;
            }

            let process =
                unsafe {
                    slot.process_mut()
                };

            if process.pid == pid {
                return Some(process);
            }
        }

        None
    }

    fn find_free_slot(
        &self,
    ) -> Option<usize> {
        self.processes
            .iter()
            .position(|slot| {
                !slot.occupied
            })
    }

    fn find_next_ready(
        &self,
        current_pid: u64,
    ) -> Option<usize> {
        let current_index =
            self.processes
                .iter()
                .position(|slot| {
                    if !slot.occupied {
                        return false;
                    }

                    let process =
                        unsafe {
                            slot.process_ref()
                        };

                    process.pid
                        == current_pid
                })
                .unwrap_or(0);

        for offset
        in 1..=MAX_PROCESSES
        {
            let index =
                (current_index + offset)
                    % MAX_PROCESSES;

            let slot =
                &self.processes[index];

            if !slot.occupied {
                continue;
            }

            let process =
                unsafe {
                    slot.process_ref()
                };

            if process.state
                == ProcessState::Ready
            {
                return Some(index);
            }
        }

        None
    }
}

// ============================================================
// Global process manager
// ============================================================

static PROCESS_MANAGER:
SimpleLock<ProcessManager> =
    SimpleLock::new(
        ProcessManager::new(),
    );

static PID_ALLOC:
AtomicU64 =
    AtomicU64::new(1);

static INITIALIZED:
AtomicBool =
    AtomicBool::new(false);

// ============================================================
// Scheduler result
// ============================================================

#[derive(
    Clone,
    Copy,
)]
pub enum ScheduleResult {
    Switched(
        SavedUserContext,
    ),

    NoProcess,
}

// ============================================================
// Initialization
// ============================================================

/// Initialize the process subsystem.
///
/// ProcessManager is already statically constructed,
/// so this function does not allocate or build a large
/// object on the kernel stack.
pub fn init() {
    INITIALIZED.store(
        true,
        Ordering::Release,
    );
}

// ============================================================
// Process creation
// ============================================================

pub fn create_process(
    address_space: UserAddressSpace,
    selectors: Selectors,
) -> u64 {
    if !INITIALIZED.load(
        Ordering::Acquire,
    ) {
        panic!(
            "Rusty: process manager not initialized"
        );
    }

    let mut guard =
        PROCESS_MANAGER.lock();

    let manager =
        &mut *guard;

    let slot_index =
        manager
            .find_free_slot()
            .expect(
                "Rusty: process table is full",
            );

    let pid =
        PID_ALLOC.fetch_add(
            1,
            Ordering::Relaxed,
        );

    assert_ne!(
        pid,
        0,
        "Rusty: PID allocator wrapped",
    );

    let process =
        Process::new(
            pid,
            &address_space,
            selectors,
        );

    // UserAddressSpace owns the mapper object,
    // but its actual page tables are allocated
    // through the kernel memory subsystem and are
    // intentionally left alive.
    drop(address_space);

    let slot =
        &mut manager.processes[
            slot_index
            ];

    slot.process.write(
        process,
    );

    slot.occupied = true;

    crate::serial::write_str(
        "process: created PID=",
    );

    crate::serial::write_usize(
        pid as usize,
    );

    crate::serial::write_str(
        "\n",
    );

    pid
}

// ============================================================
// Current process
// ============================================================

pub fn current_pid()
    -> Option<u64>
{
    let guard =
        PROCESS_MANAGER.lock();

    guard.current
}

// ============================================================
// Current process page table
// ============================================================

/// Return the root page table of the currently
/// running process.
///
/// This is used by syscall pointer validation so
/// userspace addresses are checked against the
/// correct address space.
pub fn current_level_4_frame()
    -> Option<PhysFrame<Size4KiB>>
{
    let guard =
        PROCESS_MANAGER.lock();

    let pid =
        guard.current?;

    Some(
        guard
            .find_process(pid)?
            .level_4_frame,
    )
}

// ============================================================
// Set current process
// ============================================================

pub fn set_current(
    pid: u64,
) -> bool {
    let mut guard =
        PROCESS_MANAGER.lock();

    let manager =
        &mut *guard;

    let valid =
        manager
            .find_process(pid)
            .map(|process| {
                process.state
                    != ProcessState::Exited
            })
            .unwrap_or(false);

    if !valid {
        return false;
    }

    if let Some(previous_pid) =
        manager.current
    {
        if previous_pid != pid {
            if let Some(previous) =
                manager.find_process_mut(
                    previous_pid,
                )
            {
                if previous.state
                    == ProcessState::Running
                {
                    previous.state =
                        ProcessState::Ready;
                }
            }
        }
    }

    if let Some(process) =
        manager.find_process_mut(pid)
    {
        process.state =
            ProcessState::Running;
    }

    manager.current =
        Some(pid);

    true
}

// ============================================================
// Enter current process
// ============================================================

pub unsafe fn enter_current(
    selectors: Selectors,
) -> ! {
    let level_4_frame;
    let context;

    {
        let guard =
            PROCESS_MANAGER.lock();

        let manager =
            &*guard;

        let pid =
            manager.current.expect(
                "Rusty: no current process",
            );

        let process =
            manager.find_process(pid)
                .expect(
                    "Rusty: current process missing",
                );

        if process.state
            != ProcessState::Running
        {
            panic!(
                "Rusty: current process is not Running"
            );
        }

        level_4_frame =
            process.level_4_frame;

        context =
            process.context;
    }

    // --------------------------------------------------------
    // Switch to the process page table.
    // --------------------------------------------------------

    unsafe {
        Cr3::write(
            level_4_frame,
            Cr3Flags::empty(),
        );
    }

    // --------------------------------------------------------
    // Enter Ring 3.
    // --------------------------------------------------------

    let frame =
        x86_64::structures::idt::InterruptStackFrameValue::new(
            VirtAddr::new(
                context.rip,
            ),
            selectors.user_code,
            x86_64::registers::rflags::RFlags::from_bits_retain(
                context.rflags,
            ),
            VirtAddr::new(
                context.rsp,
            ),
            selectors.user_data,
        );

    unsafe {
        frame.iretq();
    }
}

// ============================================================
// Cooperative scheduler
// ============================================================

pub fn yield_current(
    current_context: SavedUserContext,
) -> ScheduleResult {
    let mut guard =
        PROCESS_MANAGER.lock();

    let manager =
        &mut *guard;

    let current_pid =
        match manager.current {
            Some(pid) => pid,

            None =>
                return ScheduleResult::NoProcess,
        };

    // --------------------------------------------------------
    // Save current CPU context.
    // --------------------------------------------------------

    if let Some(current) =
        manager.find_process_mut(
            current_pid,
        )
    {
        current.context =
            current_context;

        current.state =
            ProcessState::Ready;
    }

    // --------------------------------------------------------
    // Find another Ready process.
    // --------------------------------------------------------

    let next_index =
        match manager.find_next_ready(
            current_pid,
        ) {
            Some(index) => index,

            None => {
                if let Some(current) =
                    manager.find_process_mut(
                        current_pid,
                    )
                {
                    current.state =
                        ProcessState::Running;
                }

                return ScheduleResult::NoProcess;
            }
        };

    // --------------------------------------------------------
    // Get next process information.
    // --------------------------------------------------------

    let next_pid;
    let next_context;
    let next_frame;

    {
        let slot =
            &mut manager.processes[
                next_index
                ];

        let next =
            unsafe {
                slot.process_mut()
            };

        next_pid =
            next.pid;

        next_context =
            next.context;

        next_frame =
            next.level_4_frame;

        next.state =
            ProcessState::Running;
    }

    manager.current =
        Some(next_pid);

    // --------------------------------------------------------
    // Switch address space.
    // --------------------------------------------------------

    unsafe {
        Cr3::write(
            next_frame,
            Cr3Flags::empty(),
        );
    }

    crate::serial::write_str(
        "scheduler: switched to PID=",
    );

    crate::serial::write_usize(
        next_pid as usize,
    );

    crate::serial::write_str(
        "\n",
    );

    ScheduleResult::Switched(
        next_context,
    )
}

// ============================================================
// Process exit
// ============================================================

pub fn exit_current(
    status: u64,
) -> ScheduleResult {
    let mut guard =
        PROCESS_MANAGER.lock();

    let manager =
        &mut *guard;

    let current_pid =
        match manager.current {
            Some(pid) => pid,

            None =>
                return ScheduleResult::NoProcess,
        };

    // --------------------------------------------------------
    // Mark process exited.
    // --------------------------------------------------------

    if let Some(current) =
        manager.find_process_mut(
            current_pid,
        )
    {
        current.state =
            ProcessState::Exited;
    }

    crate::serial::write_str(
        "process: PID=",
    );

    crate::serial::write_usize(
        current_pid as usize,
    );

    crate::serial::write_str(
        " exited with status=",
    );

    crate::serial::write_usize(
        status as usize,
    );

    crate::serial::write_str(
        "\n",
    );

    // --------------------------------------------------------
    // Find another Ready process.
    // --------------------------------------------------------

    let next_index =
        match manager.find_next_ready(
            current_pid,
        ) {
            Some(index) => index,

            None => {
                manager.current =
                    None;

                return ScheduleResult::NoProcess;
            }
        };

    // --------------------------------------------------------
    // Select next process.
    // --------------------------------------------------------

    let next_pid;
    let next_context;
    let next_frame;

    {
        let slot =
            &mut manager.processes[
                next_index
                ];

        let next =
            unsafe {
                slot.process_mut()
            };

        next_pid =
            next.pid;

        next_context =
            next.context;

        next_frame =
            next.level_4_frame;

        next.state =
            ProcessState::Running;
    }

    manager.current =
        Some(next_pid);

    // --------------------------------------------------------
    // Switch page table.
    // --------------------------------------------------------

    unsafe {
        Cr3::write(
            next_frame,
            Cr3Flags::empty(),
        );
    }

    ScheduleResult::Switched(
        next_context,
    )
}

// ============================================================
// Process information
// ============================================================

pub fn process_exists(
    pid: u64,
) -> bool {
    let guard =
        PROCESS_MANAGER.lock();

    guard
        .find_process(pid)
        .map(|process| {
            process.state
                != ProcessState::Exited
        })
        .unwrap_or(false)
}

pub fn state(
    pid: u64,
) -> Option<ProcessState> {
    let guard =
        PROCESS_MANAGER.lock();

    guard
        .find_process(pid)
        .map(|process| process.state)
}