// ============================================================
// xHCI register access
// ============================================================
//
// References:
//   - Intel eXtensible Host Controller Interface for Universal Serial
//     Bus (xHCI) Specification, revision 1.1, chapters 5 and 6.
//   - Linux drivers/usb/host/xhci.h (register offsets and bit names).
//
// All registers are memory mapped. Reads and writes are volatile and the
// controller uses system memory for DMA, so the MMIO range is mapped
// uncached by crate::memory::identity_map_mmio().
// ============================================================

use core::ptr::{read_volatile, write_volatile};

// ============================================================
// Capability registers (offset from MMIO base)
// ============================================================

pub const CAP_CAPLENGTH: usize = 0x00;
pub const CAP_HCIVERSION: usize = 0x02;
pub const CAP_HCSPARAMS1: usize = 0x04;
pub const CAP_HCSPARAMS2: usize = 0x08;
pub const CAP_HCSPARAMS3: usize = 0x0C;
pub const CAP_HCCPARAMS1: usize = 0x10;
pub const CAP_DBOFF: usize = 0x14;
pub const CAP_RTSOFF: usize = 0x18;
pub const CAP_HCCPARAMS2: usize = 0x1C;

// ============================================================
// Operational register offsets (relative to the operational base)
// ============================================================

pub const OP_USBCMD: usize = 0x00;
pub const OP_USBSTS: usize = 0x04;
pub const OP_PAGESIZE: usize = 0x08;
pub const OP_DNCTRL: usize = 0x14;
pub const OP_CRCR: usize = 0x18;
pub const OP_DCBAAP: usize = 0x30;
pub const OP_CONFIG: usize = 0x38;

/// Port register set starts at operational base + 0x400.
pub const OP_PORT_BASE: usize = 0x400;

/// PORTSC registers are 16 bytes apart.
pub const OP_PORT_STRIDE: usize = 0x10;

// ============================================================
// Runtime register offsets (relative to the runtime base)
// ============================================================
//
// Interrupter 0 starts at runtime base + 0x20. Only interrupter 0 is
// used: the driver polls the event ring instead of taking MSI/MSI-X.
// ============================================================

pub const RT_INTERRUPTER0: usize = 0x20;
pub const IR_IMAN: usize = 0x00;
pub const IR_IMOD: usize = 0x04;
pub const IR_ERSTSZ: usize = 0x08;
pub const IR_ERSTBA: usize = 0x10;
pub const IR_ERDP: usize = 0x18;

// ============================================================
// USBCMD bits
// ============================================================

pub const USBCMD_RUN: u32 = 1 << 0;
pub const USBCMD_HCRST: u32 = 1 << 1;
pub const USBCMD_INTE: u32 = 1 << 2;

// ============================================================
// USBSTS bits
// ============================================================

pub const USBSTS_HCH: u32 = 1 << 0;
pub const USBSTS_HSE: u32 = 1 << 2;
pub const USBSTS_EINT: u32 = 1 << 3;
pub const USBSTS_PCD: u32 = 1 << 4;
pub const USBSTS_CNR: u32 = 1 << 11;

// ============================================================
// CRCR bits
// ============================================================

pub const CRCR_RCS: u64 = 1 << 0;
pub const CRCR_CS: u64 = 1 << 1;
pub const CRCR_CA: u64 = 1 << 2;
pub const CRCR_CRR: u64 = 1 << 3;

// ============================================================
// PORTSC bits
// ============================================================

pub const PORTSC_CCS: u32 = 1 << 0;
pub const PORTSC_PED: u32 = 1 << 1;
pub const PORTSC_OCA: u32 = 1 << 3;
pub const PORTSC_PR: u32 = 1 << 4;
pub const PORTSC_PLS_MASK: u32 = 0xF << 5;
pub const PORTSC_PLS_SHIFT: u32 = 5;
pub const PORTSC_PP: u32 = 1 << 9;
pub const PORTSC_SPEED_MASK: u32 = 0xF << 10;
pub const PORTSC_SPEED_SHIFT: u32 = 10;
pub const PORTSC_PIC_MASK: u32 = 0x3 << 14;
pub const PORTSC_LWS: u32 = 1 << 16;
pub const PORTSC_CSC: u32 = 1 << 17;
pub const PORTSC_PEC: u32 = 1 << 18;
pub const PORTSC_WRC: u32 = 1 << 19;
pub const PORTSC_OCC: u32 = 1 << 20;
pub const PORTSC_PRC: u32 = 1 << 21;
pub const PORTSC_PLC: u32 = 1 << 22;
pub const PORTSC_CEC: u32 = 1 << 23;
pub const PORTSC_CAS: u32 = 1 << 24;
pub const PORTSC_WCE: u32 = 1 << 25;
pub const PORTSC_WDE: u32 = 1 << 26;
pub const PORTSC_WOE: u32 = 1 << 27;
pub const PORTSC_DR: u32 = 1 << 30;
pub const PORTSC_WPR: u32 = 1 << 31;

