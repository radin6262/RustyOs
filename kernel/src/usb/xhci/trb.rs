#![allow(dead_code)]

/*
 * ==========================================================================
 * xHCI TRB
 * ==========================================================================
 *
 * Every xHCI TRB is exactly 16 bytes:
 *
 *     DWORD 0 : Parameter / Buffer Pointer bits 31:0
 *     DWORD 1 : Parameter / Buffer Pointer bits 63:32
 *     DWORD 2 : Status
 *     DWORD 3 : Control
 *
 * Linux represents generic TRBs as four little-endian DWORDs. This Rust
 * representation uses the equivalent:
 *
 *     parameter: u64
 *     status:    u32
 *     control:   u32
 *
 * The Rust structure is exactly 16 bytes and 16-byte aligned so it can be
 * placed directly in xHCI DMA ring memory.
 *
 * ==========================================================================*/

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Trb {
    /*
     * DWORD 0-1
     *
     * Type-dependent parameter.
     *
     * Examples:
     *
     *     Normal TRB:
     *         Data Buffer Pointer
     *
     *     Link TRB:
     *         Segment Pointer
     *
     *     Address Device:
     *         Input Context Pointer
     *
     *     Command Completion Event:
     *         Command TRB Pointer
     *
     *     Transfer Event:
     *         Buffer / TRB Pointer
     */
    pub parameter: u64,

    /*
     * DWORD 2
     *
     * Type-dependent status/length fields.
     */
    pub status: u32,

    /*
     * DWORD 3
     *
     * Common:
     *
     *     bit 0      Cycle
     *     bits 9     BEI
     *     bits 15:10 TRB Type
     *
     * Additional fields are TRB-specific.
     */
    pub control: u32,
}

/*
 * ==========================================================================
 * Generic TRB control bits
 * ==========================================================================
 *
 * These correspond to Linux xhci.h:
 *
 *     TRB_CYCLE
 *     TRB_ENT
 *     TRB_ISP
 *     TRB_NO_SNOOP
 *     TRB_CHAIN
 *     TRB_IOC
 *     TRB_IDT
 *     TRB_BEI
 * ==========================================================================*/

pub const CONTROL_CYCLE: u32 = 1 << 0;

/*
 * ENT:
 *
 * Force next Event Data TRB to be evaluated before task switch.
 */
pub const CONTROL_ENT: u32 = 1 << 1;

/*
 * ISP:
 *
 * Interrupt on Short Packet.
 */
pub const CONTROL_ISP: u32 = 1 << 2;

/*
 * No Snoop.
 */
pub const CONTROL_NO_SNOOP: u32 = 1 << 3;

/*
 * Chain multiple TRBs into one TD.
 */
pub const CONTROL_CHAIN: u32 = 1 << 4;

/*
 * Interrupt on Completion.
 */
pub const CONTROL_IOC: u32 = 1 << 5;

/*
 * Immediate Data.
 */
pub const CONTROL_IDT: u32 = 1 << 6;

/*
 * Bits 7:8 reserved.
 */

/*
 * Block Event Interrupt.
 */
pub const CONTROL_BEI: u32 = 1 << 9;

/*
 * ==========================================================================
 * TRB type field
 * ==========================================================================
 *
 * Linux:
 *
 *     TRB_TYPE_BITMASK = 0xfc00
 *     TRB_TYPE(p)      = p << 10
 *     TRB_FIELD_TO_TYPE(p) = (p & 0xfc00) >> 10
 * ==========================================================================*/

pub const TRB_TYPE_SHIFT: u32 = 10;

pub const TRB_TYPE_BITMASK: u32 = 0xFC00;

pub const TRB_TYPE_MASK: u32 = TRB_TYPE_BITMASK;

/*
 * ==========================================================================
 * Generic status fields
 * ==========================================================================
 *
 * These are the normal xHCI TRB field positions used by Linux.
 * ==========================================================================*/

