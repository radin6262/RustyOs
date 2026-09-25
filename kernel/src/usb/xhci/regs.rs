use core::ptr::{read_volatile, write_volatile};

/*
 * ==========================================================================
 * xHCI register definitions
 * ==========================================================================
 *
 * These offsets and masks correspond to the xHCI register layout used by
 * Linux's xhci.h / xhci.c.
 *
 * Capability registers:
 *
 *     0x00  HCIVERSION / CAPLENGTH
 *     0x04  HCSPARAMS1
 *     0x08  HCSPARAMS2
 *     0x0C  HCSPARAMS3
 *     0x10  HCCPARAMS1
 *     0x14  DBOFF
 *     0x18  RTSOFF
 *     0x1C  HCCPARAMS2
 *
 * Operational registers begin at:
 *
 *     base + CAPLENGTH
 *
 * ==========================================================================*/

/*
 * ==========================================================================
 * Capability register offsets
 * ==========================================================================
 */

const CAPLENGTH_OFFSET: usize = 0x00;

const HCS_PARAMS1_OFFSET: usize = 0x04;

const HCS_PARAMS2_OFFSET: usize = 0x08;

const HCS_PARAMS3_OFFSET: usize = 0x0C;

const HCC_PARAMS1_OFFSET: usize = 0x10;

const DBOFF_OFFSET: usize = 0x14;

const RTSOFF_OFFSET: usize = 0x18;

const HCC_PARAMS2_OFFSET: usize = 0x1C;

/*
 * ==========================================================================
 * Capability masks
 * ==========================================================================
 */

/*
 * HC_CAPBASE:
 *
 * bits 7:0   = capability register length
 * bits 15:8  = reserved
 * bits 31:16 = HCI version
 */

const CAPLENGTH_MASK: u32 = 0xFF;

const HCI_VERSION_SHIFT: u32 = 16;

/*
 * HCSPARAMS1:
 *
 * bits 7:0   = max device slots
 * bits 18:8  = max interrupters
 * bits 31:24 = max ports
 */

const HCS_SLOTS_MASK: u32 = 0x0000_00FF;

const HCS_INTR_MASK: u32 = 0x0007_FF00;

const HCS_INTR_SHIFT: u32 = 8;

const HCS_PORTS_MASK: u32 = 0xFF00_0000;

const HCS_PORTS_SHIFT: u32 = 24;

/*
 * HCCPARAMS1:
 *
 * bit 0 = 64-bit addressing
 * bit 2 = 64-byte contexts
 * bits 31:16 = xHCI extended capability pointer
 */

const HCC_64BIT_ADDR: u32 = 1 << 0;

const HCC_64BYTE_CONTEXT: u32 = 1 << 2;

const HCC_EXT_CAPS_MASK: u32 = 0xFFFF_0000;

const HCC_EXT_CAPS_SHIFT: u32 = 16;

/*
 * DBOFF:
 *
 * bits 1:0 reserved.
 */

const DBOFF_MASK: u32 = 0xFFFF_FFFC;

/*
 * RTSOFF:
 *
 * bits 4:0 reserved.
 */

const RTSOFF_MASK: u32 = 0xFFFF_FFE0;

/*
 * ==========================================================================
 * Operational register offsets
 * ==========================================================================
 */

const OP_USBCMD: usize = 0x00;

const OP_USBSTS: usize = 0x04;

const OP_PAGESIZE: usize = 0x08;

const OP_DNCTRL: usize = 0x14;

const OP_CRCR: usize = 0x18;

const OP_DCBAAP: usize = 0x30;

const OP_CONFIG: usize = 0x38;

const OP_PORTS: usize = 0x400;

