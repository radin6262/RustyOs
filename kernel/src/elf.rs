use x86_64::{
    VirtAddr,
    structures::paging::{
        Page,
        PageTableFlags,
        Size4KiB,
        Translate,
    },
};

use crate::memory;
use crate::user::address_space::UserAddressSpace;

const ELF_MAGIC: [u8; 4] =
    [0x7F, b'E', b'L', b'F'];

const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const EM_X86_64: u16 = 0x3E;

const PT_LOAD: u32 = 1;

const PF_X: u32 = 1;
const PF_W: u32 = 2;

const PAGE_SIZE: u64 = 4096;

#[derive(Debug)]
pub enum ElfError {
    ParseError(&'static str),
    InvalidFormat(&'static str),
    AllocationFailed,
}

pub struct LoadedElf {
    pub entry_point: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Header {
    magic: [u8; 4],
    class: u8,
    data: u8,
    version: u8,
    os_abi: u8,
    abi_version: u8,
    pad: [u8; 7],

    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64ProgramHeader {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

pub fn load_elf(
    elf_data: &[u8],
    address_space: &mut UserAddressSpace,
) -> Result<LoadedElf, ElfError> {
    crate::serial::write_str(
        "ELF: manual parser entered\n",
    );

    // ========================================================
    // ELF header
    // ========================================================

    if elf_data.len()
        < core::mem::size_of::<Elf64Header>()
    {
        return Err(
            ElfError::InvalidFormat(
                "ELF file too small",
            )
        );
    }

    let header: Elf64Header =
        unsafe {
            core::ptr::read_unaligned(
                elf_data.as_ptr()
                    as *const Elf64Header,
            )
        };

    crate::serial::write_str(
        "ELF: magic=",
    );

    for byte in header.magic {
        crate::serial::write_hex(
            byte as u64,
        );

        crate::serial::write_str(
            " ",
        );
    }

    crate::serial::write_str(
        "\n",
    );

    if header.magic != ELF_MAGIC {
        return Err(
            ElfError::InvalidFormat(
                "Invalid ELF magic",
            )
        );
    }

    if header.class != ELFCLASS64 {
        return Err(
            ElfError::InvalidFormat(
                "Expected ELF64",
            )
        );
    }

    if header.data != ELFDATA2LSB {
        return Err(
            ElfError::InvalidFormat(
                "Expected little-endian ELF",
            )
        );
    }

    if header.version != 1 {
        return Err(
            ElfError::InvalidFormat(
                "Invalid ELF version",
            )
        );
    }

    if header.e_machine != EM_X86_64 {
        return Err(
            ElfError::InvalidFormat(
                "Expected x86_64 ELF",
            )
        );
    }

    if header.e_phentsize as usize
        != core::mem::size_of::<Elf64ProgramHeader>()
    {
        return Err(
            ElfError::InvalidFormat(
                "Unexpected program header size",
            )
        );
    }

    // ========================================================
    // Program header table
    // ========================================================

    let phoff =
        usize::try_from(
            header.e_phoff,
        )
            .map_err(|_| {
                ElfError::InvalidFormat(
                    "Program header offset overflow",
                )
            })?;

    let phentsize =
        core::mem::size_of::<Elf64ProgramHeader>();

    let phnum =
        header.e_phnum as usize;

    let ph_table_size =
        phentsize
            .checked_mul(phnum)
            .ok_or(
                ElfError::InvalidFormat(
                    "Program header table overflow",
                )
            )?;

    let ph_end =
        phoff
            .checked_add(
                ph_table_size,
            )
            .ok_or(
                ElfError::InvalidFormat(
                    "Program header table range overflow",
                )
            )?;

    if ph_end > elf_data.len() {
        return Err(
            ElfError::InvalidFormat(
                "Program header table outside ELF",
            )
        );
    }

    crate::serial::write_str(
        "ELF: entry=",
    );

    crate::serial::write_hex(
        header.e_entry,
    );

    crate::serial::write_str(
        "\n",
    );

    // ========================================================
    // Userspace range
    // ========================================================

    crate::serial::write_str(
        "ELF: USER_MIN=",
    );

    crate::serial::write_hex(
        memory::USER_MIN,
    );

    crate::serial::write_str(
        " USER_MAX=",
    );

    crate::serial::write_hex(
        memory::USER_MAX,
    );

    crate::serial::write_str(
        "\n",
    );

    // ========================================================
    // Load PT_LOAD segments
    // ========================================================

    let mut found_load =
        false;

    let mut entry_executable =
        false;

    for index in 0..phnum {
        let offset =
            phoff
                + index * phentsize;

        let ph: Elf64ProgramHeader =
            unsafe {
                core::ptr::read_unaligned(
                    elf_data
                        .as_ptr()
                        .add(offset)
                        as *const Elf64ProgramHeader,
                )
            };

        if ph.p_type != PT_LOAD {
            continue;
        }

        found_load = true;

        crate::serial::write_str(
            "ELF: PT_LOAD vaddr=",
        );

        crate::serial::write_hex(
            ph.p_vaddr,
        );

        crate::serial::write_str(
            " mem=",
        );

        crate::serial::write_usize(
            ph.p_memsz as usize,
        );

        crate::serial::write_str(
            " file=",
        );

        crate::serial::write_usize(
            ph.p_filesz as usize,
        );

        crate::serial::write_str(
            "\n",
        );

        // ----------------------------------------------------
        // Validate sizes
        // ----------------------------------------------------

        if ph.p_memsz < ph.p_filesz {
            return Err(
                ElfError::InvalidFormat(
                    "PT_LOAD memsz < filesz",
                )
            );
        }

        // ----------------------------------------------------
        // Validate file range
        // ----------------------------------------------------

        let file_offset =
            usize::try_from(
                ph.p_offset,
            )
                .map_err(|_| {
                    ElfError::InvalidFormat(
                        "PT_LOAD file offset overflow",
                    )
                })?;

        let file_size =
            usize::try_from(
                ph.p_filesz,
            )
                .map_err(|_| {
                    ElfError::InvalidFormat(
                        "PT_LOAD file size overflow",
                    )
                })?;

        let file_end =
            file_offset
                .checked_add(
                    file_size,
                )
                .ok_or(
                    ElfError::InvalidFormat(
                        "PT_LOAD file range overflow",
                    )
                )?;

        if file_end > elf_data.len() {
            return Err(
                ElfError::InvalidFormat(
                    "PT_LOAD outside ELF file",
                )
            );
        }

        // ----------------------------------------------------
        // Validate virtual range
        // ----------------------------------------------------

        let virtual_end =
            ph.p_vaddr
                .checked_add(
                    ph.p_memsz,
                )
                .ok_or(
                    ElfError::InvalidFormat(
                        "PT_LOAD virtual range overflow",
                    )
                )?;

        crate::serial::write_str(
            "ELF: virtual range ",
        );

        crate::serial::write_hex(
            ph.p_vaddr,
        );

        crate::serial::write_str(
            " -> ",
        );

        crate::serial::write_hex(
            virtual_end,
        );

        crate::serial::write_str(
            "\n",
        );

        if ph.p_memsz != 0 {
            if ph.p_vaddr < memory::USER_MIN {
                return Err(
                    ElfError::InvalidFormat(
                        "PT_LOAD starts below userspace",
                    )
                );
            }

            if virtual_end > memory::USER_MAX {
                return Err(
                    ElfError::InvalidFormat(
                        "PT_LOAD ends above userspace",
                    )
                );
            }
        }

        // ----------------------------------------------------
        // Entry point
        // ----------------------------------------------------

        if header.e_entry >= ph.p_vaddr
            && header.e_entry < virtual_end
            && (ph.p_flags & PF_X) != 0
        {
            entry_executable = true;
        }

        // ----------------------------------------------------
        // Load segment
        // ----------------------------------------------------

        let segment_data =
            &elf_data[
                file_offset..file_end
                ];

        load_segment(
            address_space,
            ph.p_vaddr,
            ph.p_memsz,
            segment_data,
            (ph.p_flags & PF_W) != 0,
            (ph.p_flags & PF_X) != 0,
        )?;
    }

    // ========================================================
    // Final validation
    // ========================================================

    if !found_load {
        return Err(
            ElfError::InvalidFormat(
                "ELF has no PT_LOAD segments",
            )
        );
    }

    if !entry_executable {
        return Err(
            ElfError::InvalidFormat(
                "ELF entry is not executable",
            )
        );
    }

    crate::serial::write_str(
        "ELF: loading complete\n",
    );

    Ok(
        LoadedElf {
            entry_point: header.e_entry,
        }
    )
}

// ============================================================
// Load one PT_LOAD segment
// ============================================================

fn load_segment(
    address_space: &mut UserAddressSpace,
    virt_addr: u64,
    mem_size: u64,
    data: &[u8],
    writable: bool,
    executable: bool,
) -> Result<(), ElfError> {
    if mem_size == 0 {
        return Ok(());
    }

    let segment_end =
        virt_addr
            .checked_add(
                mem_size,
            )
            .ok_or(
                ElfError::InvalidFormat(
                    "Segment address overflow",
                )
            )?;

    // --------------------------------------------------------
    // Page-aligned range
    // --------------------------------------------------------

    let start_addr =
        virt_addr
            & !(PAGE_SIZE - 1);

    let end_addr =
        segment_end
            .checked_add(
                PAGE_SIZE - 1,
            )
            .ok_or(
                ElfError::InvalidFormat(
                    "Segment alignment overflow",
                )
            )?
            & !(PAGE_SIZE - 1);

    if start_addr < memory::USER_MIN {
        return Err(
            ElfError::InvalidFormat(
                "Segment starts below userspace",
            )
        );
    }

    if end_addr > memory::USER_MAX {
        return Err(
            ElfError::InvalidFormat(
                "Segment ends above userspace",
            )
        );
    }

    crate::serial::write_str(
        "ELF: mapping pages ",
    );

    crate::serial::write_hex(
        start_addr,
    );

    crate::serial::write_str(
        " -> ",
    );

    crate::serial::write_hex(
        end_addr,
    );

    crate::serial::write_str(
        "\n",
    );

    // --------------------------------------------------------
    // File data range
    // --------------------------------------------------------

    let data_end =
        virt_addr
            .checked_add(
                data.len() as u64,
            )
            .ok_or(
                ElfError::InvalidFormat(
                    "Segment data overflow",
                )
            )?;

    // --------------------------------------------------------
    // Process pages
    // --------------------------------------------------------

    let mut current_addr =
        start_addr;

    while current_addr < end_addr {
        crate::serial::write_str(
            "ELF: processing page VA=",
        );

        crate::serial::write_hex(
            current_addr,
        );

        crate::serial::write_str(
            "\n",
        );

        let page =
            Page::<Size4KiB>::containing_address(
                VirtAddr::new(
                    current_addr,
                ),
            );

        // ----------------------------------------------------
        // Check whether this page is already mapped.
        //
        // Multiple PT_LOAD segments can share a page.
        // Reuse the existing physical frame instead of
        // trying to map the same virtual page twice.
        // ----------------------------------------------------

        let existing_frame =
            address_space
                .mapper_mut()
                .translate_addr(
                    VirtAddr::new(
                        current_addr,
                    ),
                )
                .map(|physical_address| {
                    x86_64::structures::paging::PhysFrame::<
                        x86_64::structures::paging::Size4KiB
                    >::containing_address(
                        physical_address,
                    )
                });

        let frame =
            match existing_frame {
                Some(frame) => {
                    crate::serial::write_str(
                        "ELF: reusing existing page\n",
                    );

                    frame
                }

                None => {
                    crate::serial::write_str(
                        "ELF: allocating new page\n",
                    );

                    let frame =
                        memory::allocate_user_frame()
                            .ok_or(
                                ElfError::AllocationFailed,
                            )?;

                    memory::zero_frame(
                        frame,
                    );

                    unsafe {
                        memory::map_user_page(
                            address_space.mapper_mut(),
                            VirtAddr::new(
                                current_addr,
                            ),
                            frame,
                            writable,
                            executable,
                        );
                    }

                    crate::serial::write_str(
                        "ELF: page mapped\n",
                    );

                    frame
                }
            };

        // ----------------------------------------------------
        // Copy file-backed bytes into the page.
        // ----------------------------------------------------

        let page_end =
            current_addr
                .checked_add(
                    PAGE_SIZE,
                )
                .ok_or(
                    ElfError::InvalidFormat(
                        "Page address overflow",
                    )
                )?;

        let copy_start =
            core::cmp::max(
                virt_addr,
                current_addr,
            );

        let copy_end =
            core::cmp::min(
                data_end,
                page_end,
            );

        if copy_start < copy_end {
            let source_offset =
                (copy_start
                    - virt_addr)
                    as usize;

            let destination_offset =
                (copy_start
                    - current_addr)
                    as usize;

            let copy_len =
                (copy_end
                    - copy_start)
                    as usize;

            crate::serial::write_str(
                "ELF: copying ",
            );

            crate::serial::write_usize(
                copy_len,
            );

            crate::serial::write_str(
                " bytes\n",
            );

            let frame_virtual =
                memory::physical_to_virtual(
                    frame
                        .start_address()
                        .as_u64(),
                );

            unsafe {
                core::ptr::copy_nonoverlapping(
                    data
                        .as_ptr()
                        .add(source_offset),

                    frame_virtual
                        .add(destination_offset),

                    copy_len,
                );
            }
        }

        current_addr +=
            PAGE_SIZE;
    }

    Ok(())
}