// ============================================================
// xHCI host controller
// ============================================================
//
// The controller is driven in polled mode: interrupts stay disabled and the
// event ring is consumed from normal kernel context. That matches how the
// rest of Rusty schedules work (cooperative, syscall driven) and avoids
// sharing transfer state with an interrupt handler.
//
// Reference: Intel xHCI specification 1.1, chapters 4-6, and the Linux
// xHCI driver (drivers/usb/host/xhci*.c) for sequence and timeouts.
// ============================================================

pub mod regs;
pub mod ring;
pub mod trb;

use alloc::vec::Vec;

use crate::delay;
use crate::serial;
use crate::usb::dma::DmaBuffer;
use crate::usb::pci::XhciPci;

use regs::*;
use ring::{EventRing, TrbRing};
use trb::*;

// ============================================================
// Timeouts (microseconds)
// ============================================================

const CONTROLLER_TIMEOUT_US: u64 = 1_000_000;
const PORT_RESET_TIMEOUT_US: u64 = 500_000;
const COMMAND_TIMEOUT_US: u64 = 500_000;
const TRANSFER_TIMEOUT_US: u64 = 1_000_000;
const POLL_INTERVAL_US: u64 = 50;

const MAX_SLOTS: u8 = 64;

// ============================================================
// Errors
// ============================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Memory,
    ControllerReset,
    ControllerNotReady,
    Timeout,
    PortNotConnected,
    PortReset,
    InvalidSpeed,
    NoSlot,
    Command(u8),
    Transfer(u8),
    Unsupported,
}

impl Error {
    pub fn name(self) -> &'static str {
        match self {
            Error::Memory => "out of DMA memory",
            Error::ControllerReset => "controller reset failed",
            Error::ControllerNotReady => "controller not ready",
            Error::Timeout => "timeout",
            Error::PortNotConnected => "port not connected",
            Error::PortReset => "port reset failed",
            Error::InvalidSpeed => "invalid port speed",
            Error::NoSlot => "no free device slot",
            Error::Command(_) => "command failed",
            Error::Transfer(_) => "transfer failed",
            Error::Unsupported => "unsupported device",
        }
    }
}

pub type Result<T> = core::result::Result<T, Error>;

// ============================================================
// Device location
// ============================================================

/// Where a device sits in the USB tree.
#[derive(Clone, Copy, Default)]
pub struct DeviceLocation {
    /// Physical root port (1-based PORTSC number) the tree is attached to.
    pub root_port: u8,
    /// Route string: four bits per hub tier, port number in each nibble.
    pub route: u32,
    /// Transaction translator info for a LS/FS device behind a HS hub.
    pub tt: Option<TtInfo>,
}

#[derive(Clone, Copy)]
pub struct TtInfo {
    pub hub_slot: u8,
    pub port: u8,
    pub multi: bool,
}

// ============================================================
// Per-slot state
// ============================================================

struct Slot {
    device_context: DmaBuffer,
    ep0: TrbRing,
    location: DeviceLocation,
    speed: u8,
    context_entries: u8,
}

// ============================================================
// Controller
// ============================================================

pub struct Xhci {
    regs: Registers,
    command: TrbRing,
    event: EventRing,
    dcbaa: DmaBuffer,
    input: DmaBuffer,
    slots: Vec<Option<Slot>>,
    pending: Vec<Trb>,
    /// Pointer array referenced by DCBAA[0] when the controller requires
    /// scratchpad buffers.
    scratchpad_array: Option<DmaBuffer>,
    scratchpad_buffers: Vec<DmaBuffer>,
    max_slots: u8,
    max_ports: u8,
    usb2_ports: (u8, u8),
    usb3_ports: (u8, u8),
    port_power_control: bool,
}

impl Xhci {
    // --------------------------------------------------------
    // Construction
    // --------------------------------------------------------