/*
 * ==========================================================================
 * Runtime register layout
 * ==========================================================================
 *
 * Runtime base:
 *
 *     +0x00 = MFINDEX
 *     +0x04..0x1F = reserved
 *     +0x20 = Interrupter 0
 *
 * Each interrupter register set is 0x20 bytes:
 *
 *     +0x00 IMAN
 *     +0x04 IMOD
 *     +0x08 ERSTSZ
 *     +0x0C reserved
 *     +0x10 ERSTBA
 *     +0x18 ERDP
 *
 * This matches struct xhci_run_regs / xhci_intr_reg in Linux.
 * ==========================================================================*/

const RUNTIME_INTERRUPTER_BASE: usize = 0x20;

const INTERRUPTER_STRIDE: usize = 0x20;

const IR_IMAN: usize = 0x00;

const IR_IMOD: usize = 0x04;

const IR_ERSTSZ: usize = 0x08;

const IR_ERSTBA: usize = 0x10;

const IR_ERDP: usize = 0x18;

/*
 * ==========================================================================
 * USBCMD
 * ==========================================================================
 */

const USBCMD_RUN: u32 = 1 << 0;

const USBCMD_RESET: u32 = 1 << 1;

const USBCMD_EIE: u32 = 1 << 2;

const USBCMD_HSEIE: u32 = 1 << 3;

/*
 * ==========================================================================
 * USBSTS
 * ==========================================================================
 */

const USBSTS_HCH: u32 = 1 << 0;

const USBSTS_HSE: u32 = 1 << 2;

const USBSTS_EINT: u32 = 1 << 3;

const USBSTS_PCD: u32 = 1 << 4;

const USBSTS_CNR: u32 = 1 << 11;

const USBSTS_HCE: u32 = 1 << 12;

/*
 * ==========================================================================
 * CRCR
 * ==========================================================================
 */

const CRCR_CYCLE: u64 = 1 << 0;

const CRCR_PAUSE: u64 = 1 << 1;

const CRCR_ABORT: u64 = 1 << 2;

const CRCR_RUNNING: u64 = 1 << 3;

const CRCR_POINTER_MASK: u64 = 0xFFFF_FFFF_FFFF_FFC0;

/*
 * ==========================================================================
 * CONFIG
 * ==========================================================================
 */

const CONFIG_MAX_SLOTS_MASK: u32 = 0xFF;

/*
 * ==========================================================================
 * IMAN
 * ==========================================================================
 */

const IMAN_IP: u32 = 1 << 0;

const IMAN_IE: u32 = 1 << 1;

/*
 * ==========================================================================
 * ERSTSZ
 * ==========================================================================
 */

const ERSTSZ_MASK: u32 = 0x0000_FFFF;

/*
 * ==========================================================================
 * ERSTBA
 * ==========================================================================
 *
 * bits 63:6 are the ERST base address.
 * ==========================================================================*/

const ERSTBA_MASK: u64 = 0xFFFF_FFFF_FFFF_FFC0;

/*
 * ==========================================================================
 * ERDP
 * ==========================================================================
 *
 * bits 2:0 = DESI
 * bit 3    = EHB
 * bits 63:4 = Event Ring Dequeue Pointer
 * ==========================================================================*/

const ERDP_DESI_MASK: u64 = 0x7;

const ERDP_EHB: u64 = 1 << 3;

const ERDP_POINTER_MASK: u64 = 0xFFFF_FFFF_FFFF_FFF0;

/*
 * ==========================================================================
 * Port layout
 * ==========================================================================
 */

const PORT_STRIDE: usize = 0x10;

/*
 * ==========================================================================
 * Limits
 * ==========================================================================
 */

const MAX_XHCI_INTERRUPTERS: usize = 1024;

const MAX_XHCI_PORTS: usize = 255;

/*
 * ==========================================================================
 * xHCI register block
 * ==========================================================================
 */

pub struct XhciRegs {
    /*
     * Capability-register base.
     *
     * This is also the base used by Linux for DBOFF.
     */
    pub base: usize,

    /*
     * Cached capability values.
     */
    pub cap_length: u8,
    pub hci_version: u16,

