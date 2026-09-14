use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
};

use x86_64::{
    PrivilegeLevel,
    VirtAddr,
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
};

// ============================================================
// Kernel stack used when entering Ring 0 from Ring 3
// ============================================================

const KERNEL_STACK_SIZE: usize =
    4096 * 5;

#[repr(align(16))]
struct KernelStack(
    [u8; KERNEL_STACK_SIZE],
);

static mut KERNEL_STACK:
KernelStack =
    KernelStack(
        [0; KERNEL_STACK_SIZE],
    );

// ============================================================
// GDT storage
// ============================================================

struct GdtStorage {
    gdt:
        UnsafeCell<
            MaybeUninit<
                GlobalDescriptorTable<8>,
            >,
        >,

    tss:
        UnsafeCell<
            MaybeUninit<
                TaskStateSegment,
            >,
        >,
}

unsafe impl Sync
for GdtStorage
{
}

impl GdtStorage {
    const fn new() -> Self {
        Self {
            gdt:
            UnsafeCell::new(
                MaybeUninit::uninit(),
            ),

            tss:
            UnsafeCell::new(
                MaybeUninit::uninit(),
            ),
        }
    }
}

static GDT_STORAGE:
GdtStorage =
    GdtStorage::new();

// ============================================================
// Selectors
// ============================================================

#[derive(Clone, Copy)]
pub struct Selectors {
    pub kernel_code:
        SegmentSelector,

    pub kernel_data:
        SegmentSelector,

    pub user_code:
        SegmentSelector,

    pub user_data:
        SegmentSelector,

    pub tss:
        SegmentSelector,
}

// ============================================================
// Initialization
// ============================================================

pub fn init() -> Selectors {
    //
    // ---------------------------------------------------------
    // Create TSS
    // ---------------------------------------------------------
    //

    let mut tss =
        TaskStateSegment::new();

    let kernel_stack_start =
        core::ptr::addr_of!(
            KERNEL_STACK
        ) as usize;

    let kernel_stack_end =
        kernel_stack_start
            + KERNEL_STACK_SIZE;

    tss.privilege_stack_table[0] =
        VirtAddr::new(
            kernel_stack_end as u64,
        );

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

    //
    // ---------------------------------------------------------
    // Create GDT
    // ---------------------------------------------------------
    //

    let mut gdt =
        GlobalDescriptorTable::<8>::new();

    let kernel_code =
        gdt.append(
            Descriptor::kernel_code_segment(),
        );

    let kernel_data =
        gdt.append(
            Descriptor::kernel_data_segment(),
        );

    let user_code =
        gdt.append(
            Descriptor::user_code_segment(),
        );

    let user_data =
        gdt.append(
            Descriptor::user_data_segment(),
        );

    let tss_selector =
        gdt.append(
            Descriptor::tss_segment(
                tss_ref,
            ),
        );

    unsafe {
        (*GDT_STORAGE.gdt.get())
            .write(gdt);
    }

    //
    // GDT must remain alive forever because the CPU keeps
    // using it after lgdt.
    //
    let gdt_ref:
        &'static GlobalDescriptorTable<8> =
        unsafe {
            &*(
                (*GDT_STORAGE.gdt.get())
                    .as_ptr()
            )
        };

    gdt_ref.load();

    //
    // Reload kernel segment registers.
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

        load_tss(
            tss_selector,
        );
    }

    Selectors {
        kernel_code,
        kernel_data,
        user_code,
        user_data,
        tss: tss_selector,
    }
}