    pub fn new(pci: &XhciPci) -> Result<Self> {
        let regs = Registers::new(pci.mmio_base);

        let hciversion = regs.hciversion();
        let caplength = regs.caplength();

        if caplength < 0x20 {
            serial::write_str("USB: invalid xHCI capability length\n");
            return Err(Error::Unsupported);
        }

        serial::write_str("USB: xHCI version=");
        serial::write_hex(hciversion as u64);
        serial::write_str(" caplength=");
        serial::write_hex(caplength as u64);

        let max_slots = regs.max_slots().min(MAX_SLOTS);
        let max_ports = regs.max_ports();

        if max_slots == 0 || max_ports == 0 {
            serial::write_str("\nUSB: controller reports no slots or ports\n");
            return Err(Error::Unsupported);
        }

        let max_intrs = (regs.hcsparams1() >> 8) & HCS1_MAX_INTRS_MASK;

        serial::write_str(" slots=");
        serial::write_usize(max_slots as usize);
        serial::write_str(" ports=");
        serial::write_usize(max_ports as usize);
        serial::write_str(" intrs=");
        serial::write_usize(max_intrs as usize);
        serial::write_str(" 64bit=");
        serial::write_str(if regs.supports_64bit() { "yes" } else { "no" });
        serial::write_str("\n");

        let command = TrbRing::new().ok_or(Error::Memory)?;
        let event = EventRing::new().ok_or(Error::Memory)?;
        let dcbaa = DmaBuffer::new(256 * 8, 64).ok_or(Error::Memory)?;
        let input = DmaBuffer::new(4096, 64).ok_or(Error::Memory)?;

        // Scratchpad buffers (xHCI 4.20). HCSPARAMS2.SPB tells how many
        // page-sized buffers the controller expects; DCBAA[0] must point at
        // an array of their physical addresses before the controller runs.
        let scratchpad_count = regs.max_scratchpad_buffers();

        let mut scratchpad_buffers = Vec::new();
        let mut scratchpad_array = None;

        if scratchpad_count > 0 {
            for _ in 0..scratchpad_count {
                scratchpad_buffers.push(DmaBuffer::new(4096, 4096).ok_or(Error::Memory)?);
            }

            let array = DmaBuffer::new(scratchpad_count as usize * 8, 64)
                .ok_or(Error::Memory)?;

            for (index, buffer) in scratchpad_buffers.iter().enumerate() {
                array.write_u64(index * 8, buffer.physical());
            }

            scratchpad_array = Some(array);
        }

        serial::write_str("USB: scratchpad buffers=");
        serial::write_usize(scratchpad_count as usize);
        serial::write_str(" restore=");
        serial::write_str(if regs.scratchpad_restore() { "yes" } else { "no" });
        serial::write_str("\n");

        let mut slots = Vec::with_capacity(max_slots as usize + 1);

        for _ in 0..=max_slots {
            slots.push(None);
        }

        let mut controller = Self {
            regs,
            command,
            event,
            dcbaa,
            input,
            slots,
            pending: Vec::new(),
            scratchpad_array,
            scratchpad_buffers,
            max_slots,
            max_ports,
            usb2_ports: (0, 0),
            usb3_ports: (0, 0),
            port_power_control: regs.port_power_control(),
        };

        controller.initialize()?;

        Ok(controller)
    }

    fn initialize(&mut self) -> Result<()> {
        self.legacy_handoff();

        self.reset_controller()?;

        // Program the controller structures before setting RUN.
        self.regs.set_pagesize(1);

        self.regs.set_config(self.max_slots as u32);

        self.dcbaa.clear();

        if let Some(array) = &self.scratchpad_array {
            self.dcbaa.write_u64(0, array.physical());
        }

        self.regs.set_dcbaap(self.dcbaa.physical());

        self.regs
            .set_crcr(self.command.physical() | CRCR_RCS);

        self.regs.set_erstsz(self.event.segment_count());
        self.regs.set_erstba(self.event.erst_physical());
        self.regs.set_erdp(self.event.erdp());

        // Polled mode: no interrupter is enabled.
        self.regs.set_iman(0);

        self.regs.set_usbcmd(USBCMD_RUN);

        let started = wait_until(
            || {
                let status = self.regs.usbsts();
                status & USBSTS_HCH == 0 && status & USBSTS_CNR == 0
            },
            CONTROLLER_TIMEOUT_US,
        );

        if !started {
            serial::write_str("USB: controller did not start\n");
            return Err(Error::ControllerNotReady);
        }

        delay::delay_ms(10);

        // Clear any status left over from reset.
        let status = self.regs.usbsts();
        self.regs.clear_usbsts(status & (USBSTS_HSE | USBSTS_EINT | USBSTS_PCD));

        self.read_supported_protocols();

        serial::write_str("USB: root ports USB2=");
        serial::write_usize(self.usb2_ports.1 as usize);
        serial::write_str(" USB3=");
        serial::write_usize(self.usb3_ports.1 as usize);
        serial::write_str("\n");

        self.power_ports();

        // Consume the port change events generated by the reset.
        self.drain_events();

        Ok(())
    }