    pub hcsparams1: u32,
    pub hcsparams2: u32,
    pub hcsparams3: u32,

    pub hccparams1: u32,
    pub hccparams2: u32,

    /*
     * Byte offsets from capability-register base.
     */
    pub doorbell_offset: usize,
    pub runtime_offset: usize,

    /*
     * Decoded hardware limits.
     */
    pub max_slots: u8,
    pub max_interrupters: usize,
    pub max_ports: usize,

    /*
     * Context size required by this controller.
     */
    pub context_size: usize,
}

impl XhciRegs {
    /*
     * ======================================================================
     * Constructor
     * ======================================================================
     */

    pub unsafe fn new(base: usize) -> Self {
        if base == 0 {
            panic!("xHCI: register base is null",);
        }

        let hc_capbase = read32(
            base.checked_add(CAPLENGTH_OFFSET)
                .expect("xHCI: capability address overflow"),
        );

        let cap_length = (hc_capbase & CAPLENGTH_MASK) as u8;

        let hci_version = (hc_capbase >> HCI_VERSION_SHIFT) as u16;

        /*
         * CAPLENGTH must be at least large enough to reach the standard
         * capability register set.
         */
        if cap_length < 0x20 {
            panic!("xHCI: invalid CAPLENGTH",);
        }

        let hcsparams1 = read32(
            base.checked_add(HCS_PARAMS1_OFFSET)
                .expect("xHCI: HCSPARAMS1 address overflow"),
        );

        let hcsparams2 = read32(
            base.checked_add(HCS_PARAMS2_OFFSET)
                .expect("xHCI: HCSPARAMS2 address overflow"),
        );

        let hcsparams3 = read32(
            base.checked_add(HCS_PARAMS3_OFFSET)
                .expect("xHCI: HCSPARAMS3 address overflow"),
        );

        let hccparams1 = read32(
            base.checked_add(HCC_PARAMS1_OFFSET)
                .expect("xHCI: HCCPARAMS1 address overflow"),
        );

        let doorbell_offset = (read32(
            base.checked_add(DBOFF_OFFSET)
                .expect("xHCI: DBOFF address overflow"),
        ) & DBOFF_MASK) as usize;

        let runtime_offset = (read32(
            base.checked_add(RTSOFF_OFFSET)
                .expect("xHCI: RTSOFF address overflow"),
        ) & RTSOFF_MASK) as usize;

        let hccparams2 = read32(
            base.checked_add(HCC_PARAMS2_OFFSET)
                .expect("xHCI: HCCPARAMS2 address overflow"),
        );

        let max_slots = (hcsparams1 & HCS_SLOTS_MASK) as u8;

        let max_interrupters = ((hcsparams1 & HCS_INTR_MASK) >> HCS_INTR_SHIFT) as usize;

        let max_ports = ((hcsparams1 & HCS_PORTS_MASK) >> HCS_PORTS_SHIFT) as usize;

        if max_slots == 0 {
            panic!("xHCI: controller reports zero device slots",);
        }

        if max_interrupters == 0 || max_interrupters > MAX_XHCI_INTERRUPTERS {
            panic!("xHCI: invalid number of interrupters",);
        }

        if max_ports == 0 || max_ports > MAX_XHCI_PORTS {
            panic!("xHCI: invalid number of ports",);
        }

        /*
         * HCCPARAMS1.CSZ:
         *
         *     0 = 32-byte contexts
         *     1 = 64-byte contexts
         */
        let context_size = if (hccparams1 & HCC_64BYTE_CONTEXT) != 0 {
            64
        } else {
            32
        };

        Self {
            base,

            cap_length,
            hci_version,

            hcsparams1,
            hcsparams2,
            hcsparams3,

            hccparams1,
            hccparams2,

            doorbell_offset,
            runtime_offset,

            max_slots,
            max_interrupters,
            max_ports,

            context_size,
        }
    }