/*
 * Transfer length:
 *
 * bits 16:0
 */
pub const STATUS_TRANSFER_LENGTH_MASK: u32 = 0x0001_FFFF;

/*
 * TD Size:
 *
 * bits 21:17
 */
pub const STATUS_TD_SIZE_MASK: u32 = 0x003E_0000;

pub const STATUS_TD_SIZE_SHIFT: u32 = 17;

/*
 * Interrupter Target:
 *
 * bits 31:22
 *
 * This selects the interrupter/MSI-X vector targeted by the event.
 */
pub const STATUS_INTERRUPT_TARGET_MASK: u32 = 0xFFC0_0000;

pub const STATUS_INTERRUPT_TARGET_SHIFT: u32 = 22;

/*
 * Transfer Event Length:
 *
 * bits 23:0
 */
pub const STATUS_EVENT_LENGTH_MASK: u32 = 0x00FF_FFFF;

/*
 * Completion Code:
 *
 * bits 31:24
 */
pub const STATUS_COMPLETION_CODE_MASK: u32 = 0xFF00_0000;

pub const STATUS_COMPLETION_CODE_SHIFT: u32 = 24;

/*
 * ==========================================================================
 * Control transfer fields
 * ==========================================================================
 */

pub const CONTROL_DIRECTION_IN: u32 = 1 << 16;

/*
 * Setup Stage / Data Stage transfer type:
 *
 * bits 17:16
 */
pub const CONTROL_TRANSFER_TYPE_MASK: u32 = 0x3 << 16;

pub const CONTROL_TRANSFER_TYPE_SHIFT: u32 = 16;

/*
 * xHCI transfer type values.
 */

pub const TRANSFER_TYPE_RESERVED: u8 = 0;

pub const TRANSFER_TYPE_RESERVED_1: u8 = 1;

pub const TRANSFER_TYPE_OUT: u8 = 2;

pub const TRANSFER_TYPE_IN: u8 = 3;

/*
 * ==========================================================================
 * Isochronous TRB fields
 * ==========================================================================
 *
 * Linux xhci.h:
 *
 *     TRB_SIA
 *     TRB_FRAME_ID
 *     TRB_TBC
 *     TRB_TLBPC
 * ==========================================================================*/

/*
 * Start Isochronous ASAP.
 */
pub const ISOCH_START_IMMEDIATELY: u32 = 1 << 31;

/*
 * Frame ID:
 *
 * bits 30:20
 */
pub const ISOCH_FRAME_ID_MASK: u32 = 0x7FF << 20;

pub const ISOCH_FRAME_ID_SHIFT: u32 = 20;

/*
 * Total Burst Count:
 *
 * bits 8:7
 */
pub const ISOCH_TOTAL_BURST_COUNT_MASK: u32 = 0x3 << 7;

pub const ISOCH_TOTAL_BURST_COUNT_SHIFT: u32 = 7;

/*
 * Transfer Burst Count / TLBPC:
 *
 * bits 19:16
 */
pub const ISOCH_TLBPC_MASK: u32 = 0xF << 16;

pub const ISOCH_TLBPC_SHIFT: u32 = 16;

/*
 * ==========================================================================
 * Event TRB fields
 * ==========================================================================
 */

/*
 * Event Data:
 *
 * bit 2
 */
pub const EVENT_DATA: u32 = 1 << 2;

/*
 * Slot ID:
 *
 * bits 31:24 of DWORD 3.
 */
pub const EVENT_SLOT_ID_MASK: u32 = 0xFF00_0000;

pub const EVENT_SLOT_ID_SHIFT: u32 = 24;

/*
 * Endpoint ID:
 *
 * bits 20:16 of DWORD 3.
 */
pub const EVENT_ENDPOINT_ID_MASK: u32 = 0x001F_0000;

pub const EVENT_ENDPOINT_ID_SHIFT: u32 = 16;

/*
 * ==========================================================================
 * Command-specific bits
 * ==========================================================================
 */