    // --------------------------------------------------------
    // Reset and start
    // --------------------------------------------------------

    fn reset_controller(&mut self) -> Result<()> {
        if !wait_until(|| self.regs.usbsts() & USBSTS_CNR == 0, CONTROLLER_TIMEOUT_US) {
            serial::write_str("USB: controller busy before reset\n");
            return Err(Error::ControllerNotReady);
        }

        let command = self.regs.usbcmd();

        if command & USBCMD_RUN != 0 {
            self.regs.set_usbcmd(command & !USBCMD_RUN);

            if !wait_until(
                || self.regs.usbsts() & USBSTS_HCH != 0,
                CONTROLLER_TIMEOUT_US,
            ) {
                serial::write_str("USB: controller did not halt\n");
                return Err(Error::ControllerReset);
            }
        }

        self.regs.set_usbcmd(USBCMD_HCRST);

        if !wait_until(
            || self.regs.usbcmd() & USBCMD_HCRST == 0,
            CONTROLLER_TIMEOUT_US,
        ) {
            serial::write_str("USB: HCRST did not complete\n");
            return Err(Error::ControllerReset);
        }

        if !wait_until(|| self.regs.usbsts() & USBSTS_CNR == 0, CONTROLLER_TIMEOUT_US) {
            serial::write_str("USB: controller not ready after reset\n");
            return Err(Error::ControllerNotReady);
        }

        serial::write_str("USB: controller reset complete\n");

        Ok(())
    }

    // --------------------------------------------------------
    // BIOS/OS legacy handoff
    // --------------------------------------------------------
    //
    // Firmware may still own the controller through the USB Legacy Support
    // extended capability. Claim OS ownership and wait for the BIOS semaphore
    // to clear before resetting the controller.
    // --------------------------------------------------------

    fn legacy_handoff(&self) {
        let mut xecp =
            ((self.regs.hccparams1() >> HCC1_XECP_SHIFT) & 0xFFFF) as usize;

        let mut guard = 0;

        while xecp != 0 && guard < 64 {
            let cap = self.regs.read32_at(xecp * 4);
            let id = (cap & 0xFF) as u8;
            let next = ((cap >> 8) & 0xFF) as usize;

            if id == 1 {
                let semaphore = self.regs.read32_at(xecp * 4 + 4);

                // BIOS Owned Semaphore (bit 16) / OS Owned Semaphore (bit 24).
                if semaphore & (1 << 16) != 0 {
                    serial::write_str("USB: taking ownership from firmware\n");

                    self.regs
                        .write32_at(xecp * 4 + 4, semaphore | (1 << 24));

                    let _ = wait_until(
                        || self.regs.read32_at(xecp * 4 + 4) & (1 << 16) == 0,
                        CONTROLLER_TIMEOUT_US,
                    );
                }

                break;
            }

            if next == 0 {
                break;
            }

            xecp += next;
            guard += 1;
        }
    }

    // --------------------------------------------------------
    // Supported Protocol extended capabilities
    // --------------------------------------------------------
    //
    // USB2 and USB3 root ports need different reset sequences, so the port
    // number ranges are taken from the capability list instead of guessed
    // from PORTSC.
    // --------------------------------------------------------