    /*
     * ======================================================================
     * Capability accessors
     * ======================================================================
     */

    #[inline(always)]
    pub const fn max_slots(&self) -> usize {
        self.max_slots as usize
    }

    #[inline(always)]
    pub const fn max_interrupters(&self) -> usize {
        self.max_interrupters
    }

    #[inline(always)]
    pub const fn max_ports(&self) -> usize {
        self.max_ports
    }

    #[inline(always)]
    pub const fn context_size(&self) -> usize {
        self.context_size
    }

    #[inline(always)]
    pub const fn hcsparams2(&self) -> u32 {
        self.hcsparams2
    }

    #[inline(always)]
    pub const fn hci_version(&self) -> u16 {
        self.hci_version
    }

    #[inline(always)]
    pub const fn supports_64bit(&self) -> bool {
        (self.hccparams1 & HCC_64BIT_ADDR) != 0
    }

    /*
     * ======================================================================
     * Page size
     * ======================================================================
     */

    #[inline(always)]
    pub fn pagesize(&self) -> u32 {
        /*
         * PAGESIZE is a 16-bit capability mask. Bits 31:16 are reserved.
         */
        unsafe { read32(self.operational_base() + OP_PAGESIZE) & 0x0000_FFFF }
    }

    /*
     * ======================================================================
     * Register bases
     * ======================================================================
     */

    #[inline(always)]
    pub fn operational_base(&self) -> usize {
        self.base + self.cap_length as usize
    }

    #[inline(always)]
    pub fn doorbell_base(&self) -> usize {
        self.base + self.doorbell_offset
    }

    #[inline(always)]
    pub fn runtime_base(&self) -> usize {
        self.base + self.runtime_offset
    }

    #[inline(always)]
    pub fn interrupter_base(&self, interrupter: usize) -> usize {
        if interrupter >= self.max_interrupters {
            panic!("xHCI: invalid interrupter index",);
        }

        self.runtime_base()
            .checked_add(RUNTIME_INTERRUPTER_BASE)
            .and_then(|base| {
                base.checked_add(
                    interrupter
                        .checked_mul(INTERRUPTER_STRIDE)
                        .expect("xHCI: interrupter offset overflow"),
                )
            })
            .expect("xHCI: interrupter register address overflow")
    }

    #[inline(always)]
    pub fn port_base(&self, port: usize) -> usize {
        if port == 0 || port > self.max_ports {
            panic!("xHCI: invalid port number",);
        }

        self.operational_base()
            .checked_add(OP_PORTS)
            .and_then(|base| {
                base.checked_add(
                    (port - 1)
                        .checked_mul(PORT_STRIDE)
                        .expect("xHCI: port offset overflow"),
                )
            })
            .expect("xHCI: port register address overflow")
    }

    /*
     * ======================================================================
     * USBCMD
     * ======================================================================
     */

    #[inline(always)]
    pub fn usbcmd(&self) -> u32 {
        unsafe { read32(self.operational_base() + OP_USBCMD) }
    }

    #[inline(always)]
    pub fn set_usbcmd(&self, value: u32) {
        unsafe {
            write32(self.operational_base() + OP_USBCMD, value);
        }
    }

    #[inline(always)]
    pub fn set_running(&self, running: bool) {
        let mut value = self.usbcmd();

        if running {
            value |= USBCMD_RUN;
        } else {
            value &= !USBCMD_RUN;
        }

        self.set_usbcmd(value);
    }

    #[inline(always)]
    pub fn enable_interrupts(&self) {
        let mut value = self.usbcmd();

        value |= USBCMD_EIE;

        self.set_usbcmd(value);
    }

    #[inline(always)]
    pub fn disable_interrupts(&self) {
        let mut value = self.usbcmd();

        value &= !USBCMD_EIE;

        self.set_usbcmd(value);
    }

    /*
     * ======================================================================
     * USBSTS
     * ======================================================================
     */

