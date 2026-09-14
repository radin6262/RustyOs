use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
};

use x86_64::{
    instructions::{
        segmentation::{
            CS,
            DS,
            ES,
            SS,
            Segment,
        },
        tables::load_tss,
    },
    registers::segmentation::SegmentSelector,
    structures::{
        gdt::{
            Descriptor,
            GlobalDescriptorTable,
        },
        tss::TaskStateSegment,
    },
    PrivilegeLevel,
    VirtAddr,
};

// ============================================================
// Kernel stack used when the CPU transitions Ring 3 -> Ring 0
// ============================================================

const KERNEL_STACK_SIZE: usize = 16 * 1024;

#[repr(align(16))]
struct KernelStack(
    [u8; KERNEL_STACK_SIZE],
);

static mut KERNEL_STACK: KernelStack =
    KernelStack([0; KERNEL_STACK_SIZE]);

// ============================================================
// Persistent GDT / TSS storage
// ============================================================

struct GdtStorage {
    gdt: UnsafeCell<
        MaybeUninit<
            GlobalDescriptorTable<8>,
        >,
    >,

    tss: UnsafeCell<
        MaybeUninit<
            TaskStateSegment,
        >,
    >,
}

unsafe impl Sync for GdtStorage {}

impl GdtStorage {
    const fn new() -> Self {
        Self {
            gdt: UnsafeCell::new(
                MaybeUninit::uninit(),
            ),
            tss: UnsafeCell::new(
                MaybeUninit::uninit(),
            ),
        }
    }
}

static GDT_STORAGE: GdtStorage =
    GdtStorage::new();

// ============================================================
// Segment selectors
// ============================================================

#[derive(Clone, Copy)]
pub struct Selectors {
    pub kernel_code: SegmentSelector,
    pub kernel_data: SegmentSelector,
    pub user_code: SegmentSelector,
    pub user_data: SegmentSelector,
    pub tss: SegmentSelector,
}

// ============================================================
// Force RPL 3 onto a selector
// ============================================================

fn make_ring3_selector(
    selector: SegmentSelector,
) -> SegmentSelector {
    SegmentSelector::new(
        selector.index(),
        PrivilegeLevel::Ring3,
    )
}

// ============================================================
// Initialize GDT + TSS
// ============================================================

pub fn init() -> Selectors {
    // ========================================================
    // TSS
    // ========================================================

    let mut tss =
        TaskStateSegment::new();

    // Rust 2024 requires the access to static mut to be unsafe.
    let kernel_stack_start =
        unsafe {
            core::ptr::addr_of!(
                KERNEL_STACK
            ) as usize
        };

    let kernel_stack_top =
        kernel_stack_start
            .checked_add(
                KERNEL_STACK_SIZE,
            )
            .expect(
                "Rusty: kernel stack address overflow",
            );

    // --------------------------------------------------------
    // Ring 3 -> Ring 0 stack
    // --------------------------------------------------------
    //
    // When an interrupt/syscall/trap enters Ring 0 from Ring 3,
    // the CPU will use this stack for CPL 0.
    //

    tss.privilege_stack_table[0] =
        VirtAddr::new(
            kernel_stack_top as u64,
        );

    // Store the TSS permanently.
    unsafe {
        (*GDT_STORAGE.tss.get())
            .write(tss);
    }

    let tss_ref:
        &'static TaskStateSegment =
        unsafe {
            &*(
                (*GDT_STORAGE.tss.get())
                    .as_ptr()
            )
        };

    // ========================================================
    // GDT
    // ========================================================

    let mut gdt =
        GlobalDescriptorTable::<8>::new();

    // --------------------------------------------------------
    // Kernel code
    // --------------------------------------------------------

    let kernel_code =
        gdt.append(
            Descriptor::kernel_code_segment(),
        );

    // --------------------------------------------------------
    // Kernel data
    // --------------------------------------------------------

    let kernel_data =
        gdt.append(
            Descriptor::kernel_data_segment(),
        );

    // --------------------------------------------------------
    // User code
    // --------------------------------------------------------

    let user_code_raw =
        gdt.append(
            Descriptor::user_code_segment(),
        );

    // --------------------------------------------------------
    // User data
    // --------------------------------------------------------

    let user_data_raw =
        gdt.append(
            Descriptor::user_data_segment(),
        );

    // --------------------------------------------------------
    // TSS
    // --------------------------------------------------------

    let tss_selector =
        gdt.append(
            Descriptor::tss_segment(
                tss_ref,
            ),
        );

    // --------------------------------------------------------
    // Explicitly make user selectors RPL 3.
    // --------------------------------------------------------

    let user_code =
        make_ring3_selector(
            user_code_raw,
        );

    let user_data =
        make_ring3_selector(
            user_data_raw,
        );

    // ========================================================
    // Store GDT
    // ========================================================

    unsafe {
        (*GDT_STORAGE.gdt.get())
            .write(gdt);
    }

    let gdt_ref:
        &'static GlobalDescriptorTable<8> =
        unsafe {
            &*(
                (*GDT_STORAGE.gdt.get())
                    .as_ptr()
            )
        };

    // ========================================================
    // Load GDT
    // ========================================================

    unsafe {
        gdt_ref.load();
    }

    // ========================================================
    // Reload kernel segments
    // ========================================================
    //
    // The GDT has changed, so the segment registers must use
    // selectors from the new GDT.
    //

    unsafe {
        CS::set_reg(
            kernel_code,
        );

        SS::set_reg(
            kernel_data,
        );

        DS::set_reg(
            kernel_data,
        );

        ES::set_reg(
            kernel_data,
        );
    }

    // ========================================================
    // Load TSS
    // ========================================================

    unsafe {
        load_tss(
            tss_selector,
        );
    }

    // ========================================================
    // Sanity checks
    // ========================================================

    assert_eq!(
        user_code.rpl(),
        PrivilegeLevel::Ring3,
        "Rusty: user code selector is not Ring 3",
    );

    assert_eq!(
        user_data.rpl(),
        PrivilegeLevel::Ring3,
        "Rusty: user data selector is not Ring 3",
    );

    assert!(
        kernel_stack_top != 0,
        "Rusty: invalid kernel stack",
    );

    // ========================================================
    // Return selectors
    // ========================================================

    Selectors {
        kernel_code,
        kernel_data,
        user_code,
        user_data,
        tss: tss_selector,
    }
}