    fn read_supported_protocols(&mut self) {
        let mut xecp =
            ((self.regs.hccparams1() >> HCC1_XECP_SHIFT) & 0xFFFF) as usize;

        let mut guard = 0;

        while xecp != 0 && guard < 64 {
            let cap = self.regs.read32_at(xecp * 4);
            let id = (cap & 0xFF) as u8;
            let next = ((cap >> 8) & 0xFF) as usize;
            let major = ((cap >> 24) & 0xFF) as u8;

            if id == 2 {
                let ports = self.regs.read32_at(xecp * 4 + 8);
                let offset = (ports & 0xFF) as u8;
                let count = ((ports >> 8) & 0xFF) as u8;

                if major == 3 {
                    self.usb3_ports = (offset, count);
                } else if major == 2 {
                    self.usb2_ports = (offset, count);
                }
            }

            if next == 0 {
                break;
            }

            xecp += next;
            guard += 1;
        }

        // Fallback for firmware that omits the capability: assume every port
        // is USB2, which uses the portable port-reset sequence.
        if self.usb2_ports.1 == 0 && self.usb3_ports.1 == 0 {
            self.usb2_ports = (1, self.max_ports);
        }
    }

    fn power_ports(&self) {
        if !self.port_power_control {
            return;
        }

        for port in 1..=self.max_ports {
            let portsc = self.regs.portsc(port);

            if portsc & PORTSC_PP == 0 {
                self.regs.set_portsc(port, PORTSC_PP);
            }
        }

        delay::delay_ms(20);
    }

    // --------------------------------------------------------
    // Port helpers
    // --------------------------------------------------------

    pub fn max_ports(&self) -> u8 {
        self.max_ports
    }

    pub fn is_usb2_port(&self, port: u8) -> bool {
        let (offset, count) = self.usb2_ports;
        count != 0 && port >= offset && port < offset.saturating_add(count)
    }

    pub fn is_usb3_port(&self, port: u8) -> bool {
        let (offset, count) = self.usb3_ports;
        count != 0 && port >= offset && port < offset.saturating_add(count)
    }

    pub fn port_connected(&self, port: u8) -> bool {
        self.regs.portsc(port) & PORTSC_CCS != 0
    }

    pub fn port_speed(&self, port: u8) -> u8 {
        ((self.regs.portsc(port) & PORTSC_SPEED_MASK) >> PORTSC_SPEED_SHIFT) as u8
    }

    pub fn port_debug(&self, port: u8) -> u32 {
        self.regs.portsc(port)
    }

    // --------------------------------------------------------
    // Port reset
    // --------------------------------------------------------

    pub fn reset_port(&mut self, port: u8, usb3: bool) -> Result<u8> {
        let mut portsc = self.regs.portsc(port);

        if portsc & PORTSC_CCS == 0 {
            return Err(Error::PortNotConnected);
        }

        if self.port_power_control && portsc & PORTSC_PP == 0 {
            self.regs.set_portsc(port, PORTSC_PP);
            delay::delay_ms(20);
            portsc = self.regs.portsc(port);
        }

        let speed = if usb3 {
            self.reset_usb3_port(port)?
        } else {
            self.reset_usb2_port(port)?
        };

        if speed == 0 {
            return Err(Error::InvalidSpeed);
        }

        Ok(speed)
    }

    fn reset_usb2_port(&mut self, port: u8) -> Result<u8> {
        let portsc = self.regs.portsc(port);

        // Nothing to do if the port is already enabled.
        if portsc & PORTSC_PED != 0 {
            return Ok(((portsc & PORTSC_SPEED_MASK) >> PORTSC_SPEED_SHIFT) as u8);
        }

        // Preserve power/indicator bits and start the reset.
        self.regs
            .set_portsc(port, (portsc & PORTSC_RW_BITS) | PORTSC_PR);

        let completed = wait_until(
            || self.regs.portsc(port) & PORTSC_PRC != 0,
            PORT_RESET_TIMEOUT_US,
        );

        let current = self.regs.portsc(port);

        // Acknowledge change bits and keep the port powered.
        self.regs.set_portsc(
            port,
            (current & PORTSC_RW_BITS) | PORTSC_CHANGE_BITS,
        );

        if !completed {
            serial::write_str("USB: port reset timeout on port ");
            serial::write_usize(port as usize);
            serial::write_str(" PORTSC=");
            serial::write_hex(current as u64);
            serial::write_str("\n");
            return Err(Error::PortReset);
        }

        // USB2 reset recovery time (spec 7.1.7.5): 10 ms.
        delay::delay_ms(10);

        let current = self.regs.portsc(port);

        if current & PORTSC_CCS == 0 {
            return Err(Error::PortNotConnected);
        }

        if current & PORTSC_PED == 0 {
            serial::write_str("USB: port not enabled after reset, port ");
            serial::write_usize(port as usize);
            serial::write_str(" PORTSC=");
            serial::write_hex(current as u64);
            serial::write_str("\n");
            return Err(Error::PortReset);
        }

        Ok(((current & PORTSC_SPEED_MASK) >> PORTSC_SPEED_SHIFT) as u8)
    }