/*
 * Address Device:
 *
 * Block Set Address Request.
 */
pub const COMMAND_BSR: u32 = 1 << 9;

/*
 * Configure Endpoint:
 *
 * Deconfigure.
 */
pub const COMMAND_DC: u32 = 1 << 9;

/*
 * Stop Endpoint:
 *
 * Transfer State Preserve.
 */
pub const COMMAND_TSP: u32 = 1 << 9;

/*
 * Set TR Dequeue Pointer:
 *
 * Stream Context Type:
 *
 * bits 3:1.
 */
pub const COMMAND_SCT_MASK: u32 = 0x7 << 1;

pub const COMMAND_SCT_SHIFT: u32 = 1;

/*
 * Stream ID:
 *
 * bits 31:16.
 */
pub const COMMAND_STREAM_ID_MASK: u32 = 0xFFFF_0000;

pub const COMMAND_STREAM_ID_SHIFT: u32 = 16;

/*
 * ==========================================================================
 * Link TRB
 * ==========================================================================
 *
 * Linux:
 *
 *     #define LINK_TOGGLE BIT(1)
 *
 * Bit 0 remains the normal Cycle bit.
 * Bit 1 is Link Toggle Cycle.
 * ==========================================================================*/

pub const LINK_TOGGLE_CYCLE: u32 = 1 << 1;

/*
 * ==========================================================================
 * Completion codes
 * ==========================================================================
 *
 * Values match Linux's xhci.h completion-code definitions.
 * ==========================================================================*/

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionCode {
    Invalid = 0,

    Success = 1,

    DataBufferError = 2,

    BabbleDetectedError = 3,

    UsbTransactionError = 4,

    TrbError = 5,

    StallError = 6,

    ResourceError = 7,

    BandwidthError = 8,

    NoSlotsAvailableError = 9,

    InvalidStreamTypeError = 10,

    SlotNotEnabledError = 11,

    EndpointNotEnabledError = 12,

    ShortPacket = 13,

    RingUnderrun = 14,

    RingOverrun = 15,

    VfEventRingFullError = 16,

    ParameterError = 17,

    BandwidthOverrunError = 18,

    ContextStateError = 19,

    NoPingResponseError = 20,

    EventRingFullError = 21,

    IncompatibleDeviceError = 22,

    MissedServiceError = 23,

    CommandRingStopped = 24,

    CommandAborted = 25,

    Stopped = 26,

    StoppedLengthInvalid = 27,

    StoppedShortPacket = 28,

    MaxExitLatencyTooLargeError = 29,

    IsochBufferOverrun = 31,

    EventLostError = 32,

    UndefinedError = 33,

    InvalidStreamIdError = 34,

    SecondaryBandwidthError = 35,

    SplitTransactionError = 36,
}

impl CompletionCode {
    #[inline(always)]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Invalid),

            1 => Some(Self::Success),

            2 => Some(Self::DataBufferError),

            3 => Some(Self::BabbleDetectedError),

            4 => Some(Self::UsbTransactionError),

            5 => Some(Self::TrbError),

            6 => Some(Self::StallError),

            7 => Some(Self::ResourceError),

            8 => Some(Self::BandwidthError),

            9 => Some(Self::NoSlotsAvailableError),

            10 => Some(Self::InvalidStreamTypeError),

            11 => Some(Self::SlotNotEnabledError),

            12 => Some(Self::EndpointNotEnabledError),

            13 => Some(Self::ShortPacket),

            14 => Some(Self::RingUnderrun),

            15 => Some(Self::RingOverrun),

            16 => Some(Self::VfEventRingFullError),

            17 => Some(Self::ParameterError),

            18 => Some(Self::BandwidthOverrunError),

            19 => Some(Self::ContextStateError),

            20 => Some(Self::NoPingResponseError),

            21 => Some(Self::EventRingFullError),

            22 => Some(Self::IncompatibleDeviceError),