    #[inline(always)]
    pub fn usbsts(&self) -> u32 {
        unsafe { read32(self.operational_base() + OP_USBSTS) }
    }

    #[inline(always)]
    pub fn set_usbsts(&self, value: u32) {
        unsafe {
            write32(self.operational_base() + OP_USBSTS, value);
        }
    }

    #[inline(always)]
    pub fn halted(&self) -> bool {
        (self.usbsts() & USBSTS_HCH) != 0
    }

    #[inline(always)]
    pub fn controller_not_ready(&self) -> bool {
        (self.usbsts() & USBSTS_CNR) != 0
    }

    #[inline(always)]
    pub fn host_system_error(&self) -> bool {
        (self.usbsts() & USBSTS_HSE) != 0
    }

    #[inline(always)]
    pub fn host_controller_error(&self) -> bool {
        (self.usbsts() & USBSTS_HCE) != 0
    }

    #[inline(always)]
    pub fn event_interrupt(&self) -> bool {
        (self.usbsts() & USBSTS_EINT) != 0
    }

    #[inline(always)]
    pub fn clear_event_interrupt(&self) {
        unsafe {
            write32(self.operational_base() + OP_USBSTS, USBSTS_EINT);
        }
    }

    /*
     * ======================================================================
     * CONFIG
     * ======================================================================
     */

    #[inline(always)]
    pub fn config(&self) -> u32 {
        unsafe { read32(self.operational_base() + OP_CONFIG) }
    }

    #[inline(always)]
    pub fn set_config(&self, value: u32) {
        unsafe {
            write32(self.operational_base() + OP_CONFIG, value);
        }
    }

    /*
     * Linux's xhci_enable_max_dev_slots():
     *
     *     config &= ~HCS_SLOTS_MASK;
     *     config |= max_slots;
     *
     * Preserve all other bits.
     */

    #[inline(always)]
    pub fn set_max_slots(&self, slots: usize) {
        if slots == 0 || slots > 0xFF {
            panic!("xHCI: invalid maximum slot count",);
        }

        let mut value = self.config();

        value &= !CONFIG_MAX_SLOTS_MASK;

        value |= (slots as u32) & CONFIG_MAX_SLOTS_MASK;

        self.set_config(value);
    }

    /*
     * ======================================================================
     * Command Ring Control Register
     * ======================================================================
     */

    #[inline(always)]
    pub fn crcr(&self) -> u64 {
        unsafe { read64(self.operational_base() + OP_CRCR) }
    }

    #[inline(always)]
    pub fn set_crcr(&self, value: u64) {
        unsafe {
            write64(self.operational_base() + OP_CRCR, value);
        }
    }

    /*
     * ======================================================================
     * DCBAAP
     * ======================================================================
     */

    #[inline(always)]
    pub fn dcbaap(&self) -> u64 {
        unsafe { read64(self.operational_base() + OP_DCBAAP) }
    }

    #[inline(always)]
    pub fn set_dcbaap(&self, value: u64) {
        if (value & 0x3F) != 0 {
            panic!("xHCI: DCBAAP is not 64-byte aligned",);
        }

        unsafe {
            write64(self.operational_base() + OP_DCBAAP, value);
        }
    }

    /*
     * ======================================================================
     * Doorbells
     * ======================================================================
     */

    #[inline(always)]
    pub fn ring_doorbell(&self, slot_id: u8, target: u8) {
        if slot_id as usize > self.max_slots as usize {
            panic!("xHCI: invalid doorbell slot",);
        }

        let address = self
            .doorbell_base()
            .checked_add(
                (slot_id as usize)
                    .checked_mul(4)
                    .expect("xHCI: doorbell offset overflow"),
            )
            .expect("xHCI: doorbell address overflow");

        unsafe {
            write32(address, target as u32);

            /*
             * Match Linux xhci-ring.c: flush the PCIe posted doorbell write
             * with an MMIO readback before the caller starts waiting for a
             * transfer event.
             */
            let _ = read32(address);
        }
    }