    fn reset_usb3_port(&mut self, port: u8) -> Result<u8> {
        let portsc = self.regs.portsc(port);
        let pls = (portsc & PORTSC_PLS_MASK) >> PORTSC_PLS_SHIFT;

        if portsc & PORTSC_PED != 0 && pls == 0 {
            return Ok(((portsc & PORTSC_SPEED_MASK) >> PORTSC_SPEED_SHIFT) as u8);
        }

        // Clear cold attach status before the warm reset.
        if portsc & PORTSC_CAS != 0 {
            self.regs.set_portsc(
                port,
                (portsc & PORTSC_RW_BITS) | PORTSC_CAS | PORTSC_CHANGE_BITS,
            );
        }

        let current = self.regs.portsc(port);

        self.regs
            .set_portsc(port, (current & PORTSC_RW_BITS) | PORTSC_WPR);

        let completed = wait_until(
            || {
                let value = self.regs.portsc(port);
                let pls = (value & PORTSC_PLS_MASK) >> PORTSC_PLS_SHIFT;
                value & PORTSC_WRC != 0 || (value & PORTSC_PED != 0 && pls == 0)
            },
            PORT_RESET_TIMEOUT_US,
        );

        let current = self.regs.portsc(port);

        self.regs.set_portsc(
            port,
            (current & PORTSC_RW_BITS) | PORTSC_CHANGE_BITS,
        );

        if !completed {
            serial::write_str("USB: USB3 warm reset timeout on port ");
            serial::write_usize(port as usize);
            serial::write_str(" PORTSC=");
            serial::write_hex(current as u64);
            serial::write_str("\n");
            return Err(Error::PortReset);
        }

        delay::delay_ms(10);

        let current = self.regs.portsc(port);

        if current & PORTSC_CCS == 0 || current & PORTSC_PED == 0 {
            return Err(Error::PortReset);
        }

        Ok(((current & PORTSC_SPEED_MASK) >> PORTSC_SPEED_SHIFT) as u8)
    }

    // --------------------------------------------------------
    // Commands
    // --------------------------------------------------------

    fn submit_command(&mut self, trb: Trb) -> Result<Trb> {
        let address = self.command.enqueue(trb);

        self.regs.ring_host_doorbell();

        self.wait_command(address)
    }

    fn wait_command(&mut self, address: u64) -> Result<Trb> {
        let deadline = delay::now_us().saturating_add(COMMAND_TIMEOUT_US);

        loop {
            while let Some(event) = self.pop_hardware_event() {
                match event.trb_type() {
                    TRB_COMMAND_COMPLETION if event.event_pointer() == address => {
                        let code = event.completion_code();

                        if code != CC_SUCCESS {
                            serial::write_str("USB: command completion code ");
                            serial::write_hex(code as u64);
                            serial::write_str(" (");
                            serial::write_str(completion_name(code));
                            serial::write_str(")\n");
                            return Err(Error::Command(code));
                        }

                        return Ok(event);
                    }

                    _ => self.defer(event),
                }
            }

            if delay::now_us() >= deadline {
                serial::write_str("USB: command timeout\n");
                return Err(Error::Timeout);
            }

            delay::delay_us(POLL_INTERVAL_US);
        }
    }