            23 => Some(Self::MissedServiceError),

            24 => Some(Self::CommandRingStopped),

            25 => Some(Self::CommandAborted),

            26 => Some(Self::Stopped),

            27 => Some(Self::StoppedLengthInvalid),

            28 => Some(Self::StoppedShortPacket),

            29 => Some(Self::MaxExitLatencyTooLargeError),

            /*
             * 30 is reserved.
             */
            31 => Some(Self::IsochBufferOverrun),

            32 => Some(Self::EventLostError),

            33 => Some(Self::UndefinedError),

            34 => Some(Self::InvalidStreamIdError),

            35 => Some(Self::SecondaryBandwidthError),

            36 => Some(Self::SplitTransactionError),

            _ => None,
        }
    }

    #[inline(always)]
    pub const fn from_status(status: u32) -> Option<Self> {
        Self::from_u8(
            ((status & STATUS_COMPLETION_CODE_MASK) >> STATUS_COMPLETION_CODE_SHIFT) as u8,
        )
    }

    #[inline(always)]
    pub const fn code(self) -> u8 {
        self as u8
    }
}

/*
 * ==========================================================================
 * TRB types
 * ==========================================================================
 *
 * Numeric values match the xHCI specification and Linux's TRB_* constants.
 *
 *     1..8
 *         Transfer TRBs
 *
 *     9..23
 *         Command TRBs
 *
 *     32..39
 *         Event TRBs
 * ==========================================================================*/

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrbType {
    /*
     * Transfer TRBs.
     */
    Normal = 1,

    SetupStage = 2,

    DataStage = 3,

    StatusStage = 4,

    Isoch = 5,

    Link = 6,

    EventData = 7,

    NoOp = 8,

    /*
     * Command TRBs.
     */
    EnableSlotCommand = 9,

    DisableSlotCommand = 10,

    AddressDeviceCommand = 11,

    ConfigureEndpointCommand = 12,

    EvaluateContextCommand = 13,

    ResetEndpointCommand = 14,

    StopEndpointCommand = 15,

    SetTrDequeuePointerCommand = 16,

    ResetDeviceCommand = 17,

    ForceEventCommand = 18,

    NegotiateBandwidthCommand = 19,

    SetLatencyToleranceValueCommand = 20,

    GetPortBandwidthCommand = 21,

    ForceHeaderCommand = 22,

    NoOpCommand = 23,

    /*
     * Event TRBs.
     */
    TransferEvent = 32,

    CommandCompletionEvent = 33,

    PortStatusChangeEvent = 34,

    BandwidthRequestEvent = 35,

    DoorbellEvent = 36,

    HostControllerEvent = 37,

    DeviceNotificationEvent = 38,

    MfIndexWrapEvent = 39,
}

impl TrbType {
    #[inline(always)]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    #[inline(always)]
    pub const fn as_u32(self) -> u32 {
        self as u32
    }

    #[inline(always)]
    pub const fn control_bits(self) -> u32 {
        (self as u32) << TRB_TYPE_SHIFT
    }

    #[inline(always)]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Normal),

            2 => Some(Self::SetupStage),

            3 => Some(Self::DataStage),

            4 => Some(Self::StatusStage),

            5 => Some(Self::Isoch),

            6 => Some(Self::Link),

            7 => Some(Self::EventData),

            8 => Some(Self::NoOp),

            9 => Some(Self::EnableSlotCommand),

            10 => Some(Self::DisableSlotCommand),

            11 => Some(Self::AddressDeviceCommand),

            12 => Some(Self::ConfigureEndpointCommand),

            13 => Some(Self::EvaluateContextCommand),

            14 => Some(Self::ResetEndpointCommand),

            15 => Some(Self::StopEndpointCommand),

            16 => Some(Self::SetTrDequeuePointerCommand),

            17 => Some(Self::ResetDeviceCommand),

            18 => Some(Self::ForceEventCommand),