/// Change bits are write-one-to-clear.
pub const PORTSC_CHANGE_BITS: u32 = PORTSC_CSC
    | PORTSC_PEC
    | PORTSC_WRC
    | PORTSC_OCC
    | PORTSC_PRC
    | PORTSC_PLC
    | PORTSC_CEC;

/// Bits software may write without side effects (power, indicator, wake).
pub const PORTSC_RW_BITS: u32 = PORTSC_PP | PORTSC_PIC_MASK | PORTSC_WCE | PORTSC_WDE | PORTSC_WOE;

// ============================================================
// IMAN bits
// ============================================================

pub const IMAN_IP: u32 = 1 << 0;
pub const IMAN_IE: u32 = 1 << 1;

// ============================================================
// HCSPARAMS1 bits
// ============================================================

pub const HCS1_MAX_SLOTS_MASK: u32 = 0xFF;
pub const HCS1_MAX_INTRS_MASK: u32 = 0x7FF;
pub const HCS1_MAX_PORTS_MASK: u32 = 0xFF;
pub const HCS1_PPC: u32 = 1 << 4;

// ============================================================
// HCCPARAMS1 bits
// ============================================================

pub const HCC1_AC64: u32 = 1 << 0;
pub const HCC1_XECP_SHIFT: u32 = 16;
pub const HCC1_XECP_MASK: u32 = 0xFFFF;

// ============================================================
// Register bank
// ============================================================

#[derive(Clone, Copy)]
pub struct Registers {
    base: usize,
}

impl Registers {
    pub const fn new(base: usize) -> Self {
        Self {
            base,
        }
    }

    #[inline]
    fn read32(&self, offset: usize) -> u32 {
        unsafe { read_volatile((self.base + offset) as *const u32) }
    }

    #[inline]
    fn write32(&self, offset: usize, value: u32) {
        unsafe {
            write_volatile((self.base + offset) as *mut u32, value);
        }
    }

    #[inline]
    fn read64(&self, offset: usize) -> u64 {
        unsafe { read_volatile((self.base + offset) as *const u64) }
    }

    #[inline]
    fn write64(&self, offset: usize, value: u64) {
        unsafe {
            write_volatile((self.base + offset) as *mut u64, value);
        }
    }

    /// Read a 32-bit register at an absolute offset from the MMIO base.
    /// Used for the extended capability list.
    pub fn read32_at(&self, offset: usize) -> u32 {
        self.read32(offset)
    }

    /// Write a 32-bit register at an absolute offset from the MMIO base.
    pub fn write32_at(&self, offset: usize, value: u32) {
        self.write32(offset, value);
    }

    // --------------------------------------------------------
    // Absolute-address accessors
    //
    // The derived bases (operational, port, runtime, doorbell) already
    // include the MMIO base, so they must not be passed through the
    // base-relative helpers above.
    // --------------------------------------------------------

    #[inline]
    fn read_abs32(&self, address: usize) -> u32 {
        unsafe { read_volatile(address as *const u32) }
    }

    #[inline]
    fn write_abs32(&self, address: usize, value: u32) {
        unsafe {
            write_volatile(address as *mut u32, value);
        }
    }

    #[inline]
    fn read_abs64(&self, address: usize) -> u64 {
        unsafe { read_volatile(address as *const u64) }
    }

    #[inline]
    fn write_abs64(&self, address: usize, value: u64) {
        unsafe {
            write_volatile(address as *mut u64, value);
        }
    }

    // --------------------------------------------------------
    // Capability registers
    // --------------------------------------------------------

    pub fn caplength(&self) -> u8 {
        // Capability registers are accessed as 32-bit words: some
        // controllers (including QEMU's xHCI) do not implement 8/16-bit
        // MMIO accesses.
        (self.read32(CAP_CAPLENGTH) & 0xFF) as u8
    }

    /// MMIO base address, for diagnostics.
    pub fn base(&self) -> usize {
        self.base
    }

    pub fn hciversion(&self) -> u16 {
        (self.read32(CAP_CAPLENGTH) >> 16) as u16
    }

    pub fn hcsparams1(&self) -> u32 {
        self.read32(CAP_HCSPARAMS1)
    }

    pub fn hcsparams2(&self) -> u32 {
        self.read32(CAP_HCSPARAMS2)
    }

    pub fn hccparams1(&self) -> u32 {
        self.read32(CAP_HCCPARAMS1)
    }

    pub fn dboff(&self) -> u32 {
        self.read32(CAP_DBOFF)
    }

    pub fn rtsoff(&self) -> u32 {
        self.read32(CAP_RTSOFF)
    }

    pub fn max_slots(&self) -> u8 {
        (self.hcsparams1() & HCS1_MAX_SLOTS_MASK) as u8
    }

    pub fn max_ports(&self) -> u8 {
        ((self.hcsparams1() >> 24) & HCS1_MAX_PORTS_MASK) as u8
    }

    /// Number of scratchpad buffers the controller requires.
    ///
    /// HCSPARAMS2 bits 21-25 (SPB). When nonzero, software must provide a
    /// pointer array and one page-sized buffer per entry, and point
    /// DCBAA[0] at the array before starting the controller.
    pub fn max_scratchpad_buffers(&self) -> u8 {
        ((self.hcsparams2() >> 21) & 0x1F) as u8
    }