    pub fn enable_slot(&mut self) -> Result<u8> {
        let event = self.submit_command(Trb::enable_slot(true))?;
        let slot = event.slot_id();

        if slot == 0 || slot > self.max_slots {
            return Err(Error::NoSlot);
        }

        Ok(slot)
    }

    pub fn disable_slot(&mut self, slot: u8) {
        let _ = self.submit_command(Trb::disable_slot(slot, true));
        self.slots[slot as usize] = None;
    }

    // --------------------------------------------------------
    // Device addressing
    // --------------------------------------------------------

    /// Address a device.
    ///
    /// The first call for a slot uses `bsr = true`, which keeps the device at
    /// address 0 so its first descriptors can be read. The second call sets
    /// the final address and supplies the Hub bit and TT information learned
    /// from those descriptors.
    pub fn address_device(
        &mut self,
        slot: u8,
        location: DeviceLocation,
        speed: u8,
        max_packet: u16,
        hub: bool,
        bsr: bool,
    ) -> Result<()> {
        if self.slots[slot as usize].is_none() {
            let device_context = DmaBuffer::new(2048, 64).ok_or(Error::Memory)?;
            let ep0 = TrbRing::new().ok_or(Error::Memory)?;

            self.slots[slot as usize] = Some(Slot {
                device_context,
                ep0,
                location,
                speed,
                context_entries: 1,
            });
        }

        let (ep0_physical, device_context_physical) = {
            let entry = self.slots[slot as usize]
                .as_ref()
                .ok_or(Error::Memory)?;

            (entry.ep0.physical(), entry.device_context.physical())
        };

        // The second Address Device resets the controller's EP0 dequeue
        // pointer, so the software ring has to rewind with it.
        if !bsr {
            if let Some(entry) = self.slots[slot as usize].as_mut() {
                entry.ep0.reset();
            }
        }

        self.dcbaa
            .write_u64(slot as usize * 8, device_context_physical);

        self.input.clear();

        // Input Control Context: add slot + EP0.
        self.input.write_u32(0x00, 0);
        self.input.write_u32(0x04, 0b11);

        // Slot Context.
        let mut slot_dword0 = (location.route & 0xF_FFFF)
            | ((speed as u32) << 20)
            | (1 << 27);

        if let Some(tt) = location.tt {
            if tt.multi {
                slot_dword0 |= 1 << 24;
            }
        }

        if hub {
            slot_dword0 |= 1 << 25;
        }

        let slot_dword1 = (location.root_port as u32) << 16;

        let slot_dword2 = match location.tt {
            Some(tt) => (tt.hub_slot as u32) | ((tt.port as u32) << 8),
            None => 0,
        };

        self.input.write_u32(0x20, slot_dword0);
        self.input.write_u32(0x24, slot_dword1);
        self.input.write_u32(0x28, slot_dword2);
        self.input.write_u32(0x2C, 0);

        // EP0 Context.
        let max_packet = max_packet.clamp(8, 1024) as u32;

        self.input.write_u32(0x40, 0);
        self.input.write_u32(0x44, (3 << 1) | (4 << 3) | (max_packet << 16));
        self.input.write_u32(0x48, (ep0_physical as u32) | 1);
        self.input.write_u32(0x4C, (ep0_physical >> 32) as u32);
        self.input.write_u32(0x50, 8);

        let input_physical = self.input.physical();

        self.submit_command(Trb::address_device(input_physical, slot, true, bsr))?;

        if let Some(entry) = self.slots[slot as usize].as_mut() {
            entry.context_entries = 1;
            entry.location = location;
            entry.speed = speed;
        }

        if !bsr {
            serial::write_str("USB: slot ");
            serial::write_usize(slot as usize);
            serial::write_str(" addressed on port ");
            serial::write_usize(location.root_port as usize);
            serial::write_str(" route=");
            serial::write_hex(location.route as u64);
            serial::write_str(" speed=");
            serial::write_usize(speed as usize);
            serial::write_str("\n");
        }

        Ok(())
    }

    // --------------------------------------------------------
    // Control transfers
    // --------------------------------------------------------

