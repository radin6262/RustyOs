// ============================================================
// xHCI Transfer Request Blocks
// ============================================================
//
// A TRB is 16 bytes: a 64-bit parameter, a 32-bit status and a 32-bit
// control word. Rings are arrays of TRBs that the controller consumes in
// order; software and hardware each keep a cycle state so stale entries are
// never executed after a ring wrap.
//
// Reference: xHCI 1.1 section 6.4 (TRB definitions).
// ============================================================

// ============================================================
// TRB types
// ============================================================

pub const TRB_NORMAL: u8 = 1;
pub const TRB_SETUP_STAGE: u8 = 2;
pub const TRB_DATA_STAGE: u8 = 3;
pub const TRB_STATUS_STAGE: u8 = 4;
pub const TRB_LINK: u8 = 6;
pub const TRB_NOOP: u8 = 8;
pub const TRB_ENABLE_SLOT: u8 = 9;
pub const TRB_DISABLE_SLOT: u8 = 10;
pub const TRB_ADDRESS_DEVICE: u8 = 11;
pub const TRB_CONFIGURE_ENDPOINT: u8 = 12;
pub const TRB_EVALUATE_CONTEXT: u8 = 13;
pub const TRB_RESET_ENDPOINT: u8 = 14;
pub const TRB_STOP_ENDPOINT: u8 = 15;
pub const TRB_SET_TR_DEQUEUE: u8 = 16;
pub const TRB_RESET_DEVICE: u8 = 17;

pub const TRB_TRANSFER_EVENT: u8 = 32;
pub const TRB_COMMAND_COMPLETION: u8 = 33;
pub const TRB_PORT_STATUS_CHANGE: u8 = 34;

// ============================================================
// Completion codes
// ============================================================

pub const CC_SUCCESS: u8 = 1;
pub const CC_DATA_BUFFER_ERROR: u8 = 2;
pub const CC_BABBLE: u8 = 3;
pub const CC_USB_TRANSACTION_ERROR: u8 = 4;
pub const CC_TRB_ERROR: u8 = 5;
pub const CC_STALL: u8 = 6;
pub const CC_RESOURCE_ERROR: u8 = 7;
pub const CC_BANDWIDTH_ERROR: u8 = 8;
pub const CC_NO_SLOTS: u8 = 9;
pub const CC_INVALID_STREAM_ID: u8 = 10;
pub const CC_SLOT_NOT_ENABLED: u8 = 11;
pub const CC_ENDPOINT_NOT_ENABLED: u8 = 12;
pub const CC_SHORT_PACKET: u8 = 13;
pub const CC_RING_UNDERRUN: u8 = 14;
pub const CC_RING_OVERRUN: u8 = 15;
pub const CC_PARAMETER_ERROR: u8 = 17;
pub const CC_BANDWIDTH_OVERRUN: u8 = 18;
pub const CC_CONTEXT_STATE_ERROR: u8 = 19;
pub const CC_NO_PING_RESPONSE: u8 = 20;
pub const CC_EVENT_RING_FULL: u8 = 21;
pub const CC_INCOMPATIBLE_DEVICE: u8 = 22;
pub const CC_MISSED_SERVICE: u8 = 23;
pub const CC_COMMAND_RING_STOPPED: u8 = 24;
pub const CC_COMMAND_ABORTED: u8 = 25;
pub const CC_STOPPED: u8 = 26;

pub fn completion_name(code: u8) -> &'static str {
    match code {
        CC_SUCCESS => "success",
        CC_DATA_BUFFER_ERROR => "data buffer error",
        CC_BABBLE => "babble",
        CC_USB_TRANSACTION_ERROR => "USB transaction error",
        CC_TRB_ERROR => "TRB error",
        CC_STALL => "stall",
        CC_RESOURCE_ERROR => "resource error",
        CC_BANDWIDTH_ERROR => "bandwidth error",
        CC_NO_SLOTS => "no slots available",
        CC_INVALID_STREAM_ID => "invalid stream ID",
        CC_SLOT_NOT_ENABLED => "slot not enabled",
        CC_ENDPOINT_NOT_ENABLED => "endpoint not enabled",
        CC_SHORT_PACKET => "short packet",
        CC_RING_UNDERRUN => "ring underrun",
        CC_RING_OVERRUN => "ring overrun",
        CC_PARAMETER_ERROR => "parameter error",
        CC_BANDWIDTH_OVERRUN => "bandwidth overrun",
        CC_CONTEXT_STATE_ERROR => "context state error",
        CC_NO_PING_RESPONSE => "no ping response",
        CC_EVENT_RING_FULL => "event ring full",
        CC_INCOMPATIBLE_DEVICE => "incompatible device",
        CC_MISSED_SERVICE => "missed service",
        CC_COMMAND_RING_STOPPED => "command ring stopped",
        CC_COMMAND_ABORTED => "command aborted",
        CC_STOPPED => "stopped",
        _ => "unknown completion code",
    }
}

// ============================================================
// Setup stage transfer types
// ============================================================

pub const TRT_NO_DATA: u8 = 0;
pub const TRT_IN: u8 = 2;
pub const TRT_OUT: u8 = 3;

// ============================================================
// TRB
// ============================================================