    /// HCSPARAMS2 bit 26: scratchpad state must be restored on resume.
    pub fn scratchpad_restore(&self) -> bool {
        self.hcsparams2() & (1 << 26) != 0
    }

    pub fn port_power_control(&self) -> bool {
        self.hcsparams1() & HCS1_PPC != 0
    }

    pub fn supports_64bit(&self) -> bool {
        self.hccparams1() & HCC1_AC64 != 0
    }

    // --------------------------------------------------------
    // Derived base addresses
    // --------------------------------------------------------

    pub fn operational_base(&self) -> usize {
        self.base + self.caplength() as usize
    }

    pub fn port_base(&self) -> usize {
        self.operational_base() + OP_PORT_BASE
    }

    pub fn doorbell_base(&self) -> usize {
        self.base + (self.dboff() as usize & !0x3)
    }

    pub fn runtime_base(&self) -> usize {
        self.base + (self.rtsoff() as usize & !0x1F)
    }

    pub fn interrupter_base(&self) -> usize {
        self.runtime_base() + RT_INTERRUPTER0
    }

    // --------------------------------------------------------
    // Operational registers
    // --------------------------------------------------------

    pub fn usbcmd(&self) -> u32 {
        self.read_abs32(self.operational_base() + OP_USBCMD)
    }

    pub fn set_usbcmd(&self, value: u32) {
        self.write_abs32(self.operational_base() + OP_USBCMD, value);
    }

    pub fn usbsts(&self) -> u32 {
        self.read_abs32(self.operational_base() + OP_USBSTS)
    }

    /// Clear write-one-to-clear status bits.
    pub fn clear_usbsts(&self, bits: u32) {
        self.write_abs32(self.operational_base() + OP_USBSTS, bits);
    }

    pub fn pagesize(&self) -> u32 {
        self.read_abs32(self.operational_base() + OP_PAGESIZE)
    }

    pub fn set_pagesize(&self, value: u32) {
        self.write_abs32(self.operational_base() + OP_PAGESIZE, value);
    }

    pub fn crcr(&self) -> u64 {
        self.read_abs64(self.operational_base() + OP_CRCR)
    }

    pub fn set_crcr(&self, value: u64) {
        self.write_abs64(self.operational_base() + OP_CRCR, value);
    }

    pub fn dcbaap(&self) -> u64 {
        self.read_abs64(self.operational_base() + OP_DCBAAP)
    }

    pub fn set_dcbaap(&self, value: u64) {
        self.write_abs64(self.operational_base() + OP_DCBAAP, value);
    }

    pub fn config(&self) -> u32 {
        self.read_abs32(self.operational_base() + OP_CONFIG)
    }

    pub fn set_config(&self, value: u32) {
        self.write_abs32(self.operational_base() + OP_CONFIG, value);
    }

    // --------------------------------------------------------
    // Port registers (port numbers are 1-based)
    // --------------------------------------------------------

    pub fn portsc(&self, port: u8) -> u32 {
        let offset = self.port_base() + OP_PORT_STRIDE * (port as usize - 1);
        self.read_abs32(offset)
    }

    pub fn set_portsc(&self, port: u8, value: u32) {
        let offset = self.port_base() + OP_PORT_STRIDE * (port as usize - 1);
        self.write_abs32(offset, value);
    }

    // --------------------------------------------------------
    // Interrupter 0 registers
    // --------------------------------------------------------

    pub fn iman(&self) -> u32 {
        self.read_abs32(self.interrupter_base() + IR_IMAN)
    }

    pub fn set_iman(&self, value: u32) {
        self.write_abs32(self.interrupter_base() + IR_IMAN, value);
    }

    pub fn erstsz(&self) -> u32 {
        self.read_abs32(self.interrupter_base() + IR_ERSTSZ)
    }

    pub fn set_erstsz(&self, value: u32) {
        self.write_abs32(self.interrupter_base() + IR_ERSTSZ, value);
    }

    pub fn erstba(&self) -> u64 {
        self.read_abs64(self.interrupter_base() + IR_ERSTBA)
    }

    pub fn set_erstba(&self, value: u64) {
        self.write_abs64(self.interrupter_base() + IR_ERSTBA, value);
    }

    pub fn erdp(&self) -> u64 {
        self.read_abs64(self.interrupter_base() + IR_ERDP)
    }

    pub fn set_erdp(&self, value: u64) {
        self.write_abs64(self.interrupter_base() + IR_ERDP, value);
    }

    // --------------------------------------------------------
    // Doorbells
    // --------------------------------------------------------

    /// Ring the host controller doorbell (target 0: command ring).
    pub fn ring_host_doorbell(&self) {
        self.write_abs32(self.doorbell_base(), 0);
    }

    /// Ring a device slot doorbell for the given endpoint target.
    pub fn ring_doorbell(&self, slot: u8, target: u32) {
        let offset = self.doorbell_base() + 4 * slot as usize;
        self.write_abs32(offset, target);
    }
}