            19 => Some(Self::NegotiateBandwidthCommand),

            20 => Some(Self::SetLatencyToleranceValueCommand),

            21 => Some(Self::GetPortBandwidthCommand),

            22 => Some(Self::ForceHeaderCommand),

            23 => Some(Self::NoOpCommand),

            32 => Some(Self::TransferEvent),

            33 => Some(Self::CommandCompletionEvent),

            34 => Some(Self::PortStatusChangeEvent),

            35 => Some(Self::BandwidthRequestEvent),

            36 => Some(Self::DoorbellEvent),

            37 => Some(Self::HostControllerEvent),

            38 => Some(Self::DeviceNotificationEvent),

            39 => Some(Self::MfIndexWrapEvent),

            _ => None,
        }
    }
}

/*
 * ==========================================================================
 * TRB implementation
 * ==========================================================================
 */

impl Trb {
    /*
     * ----------------------------------------------------------------------
     * Constructor
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn new(parameter: u64, status: u32, control: u32) -> Self {
        Self {
            parameter,
            status,
            control,
        }
    }

    /*
     * ----------------------------------------------------------------------
     * Zero TRB
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn zero() -> Self {
        Self {
            parameter: 0,
            status: 0,
            control: 0,
        }
    }

    /*
     * ----------------------------------------------------------------------
     * TRB type
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn trb_type(&self) -> u8 {
        ((self.control & TRB_TYPE_BITMASK) >> TRB_TYPE_SHIFT) as u8
    }

    #[inline(always)]
    pub const fn trb_type_enum(&self) -> Option<TrbType> {
        TrbType::from_u8(self.trb_type())
    }

    #[inline(always)]
    pub const fn is_type(&self, trb_type: TrbType) -> bool {
        self.trb_type() == trb_type.as_u8()
    }

    /*
     * ----------------------------------------------------------------------
     * Cycle
     * ----------------------------------------------------------------------
     *
     * There is intentionally only ONE implementation of these methods.
     * EventRing uses cycle(), while other code can use cycle_bit().
     */

    #[inline(always)]
    pub const fn cycle_bit(&self) -> bool {
        (self.control & CONTROL_CYCLE) != 0
    }

    /*
     * Compatibility API used by event_ring.rs.
     */
    #[inline(always)]
    pub const fn cycle(&self) -> bool {
        self.cycle_bit()
    }

    #[inline(always)]
    pub fn set_cycle(&mut self, cycle: bool) {
        if cycle {
            self.control |= CONTROL_CYCLE;
        } else {
            self.control &= !CONTROL_CYCLE;
        }
    }

    #[inline(always)]
    pub fn with_cycle(mut self, cycle: bool) -> Self {
        self.set_cycle(cycle);

        self
    }