    /*
     * Doorbell 0 is the command ring.
     *
     * Linux's DB_VALUE_HOST is 0.
     */

    #[inline(always)]
    pub fn ring_command(&self) {
        let address = self.doorbell_base();

        unsafe {
            write32(address, 0);

            /* Flush the PCIe posted command-ring doorbell write. */
            let _ = read32(address);
        }
    }

    /*
     * ======================================================================
     * IMAN
     * ======================================================================
     */

    #[inline(always)]
    pub fn iman(&self, interrupter: usize) -> u32 {
        unsafe { read32(self.interrupter_base(interrupter) + IR_IMAN) }
    }

    #[inline(always)]
    pub fn set_iman(&self, interrupter: usize, value: u32) {
        unsafe {
            write32(self.interrupter_base(interrupter) + IR_IMAN, value);
        }
    }

    /*
     * Exactly the RMW sequence used by Linux xhci_enable_interrupter():
     *
     *     iman &= ~IMAN_IP;
     *     iman |= IMAN_IE;
     */

    #[inline(always)]
    pub fn enable_interrupter(&self, interrupter: usize) {
        let mut iman = self.iman(interrupter);

        iman &= !IMAN_IP;

        iman |= IMAN_IE;

        self.set_iman(interrupter, iman);

        /*
         * Flush posted MMIO write.
         */
        let _ = self.iman(interrupter);
    }

    /*
     * Exactly the Linux disable-interrupter RMW behavior.
     */

    #[inline(always)]
    pub fn disable_interrupter(&self, interrupter: usize) {
        let mut iman = self.iman(interrupter);

        /*
         * Writing zero to RW1C IP leaves it unchanged.
         */
        iman &= !IMAN_IP;

        iman &= !IMAN_IE;

        self.set_iman(interrupter, iman);

        let _ = self.iman(interrupter);
    }

    /*
     * ======================================================================
     * IMOD
     * ======================================================================
     */

    #[inline(always)]
    pub fn imod(&self, interrupter: usize) -> u32 {
        unsafe { read32(self.interrupter_base(interrupter) + IR_IMOD) }
    }

    #[inline(always)]
    pub fn set_imod(&self, interrupter: usize, value: u32) {
        unsafe {
            write32(self.interrupter_base(interrupter) + IR_IMOD, value);
        }
    }

    /*
     * ======================================================================
     * ERSTSZ
     * ======================================================================
     *
     * Linux preserves the upper reserved bits and replaces only the low
     * 16-bit segment-count field.
     */

    #[inline(always)]
    pub fn erstsz(&self, interrupter: usize) -> u32 {
        unsafe { read32(self.interrupter_base(interrupter) + IR_ERSTSZ) }
    }

    #[inline(always)]
    pub fn set_erstsz(&self, interrupter: usize, segments: u32) {
        if segments == 0 || segments > 0xFFFF {
            panic!("xHCI: invalid ERST segment count",);
        }

        let address = self.interrupter_base(interrupter) + IR_ERSTSZ;

        unsafe {
            let mut value = read32(address);

            value &= !ERSTSZ_MASK;

            value |= segments & ERSTSZ_MASK;

            write32(address, value);
        }
    }

    /*
     * ======================================================================
     * ERSTBA
     * ======================================================================
     *
     * Linux preserves the reserved low bits and changes only bits 63:6.
     */

    #[inline(always)]
    pub fn erstba(&self, interrupter: usize) -> u64 {
        unsafe { read64(self.interrupter_base(interrupter) + IR_ERSTBA) }
    }