#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct Trb {
    /// Transfer/command parameter, or event pointer.
    pub parameter: u64,
    /// Status word (length, residual, completion code).
    pub status: u32,
    /// Control word (type, flags, slot and endpoint IDs).
    pub control: u32,
}

impl Trb {
    pub const fn zero() -> Self {
        Self {
            parameter: 0,
            status: 0,
            control: 0,
        }
    }

    pub const fn new(parameter: u64, status: u32, control: u32) -> Self {
        Self {
            parameter,
            status,
            control,
        }
    }

    #[inline]
    pub fn trb_type(&self) -> u8 {
        ((self.control >> 10) & 0x3F) as u8
    }

    #[inline]
    pub fn cycle(&self) -> bool {
        self.control & 1 != 0
    }

    #[inline]
    pub fn slot_id(&self) -> u8 {
        (self.control >> 24) as u8
    }

    #[inline]
    pub fn endpoint_id(&self) -> u8 {
        ((self.control >> 16) & 0x1F) as u8
    }

    #[inline]
    pub fn completion_code(&self) -> u8 {
        (self.status >> 24) as u8
    }

    /// Residual transfer length reported by a Transfer Event.
    #[inline]
    pub fn residual(&self) -> u32 {
        self.status & 0x00FF_FFFF
    }

    /// The TRB whose completion produced this event.
    #[inline]
    pub fn event_pointer(&self) -> u64 {
        self.parameter
    }

    /// Port number from a Port Status Change Event.
    #[inline]
    pub fn port_id(&self) -> u8 {
        (self.parameter >> 24) as u8
    }

    #[inline]
    fn control(type_code: u8, slot: u8, cycle: bool) -> u32 {
        ((type_code as u32) << 10) | ((slot as u32) << 24) | cycle as u32
    }

    /// Normal TRB used for bulk and interrupt transfers.
    pub fn normal(
        buffer: u64,
        length: usize,
        direction_in: bool,
        slot: u8,
        cycle: bool,
        ioc: bool,
    ) -> Self {
        let mut control = Self::control(TRB_NORMAL, slot, cycle);
        control |= (direction_in as u32) << 16;
        if ioc {
            control |= 1 << 5;
        }

        Self {
            parameter: buffer,
            status: length as u32 & 0x1FFFF,
            control,
        }
    }

    /// Setup Stage TRB of a control transfer.
    pub fn setup(setup: &[u8; 8], trt: u8, slot: u8, cycle: bool) -> Self {
        let mut parameter = 0u64;

        for (index, byte) in setup.iter().enumerate() {
            parameter |= (*byte as u64) << (8 * index);
        }

        // IDT must be set so the device sees the packet boundaries.
        let control = Self::control(TRB_SETUP_STAGE, slot, cycle)
            | (1 << 6)
            | ((trt as u32) << 16);

        Self {
            parameter,
            status: 8,
            control,
        }
    }

    /// Data Stage TRB of a control transfer.
    pub fn data(
        buffer: u64,
        length: usize,
        direction_in: bool,
        slot: u8,
        cycle: bool,
        ioc: bool,
    ) -> Self {
        let mut control = Self::control(TRB_DATA_STAGE, slot, cycle)
            | ((direction_in as u32) << 16);

        if ioc {
            control |= 1 << 5;
        }

        Self {
            parameter: buffer,
            status: length as u32 & 0x1FFFF,
            control,
        }
    }

    /// Status Stage TRB of a control transfer.
    pub fn status(direction_in: bool, slot: u8, cycle: bool, ioc: bool) -> Self {
        let mut control =
            Self::control(TRB_STATUS_STAGE, slot, cycle) | ((direction_in as u32) << 16);

        if ioc {
            control |= 1 << 5;
        }

        Self {
            parameter: 0,
            status: 0,
            control,
        }
    }

    /// Link TRB closing a ring and pointing back at its first entry.
    pub fn link(next: u64, toggle_cycle: bool, cycle: bool) -> Self {
        Self {
            parameter: next,
            status: 0,
            control: Self::control(TRB_LINK, 0, cycle) | ((toggle_cycle as u32) << 1),
        }
    }

    pub fn enable_slot(cycle: bool) -> Self {
        Self {
            parameter: 0,
            status: 0,
            control: Self::control(TRB_ENABLE_SLOT, 0, cycle),
        }
    }

    pub fn disable_slot(slot: u8, cycle: bool) -> Self {
        Self {
            parameter: 0,
            status: 0,
            control: Self::control(TRB_DISABLE_SLOT, slot, cycle),
        }
    }

    pub fn address_device(input_context: u64, slot: u8, cycle: bool, bsr: bool) -> Self {
        let mut control = Self::control(TRB_ADDRESS_DEVICE, slot, cycle);

        // Block Set Address Request: keep the device at address 0 so its
        // first descriptors can be read before the final address is set.
        if bsr {
            control |= 1 << 9;
        }

        Self {
            parameter: input_context,
            status: 0,
            control,
        }
    }

    pub fn configure_endpoint(input_context: u64, slot: u8, cycle: bool) -> Self {
        Self {
            parameter: input_context,
            status: 0,
            control: Self::control(TRB_CONFIGURE_ENDPOINT, slot, cycle),
        }
    }

    pub fn noop(cycle: bool) -> Self {
        Self {
            parameter: 0,
            status: 0,
            control: Self::control(TRB_NOOP, 0, cycle),
        }
    }
}