    /*
     * ----------------------------------------------------------------------
     * ENT
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn evaluate_next_event_data(&self) -> bool {
        (self.control & CONTROL_ENT) != 0
    }

    #[inline(always)]
    pub fn set_evaluate_next_event_data(&mut self, enabled: bool) {
        if enabled {
            self.control |= CONTROL_ENT;
        } else {
            self.control &= !CONTROL_ENT;
        }
    }

    /*
     * ----------------------------------------------------------------------
     * Chain
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn chain(&self) -> bool {
        (self.control & CONTROL_CHAIN) != 0
    }

    #[inline(always)]
    pub fn set_chain(&mut self, enabled: bool) {
        if enabled {
            self.control |= CONTROL_CHAIN;
        } else {
            self.control &= !CONTROL_CHAIN;
        }
    }

    /*
     * ----------------------------------------------------------------------
     * Interrupt on Completion
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn interrupt_on_completion(&self) -> bool {
        (self.control & CONTROL_IOC) != 0
    }

    #[inline(always)]
    pub fn set_interrupt_on_completion(&mut self, enabled: bool) {
        if enabled {
            self.control |= CONTROL_IOC;
        } else {
            self.control &= !CONTROL_IOC;
        }
    }

    /*
     * ----------------------------------------------------------------------
     * Interrupt on Short Packet
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn interrupt_on_short_packet(&self) -> bool {
        (self.control & CONTROL_ISP) != 0
    }

    #[inline(always)]
    pub fn set_interrupt_on_short_packet(&mut self, enabled: bool) {
        if enabled {
            self.control |= CONTROL_ISP;
        } else {
            self.control &= !CONTROL_ISP;
        }
    }

    /*
     * ----------------------------------------------------------------------
     * No Snoop
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn no_snoop(&self) -> bool {
        (self.control & CONTROL_NO_SNOOP) != 0
    }

    #[inline(always)]
    pub fn set_no_snoop(&mut self, enabled: bool) {
        if enabled {
            self.control |= CONTROL_NO_SNOOP;
        } else {
            self.control &= !CONTROL_NO_SNOOP;
        }
    }

    /*
     * ----------------------------------------------------------------------
     * Immediate Data
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn immediate_data(&self) -> bool {
        (self.control & CONTROL_IDT) != 0
    }

    #[inline(always)]
    pub fn set_immediate_data(&mut self, enabled: bool) {
        if enabled {
            self.control |= CONTROL_IDT;
        } else {
            self.control &= !CONTROL_IDT;
        }
    }

    /*
     * ----------------------------------------------------------------------
     * Block Event Interrupt
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn block_event_interrupt(&self) -> bool {
        (self.control & CONTROL_BEI) != 0
    }

    #[inline(always)]
    pub fn set_block_event_interrupt(&mut self, enabled: bool) {
        if enabled {
            self.control |= CONTROL_BEI;
        } else {
            self.control &= !CONTROL_BEI;
        }
    }

    /*
     * ----------------------------------------------------------------------
     * Link TRB Toggle Cycle
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn link_toggle_cycle(&self) -> bool {
        (self.control & LINK_TOGGLE_CYCLE) != 0
    }

    /*
     * Compatibility name.
     */
    #[inline(always)]
    pub const fn link_toggle(&self) -> bool {
        self.link_toggle_cycle()
    }

    #[inline(always)]
    pub fn set_link_toggle_cycle(&mut self, enabled: bool) {
        if enabled {
            self.control |= LINK_TOGGLE_CYCLE;
        } else {
            self.control &= !LINK_TOGGLE_CYCLE;
        }
    }

    #[inline(always)]
    pub fn set_link_toggle(&mut self, enabled: bool) {
        self.set_link_toggle_cycle(enabled);
    }