    #[inline(always)]
    pub fn set_erstba(&self, interrupter: usize, value: u64) {
        if (value & 0x3F) != 0 {
            panic!("xHCI: ERSTBA is not 64-byte aligned",);
        }

        let address = self.interrupter_base(interrupter) + IR_ERSTBA;

        unsafe {
            let mut current = read64(address);

            current &= !ERSTBA_MASK;

            current |= value & ERSTBA_MASK;

            write64(address, current);
        }
    }

    /*
     * ======================================================================
     * ERDP
     * ======================================================================
     *
     * This implements Linux's current xhci_update_erst_dequeue() semantics.
     *
     * `clear_ehb == false`:
     *
     *     update pointer
     *     preserve EHB
     *
     * `clear_ehb == true`:
     *
     *     update pointer
     *     write EHB=1 to clear Event Handler Busy
     *
     * DESI is zero here because this driver currently has one segment.
     */

    #[inline(always)]
    pub fn erdp(&self, interrupter: usize) -> u64 {
        unsafe { read64(self.interrupter_base(interrupter) + IR_ERDP) }
    }

    #[inline(always)]
    pub fn set_erdp(&self, interrupter: usize, address: u64, clear_ehb: bool) {
        if (address & 0x0F) != 0 {
            panic!("xHCI: ERDP address is not 16-byte aligned",);
        }

        let mut value = (address & ERDP_POINTER_MASK) | (0 & ERDP_DESI_MASK);

        if clear_ehb {
            value |= ERDP_EHB;
        }

        unsafe {
            write64(self.interrupter_base(interrupter) + IR_ERDP, value);
        }
    }

    /*
     * Compatibility with your existing xhci.rs.
     *
     * This explicitly clears EHB, matching Linux's final event-ring
     * dequeue update after processing events.
     */

    #[inline(always)]
    pub fn set_erdp_clear_busy(&self, interrupter: usize, address: u64) {
        self.set_erdp(interrupter, address, true);
    }

    /*
     * Use this during initial interrupter setup if you want the exact
     * Linux behavior of preserving EHB.
     */

    #[inline(always)]
    pub fn set_erdp_preserve_busy(&self, interrupter: usize, address: u64) {
        self.set_erdp(interrupter, address, false);
    }

    /*
     * ======================================================================
     * PORTSC
     * ======================================================================
     */

    #[inline(always)]
    pub fn portsc(&self, port: usize) -> u32 {
        unsafe { read32(self.port_base(port)) }
    }

    #[inline(always)]
    pub fn set_portsc(&self, port: usize, value: u32) {
        unsafe {
            write32(self.port_base(port), value);
        }
    }

    #[inline(always)]
    pub fn port_connected(&self, port: usize) -> bool {
        (self.portsc(port) & (1 << 0)) != 0
    }

    #[inline(always)]
    pub fn port_enabled(&self, port: usize) -> bool {
        (self.portsc(port) & (1 << 1)) != 0
    }

    #[inline(always)]
    pub fn port_reset(&self, port: usize) -> bool {
        (self.portsc(port) & (1 << 4)) != 0
    }

    #[inline(always)]
    pub fn port_link_state(&self, port: usize) -> u8 {
        ((self.portsc(port) >> 5) & 0x0F) as u8
    }

    #[inline(always)]
    pub fn port_speed(&self, port: usize) -> u8 {
        ((self.portsc(port) >> 10) & 0x0F) as u8
    }

    /*
     * ======================================================================
     * Extended capabilities
     * ======================================================================
     */

    #[inline(always)]
    pub fn extended_capability_pointer(&self) -> usize {
        ((self.hccparams1 & HCC_EXT_CAPS_MASK) >> HCC_EXT_CAPS_SHIFT) as usize * 4
    }

    #[inline(always)]
    pub fn extended_capability(&self, offset: usize) -> u32 {
        unsafe {
            read32(
                self.base
                    .checked_add(offset)
                    .expect("xHCI: extended capability address overflow"),
            )
        }
    }