    /// Perform a control transfer on the device's default pipe.
    ///
    /// `data` carries the optional data stage buffer and its length. The
    /// transfer direction is taken from bit 7 of bmRequestType.
    pub fn control(
        &mut self,
        slot: u8,
        setup: [u8; 8],
        data: Option<(&DmaBuffer, usize)>,
    ) -> Result<usize> {
        let direction_in = setup[0] & 0x80 != 0;

        let trt = match data {
            None => TRT_NO_DATA,
            Some(_) if direction_in => TRT_IN,
            Some(_) => TRT_OUT,
        };

        let (setup_address, data_address, status_address) = {
            let entry = self.slots[slot as usize]
                .as_mut()
                .ok_or(Error::PortNotConnected)?;

            let ep0 = &mut entry.ep0;

            let setup_address = ep0.enqueue(Trb::setup(&setup, trt, slot, true));

            let data_address = match data {
                Some((buffer, length)) => Some((
                    ep0.enqueue(Trb::data(
                        buffer.physical(),
                        length,
                        direction_in,
                        slot,
                        true,
                        true,
                    )),
                    length,
                )),
                None => None,
            };

            // The status stage runs opposite to the data stage; no-data
            // requests use an IN status stage.
            let status_direction_in = if data.is_some() { !direction_in } else { true };

            let status_address = ep0.enqueue(Trb::status(
                status_direction_in,
                slot,
                true,
                true,
            ));

            (setup_address, data_address, status_address)
        };

        let _ = setup_address;

        self.regs.ring_doorbell(slot, 1);

        let mut actual = 0usize;

        if let Some((address, length)) = data_address {
            let event = self.wait_transfer(slot, 1, address)?;
            actual = length.saturating_sub(event.residual() as usize);
        }

        let status = self.wait_transfer(slot, 1, status_address)?;

        if status.completion_code() != CC_SUCCESS && status.completion_code() != CC_SHORT_PACKET
        {
            return Err(Error::Transfer(status.completion_code()));
        }

        Ok(actual)
    }

    fn wait_transfer(&mut self, slot: u8, dci: u8, address: u64) -> Result<Trb> {
        let deadline = delay::now_us().saturating_add(TRANSFER_TIMEOUT_US);

        loop {
            while let Some(event) = self.pop_hardware_event() {
                if event.trb_type() == TRB_TRANSFER_EVENT
                    && event.slot_id() == slot
                    && event.endpoint_id() == dci
                    && event.event_pointer() == address
                {
                    let code = event.completion_code();

                    if code == CC_SUCCESS || code == CC_SHORT_PACKET {
                        return Ok(event);
                    }

                    serial::write_str("USB: transfer completion code ");
                    serial::write_hex(code as u64);
                    serial::write_str(" (");
                    serial::write_str(completion_name(code));
                    serial::write_str(")\n");

                    return Err(Error::Transfer(code));
                }

                self.defer(event);
            }

            if delay::now_us() >= deadline {
                return Err(Error::Timeout);
            }

            delay::delay_us(POLL_INTERVAL_US);
        }
    }

    // --------------------------------------------------------
    // Interrupt endpoints
    // --------------------------------------------------------