    /*
     * ----------------------------------------------------------------------
     * Transfer length
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn transfer_length(&self) -> u32 {
        self.status & STATUS_TRANSFER_LENGTH_MASK
    }

    #[inline(always)]
    pub fn set_transfer_length(&mut self, length: u32) {
        self.status &= !STATUS_TRANSFER_LENGTH_MASK;

        self.status |= length & STATUS_TRANSFER_LENGTH_MASK;
    }

    /*
     * ----------------------------------------------------------------------
     * Event transfer length
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn event_length(&self) -> u32 {
        self.status & STATUS_EVENT_LENGTH_MASK
    }

    /*
     * ----------------------------------------------------------------------
     * TD size
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn td_size(&self) -> u8 {
        ((self.status & STATUS_TD_SIZE_MASK) >> STATUS_TD_SIZE_SHIFT) as u8
    }

    #[inline(always)]
    pub fn set_td_size(&mut self, value: u8) {
        let value = value.min(31);

        self.status &= !STATUS_TD_SIZE_MASK;

        self.status |= (value as u32) << STATUS_TD_SIZE_SHIFT;
    }

    /*
     * ----------------------------------------------------------------------
     * Interrupter Target
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn interrupter_target(&self) -> u16 {
        ((self.status & STATUS_INTERRUPT_TARGET_MASK) >> STATUS_INTERRUPT_TARGET_SHIFT) as u16
    }

    #[inline(always)]
    pub fn set_interrupter_target(&mut self, target: u16) {
        let target = target.min(0x03FF);

        self.status &= !STATUS_INTERRUPT_TARGET_MASK;

        self.status |= (target as u32) << STATUS_INTERRUPT_TARGET_SHIFT;
    }

    /*
     * ----------------------------------------------------------------------
     * Completion code
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn completion_code(&self) -> u8 {
        ((self.status & STATUS_COMPLETION_CODE_MASK) >> STATUS_COMPLETION_CODE_SHIFT) as u8
    }

    #[inline(always)]
    pub const fn completion(&self) -> Option<CompletionCode> {
        CompletionCode::from_u8(self.completion_code())
    }

    /*
     * ----------------------------------------------------------------------
     * Event Slot ID
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn slot_id(&self) -> u8 {
        ((self.control & EVENT_SLOT_ID_MASK) >> EVENT_SLOT_ID_SHIFT) as u8
    }

    #[inline(always)]
    pub fn set_slot_id(&mut self, slot_id: u8) {
        self.control &= !EVENT_SLOT_ID_MASK;

        self.control |= (slot_id as u32) << EVENT_SLOT_ID_SHIFT;
    }

    /*
     * ----------------------------------------------------------------------
     * Event Endpoint ID
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn endpoint_id(&self) -> u8 {
        ((self.control & EVENT_ENDPOINT_ID_MASK) >> EVENT_ENDPOINT_ID_SHIFT) as u8
    }

    #[inline(always)]
    pub fn set_endpoint_id(&mut self, endpoint_id: u8) {
        let endpoint_id = endpoint_id.min(31);

        self.control &= !EVENT_ENDPOINT_ID_MASK;

        self.control |= (endpoint_id as u32) << EVENT_ENDPOINT_ID_SHIFT;
    }

    /*
     * ----------------------------------------------------------------------
     * Control Transfer Direction
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn direction_in(&self) -> bool {
        (self.control & CONTROL_DIRECTION_IN) != 0
    }

    #[inline(always)]
    pub fn set_direction_in(&mut self, direction_in: bool) {
        if direction_in {
            self.control |= CONTROL_DIRECTION_IN;
        } else {
            self.control &= !CONTROL_DIRECTION_IN;
        }
    }

    /*
     * ----------------------------------------------------------------------
     * Control Transfer Type
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn transfer_type(&self) -> u8 {
        ((self.control & CONTROL_TRANSFER_TYPE_MASK) >> CONTROL_TRANSFER_TYPE_SHIFT) as u8
    }

    #[inline(always)]
    pub fn set_transfer_type(&mut self, transfer_type: u8) {
        let transfer_type = transfer_type & 0x3;

        self.control &= !CONTROL_TRANSFER_TYPE_MASK;

        self.control |= (transfer_type as u32) << CONTROL_TRANSFER_TYPE_SHIFT;
    }

    /*
     * ----------------------------------------------------------------------
     * Isochronous Frame ID
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn frame_id(&self) -> u16 {
        ((self.control & ISOCH_FRAME_ID_MASK) >> ISOCH_FRAME_ID_SHIFT) as u16
    }

    #[inline(always)]
    pub fn set_frame_id(&mut self, frame_id: u16) {
        let frame_id = frame_id & 0x07FF;

        self.control &= !ISOCH_FRAME_ID_MASK;

        self.control |= (frame_id as u32) << ISOCH_FRAME_ID_SHIFT;
    }

    /*
     * ----------------------------------------------------------------------
     * Isochronous Start Immediately
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn isoch_start_immediately(&self) -> bool {
        (self.control & ISOCH_START_IMMEDIATELY) != 0
    }

    #[inline(always)]
    pub fn set_isoch_start_immediately(&mut self, enabled: bool) {
        if enabled {
            self.control |= ISOCH_START_IMMEDIATELY;
        } else {
            self.control &= !ISOCH_START_IMMEDIATELY;
        }
    }

    /*
     * ----------------------------------------------------------------------
     * Isochronous Total Burst Count
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn total_burst_count(&self) -> u8 {
        ((self.control & ISOCH_TOTAL_BURST_COUNT_MASK) >> ISOCH_TOTAL_BURST_COUNT_SHIFT) as u8
    }

    #[inline(always)]
    pub fn set_total_burst_count(&mut self, value: u8) {
        let value = value.min(3);

        self.control &= !ISOCH_TOTAL_BURST_COUNT_MASK;

        self.control |= (value as u32) << ISOCH_TOTAL_BURST_COUNT_SHIFT;
    }

    /*
     * ----------------------------------------------------------------------
     * Isochronous TLBPC
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn tlbpc(&self) -> u8 {
        ((self.control & ISOCH_TLBPC_MASK) >> ISOCH_TLBPC_SHIFT) as u8
    }

    #[inline(always)]
    pub fn set_tlbpc(&mut self, value: u8) {
        let value = value.min(15);

        self.control &= !ISOCH_TLBPC_MASK;

        self.control |= (value as u32) << ISOCH_TLBPC_SHIFT;
    }

    /*
     * ----------------------------------------------------------------------
     * Builder: TRB type
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn with_type(mut self, trb_type: TrbType) -> Self {
        self.control &= !TRB_TYPE_BITMASK;

        self.control |= trb_type.control_bits();

        self
    }

    /*
     * ----------------------------------------------------------------------
     * Builder: Generic flags
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn with_flags(mut self, flags: u32) -> Self {
        self.control |= flags;

        self
    }

    /*
     * ----------------------------------------------------------------------
     * Builder: parameter
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn with_parameter(mut self, parameter: u64) -> Self {
        self.parameter = parameter;

        self
    }

    /*
     * ----------------------------------------------------------------------
     * Builder: status
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn with_status(mut self, status: u32) -> Self {
        self.status = status;

        self
    }

    /*
     * ----------------------------------------------------------------------
     * Builder: command TRB
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn command(trb_type: TrbType, parameter: u64, control_flags: u32) -> Self {
        Self {
            parameter,

            status: 0,

            control: trb_type.control_bits() | control_flags,
        }
    }

    /*
     * ----------------------------------------------------------------------
     * Builder: Link TRB
     * ----------------------------------------------------------------------
     *
     * Parameter = next segment physical address
     * Cycle     = current producer cycle
     * Toggle    = Toggle Cycle
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn link(segment_phys: u64, cycle: bool, toggle_cycle: bool) -> Self {
        let mut control = TrbType::Link.control_bits();

        if cycle {
            control |= CONTROL_CYCLE;
        }

        if toggle_cycle {
            control |= LINK_TOGGLE_CYCLE;
        }

        Self {
            parameter: segment_phys,

            status: 0,

            control,
        }
    }

    /*
     * ----------------------------------------------------------------------
     * Builder: Enable Slot command
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn enable_slot(cycle: bool) -> Self {
        let mut trb = Self::command(TrbType::EnableSlotCommand, 0, 0);

        if cycle {
            trb.control |= CONTROL_CYCLE;
        }

        trb
    }

    /*
     * ----------------------------------------------------------------------
     * Builder: No-op command
     * ----------------------------------------------------------------------
     */

    #[inline(always)]
    pub const fn noop_command(cycle: bool) -> Self {
        let mut trb = Self::command(TrbType::NoOpCommand, 0, 0);

        if cycle {
            trb.control |= CONTROL_CYCLE;
        }

        trb
    }
}

/*
 * ==========================================================================
 * Compile-time layout checks
 * ==========================================================================
 */

const _: () = {
    assert!(core::mem::size_of::<Trb>() == 16);

    assert!(core::mem::align_of::<Trb>() == 16);
};