    pub fn extended_capabilities(&self) -> ExtendedCapabilityIterator<'_> {
        ExtendedCapabilityIterator {
            regs: self,
            offset: self.extended_capability_pointer(),
        }
    }

    pub fn walk_extended_capabilities<F>(&self, mut callback: F)
    where
        F: FnMut(usize, u8, usize, u32),
    {
        let mut offset = self.extended_capability_pointer();

        /*
         * An xECP of zero means there is no extended capability list.
         */
        while offset != 0 {
            let header = self.extended_capability(offset);

            let capability_id = (header & 0xFF) as u8;

            let next = ((header >> 8) & 0xFF) as usize;

            callback(offset, capability_id, next, header);

            if next == 0 {
                break;
            }

            let next_offset = match offset.checked_add(
                next.checked_mul(4)
                    .expect("xHCI: extended capability offset overflow"),
            ) {
                Some(value) => value,

                None => break,
            };

            if next_offset <= offset {
                break;
            }

            offset = next_offset;
        }
    }

    /*
     * ======================================================================
     * Raw access
     * ======================================================================
     */

    #[inline(always)]
    pub unsafe fn read32(&self, offset: usize) -> u32 {
        read32(
            self.base
                .checked_add(offset)
                .expect("xHCI: register address overflow"),
        )
    }

    #[inline(always)]
    pub unsafe fn write32(&self, offset: usize, value: u32) {
        write32(
            self.base
                .checked_add(offset)
                .expect("xHCI: register address overflow"),
            value,
        );
    }

    #[inline(always)]
    pub unsafe fn read64(&self, offset: usize) -> u64 {
        read64(
            self.base
                .checked_add(offset)
                .expect("xHCI: register address overflow"),
        )
    }

    #[inline(always)]
    pub unsafe fn write64(&self, offset: usize, value: u64) {
        write64(
            self.base
                .checked_add(offset)
                .expect("xHCI: register address overflow"),
            value,
        );
    }
}

/*
 * ==========================================================================
 * Extended capability iterator
 * ==========================================================================
 */

pub struct ExtendedCapabilityIterator<'a> {
    regs: &'a XhciRegs,
    offset: usize,
}

impl<'a> Iterator for ExtendedCapabilityIterator<'a> {
    type Item = (usize, u8, u32);

    fn next(&mut self) -> Option<Self::Item> {
        let offset = self.offset;

        if offset == 0 {
            return None;
        }

        let header = self.regs.extended_capability(offset);

        let capability_id = (header & 0xFF) as u8;

        let next = ((header >> 8) & 0xFF) as usize;

        let next_offset = if next == 0 {
            0
        } else {
            offset.checked_add(next.checked_mul(4)?).unwrap_or(0)
        };

        if next_offset != 0 && next_offset <= offset {
            self.offset = 0;
        } else {
            self.offset = next_offset;
        }

        Some((offset, capability_id, header))
    }
}

/*
 * ==========================================================================
 * MMIO helpers
 * ==========================================================================
 */

#[inline(always)]
unsafe fn read32(address: usize) -> u32 {
    read_volatile(address as *const u32)
}

#[inline(always)]
unsafe fn read64(address: usize) -> u64 {
    /*
     * xHCI 64-bit pointer registers are accessed as two DWORDs,
     * low DWORD first and high DWORD second.
     */
    let low = read32(address) as u64;
    let high = read32(
        address
            .checked_add(4)
            .expect("xHCI: 64-bit register address overflow"),
    ) as u64;

    low | (high << 32)
}

#[inline(always)]
unsafe fn write32(address: usize, value: u32) {
    write_volatile(address as *mut u32, value);
}

#[inline(always)]
unsafe fn write64(address: usize, value: u64) {
    /*
     * xHCI 64-bit pointer registers are written low DWORD first,
     * then high DWORD.
     */
    write32(address, value as u32);
    write32(
        address
            .checked_add(4)
            .expect("xHCI: 64-bit register address overflow"),
        (value >> 32) as u32,
    );
}