    pub fn configure_interrupt_in(
        &mut self,
        slot: u8,
        dci: u8,
        max_packet: u16,
        interval: u8,
        ring_physical: u64,
    ) -> Result<()> {
        let (previous_entries, device_context) = {
            let entry = self.slots[slot as usize]
                .as_ref()
                .ok_or(Error::PortNotConnected)?;

            (entry.context_entries, entry.device_context.physical())
        };

        if dci < 2 || dci > 31 {
            return Err(Error::Unsupported);
        }

        let context_entries = previous_entries.max(dci);

        self.input.clear();

        // The Slot Context must be supplied along with the endpoint. Copy the
        // controller's current Slot Context so fields it owns (the device
        // address in particular) are preserved, then update the number of
        // valid endpoint contexts.
        for index in 0..4 {
            let value = unsafe {
                core::ptr::read_volatile((device_context as usize + index * 4) as *const u32)
            };

            self.input.write_u32(0x20 + index * 4, value);
        }

        let slot_dword0 = self.input.read_u32(0x20) & !(0x1F << 27);
        self.input
            .write_u32(0x20, slot_dword0 | ((context_entries as u32) << 27));

        // Add Context flags: Slot Context plus the new endpoint.
        self.input.write_u32(0x00, 0);
        self.input
            .write_u32(0x04, 1 | (1 << dci));

        let offset = 0x40 + 32 * (dci as usize - 1);

        let max_packet = max_packet.min(1024) as u32;

        self.input.write_u32(offset, (interval as u32) << 16);
        self.input
            .write_u32(offset + 4, (3 << 1) | (7 << 3) | (max_packet << 16));
        self.input
            .write_u32(offset + 8, (ring_physical as u32) | 1);
        self.input
            .write_u32(offset + 12, (ring_physical >> 32) as u32);
        self.input
            .write_u32(offset + 16, 8 | (max_packet << 16));

        let input_physical = self.input.physical();

        self.submit_command(Trb::configure_endpoint(input_physical, slot, true))?;

        if let Some(entry) = self.slots[slot as usize].as_mut() {
            entry.context_entries = context_entries;
        }

        Ok(())
    }

    /// Publish one interrupt IN TRB and ring the endpoint doorbell.
    pub fn queue_interrupt_in(
        &mut self,
        slot: u8,
        dci: u8,
        ring: &mut TrbRing,
        buffer: &DmaBuffer,
        length: usize,
    ) -> Result<u64> {
        let trb = Trb::normal(buffer.physical(), length, true, slot, true, true);

        let address = ring.enqueue(trb);

        self.regs.ring_doorbell(slot, dci as u32);

        Ok(address)
    }

    // --------------------------------------------------------
    // Event handling
    // --------------------------------------------------------

    fn pop_hardware_event(&mut self) -> Option<Trb> {
        let event = self.event.pop()?;

        // Tell the controller that the entry is free (EHB is cleared by
        // writing the new dequeue pointer with bit 3 zero).
        self.regs.set_erdp(self.event.erdp());

        Some(event)
    }

    fn defer(&mut self, event: Trb) {
        if self.pending.len() >= 64 {
            self.pending.remove(0);
        }

        self.pending.push(event);
    }

    /// Pop the next event, preferring events deferred by a synchronous wait.
    pub fn pop_event(&mut self) -> Option<Trb> {
        if !self.pending.is_empty() {
            return Some(self.pending.remove(0));
        }

        self.pop_hardware_event()
    }

    pub fn drain_events(&mut self) {
        while self.pop_hardware_event().is_some() {}
        self.pending.clear();
    }

    // --------------------------------------------------------
    // Diagnostics
    // --------------------------------------------------------

    #[allow(dead_code)]
    pub fn debug_state(&self) {
        serial::write_str("USB: USBCMD=");
        serial::write_hex(self.regs.usbcmd() as u64);
        serial::write_str(" USBSTS=");
        serial::write_hex(self.regs.usbsts() as u64);
        serial::write_str(" CONFIG=");
        serial::write_hex(self.regs.config() as u64);
        serial::write_str(" CRCR=");
        serial::write_hex(self.regs.crcr());
        serial::write_str(" DCBAAP=");
        serial::write_hex(self.regs.dcbaap());
        serial::write_str("\n");
    }

    #[allow(dead_code)]
    pub fn dump_ports(&self) {
        for port in 1..=self.max_ports {
            serial::write_str("USB:   port ");
            serial::write_usize(port as usize);
            serial::write_str(" PORTSC=");
            serial::write_hex(self.regs.portsc(port) as u64);
            serial::write_str("\n");
        }
    }
}

// ============================================================
// Polling helper
// ============================================================

fn wait_until<F>(mut condition: F, timeout_us: u64) -> bool
where
    F: FnMut() -> bool,
{
    let deadline = delay::now_us().saturating_add(timeout_us);

    loop {
        if condition() {
            return true;
        }

        if delay::now_us() >= deadline {
            return false;
        }

        delay::delay_us(POLL_INTERVAL_US);
    }
}
