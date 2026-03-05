//! xHCI Data Structures — Phase 21a.
//!
//! Transfer Request Blocks (TRBs), Device Contexts, Endpoint Contexts,
//! Event Ring Segment Table entries, and the Device Context Base Address
//! Array (DCBAA). All structures are #[repr(C)] for hardware layout.

#![allow(dead_code)]

// ============================================================================
// Transfer Request Blocks (TRBs) — 16 bytes each
// ============================================================================

/// A generic TRB (Transfer Request Block) — 16 bytes.
/// All rings (Command, Transfer, Event) use this same structure.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Trb {
    /// Parameter (meaning depends on TRB type).
    pub parameter: u64,
    /// Status field.
    pub status: u32,
    /// Control field: [15:10] TRB Type, [0] Cycle bit, plus type-specific bits.
    pub control: u32,
}

impl Trb {
    pub const SIZE: usize = 16;

    /// Create a zeroed TRB.
    pub fn zero() -> Self {
        Self {
            parameter: 0,
            status: 0,
            control: 0,
        }
    }

    /// Get the TRB type from the control field.
    pub fn trb_type(&self) -> u8 {
        ((self.control >> 10) & 0x3F) as u8
    }

    /// Get the cycle bit.
    pub fn cycle(&self) -> bool {
        self.control & 1 != 0
    }

    /// Set the cycle bit.
    pub fn set_cycle(&mut self, cycle: bool) {
        if cycle {
            self.control |= 1;
        } else {
            self.control &= !1;
        }
    }

    /// Set the TRB type.
    pub fn set_type(&mut self, trb_type: u8) {
        self.control = (self.control & !(0x3F << 10)) | ((trb_type as u32 & 0x3F) << 10);
    }

    /// Build a Link TRB pointing to the start of the ring.
    pub fn link(ring_phys: u64, cycle: bool) -> Self {
        let mut trb = Self::zero();
        trb.parameter = ring_phys;
        trb.set_type(TRB_TYPE_LINK);
        trb.set_cycle(cycle);
        // Set Toggle Cycle bit (bit 1 of control) so cycle flips on wrap
        trb.control |= 1 << 1;
        trb
    }

    /// Build a No-Op Command TRB (for testing the command ring).
    pub fn noop_command(cycle: bool) -> Self {
        let mut trb = Self::zero();
        trb.set_type(TRB_TYPE_NO_OP_CMD);
        trb.set_cycle(cycle);
        trb
    }

    /// Build an Enable Slot Command TRB.
    pub fn enable_slot(cycle: bool) -> Self {
        let mut trb = Self::zero();
        trb.set_type(TRB_TYPE_ENABLE_SLOT);
        trb.set_cycle(cycle);
        trb
    }
}

// ── TRB Type codes (control[15:10]) ─────────────────────────────────

// Transfer Ring TRB types
pub const TRB_TYPE_NORMAL: u8 = 1;
pub const TRB_TYPE_SETUP_STAGE: u8 = 2;
pub const TRB_TYPE_DATA_STAGE: u8 = 3;
pub const TRB_TYPE_STATUS_STAGE: u8 = 4;
pub const TRB_TYPE_ISOCH: u8 = 5;
pub const TRB_TYPE_LINK: u8 = 6;
pub const TRB_TYPE_EVENT_DATA: u8 = 7;
pub const TRB_TYPE_NO_OP_TRANSFER: u8 = 8;

// Command Ring TRB types
pub const TRB_TYPE_ENABLE_SLOT: u8 = 9;
pub const TRB_TYPE_DISABLE_SLOT: u8 = 10;
pub const TRB_TYPE_ADDRESS_DEVICE: u8 = 11;
pub const TRB_TYPE_CONFIG_EP: u8 = 12;
pub const TRB_TYPE_EVALUATE_CTX: u8 = 13;
pub const TRB_TYPE_RESET_EP: u8 = 14;
pub const TRB_TYPE_STOP_EP: u8 = 15;
pub const TRB_TYPE_SET_TR_DEQUEUE: u8 = 16;
pub const TRB_TYPE_RESET_DEVICE: u8 = 17;
pub const TRB_TYPE_NO_OP_CMD: u8 = 23;

// Event Ring TRB types
pub const TRB_TYPE_TRANSFER_EVENT: u8 = 32;
pub const TRB_TYPE_CMD_COMPLETION: u8 = 33;
pub const TRB_TYPE_PORT_STATUS_CHANGE: u8 = 34;
pub const TRB_TYPE_BANDWIDTH_REQUEST: u8 = 35;
pub const TRB_TYPE_DOORBELL_EVENT: u8 = 36;
pub const TRB_TYPE_HOST_CONTROLLER: u8 = 37;
pub const TRB_TYPE_DEVICE_NOTIFICATION: u8 = 38;
pub const TRB_TYPE_MFINDEX_WRAP: u8 = 39;

// ── Completion Codes (from Command Completion / Transfer Event TRBs) ─

pub const CC_INVALID: u8 = 0;
pub const CC_SUCCESS: u8 = 1;
pub const CC_DATA_BUFFER_ERROR: u8 = 2;
pub const CC_BABBLE_DETECTED: u8 = 3;
pub const CC_USB_TRANSACTION_ERROR: u8 = 4;
pub const CC_TRB_ERROR: u8 = 5;
pub const CC_STALL_ERROR: u8 = 6;
pub const CC_SHORT_PACKET: u8 = 13;
pub const CC_RING_UNDERRUN: u8 = 14;
pub const CC_RING_OVERRUN: u8 = 15;
pub const CC_NO_SLOTS_AVAILABLE: u8 = 9;
pub const CC_SLOT_NOT_ENABLED: u8 = 11;
pub const CC_EP_NOT_ENABLED: u8 = 12;
pub const CC_COMMAND_RING_STOPPED: u8 = 24;
pub const CC_COMMAND_ABORTED: u8 = 25;
pub const CC_STOPPED: u8 = 26;

/// Extract completion code from a TRB status field.
pub fn trb_completion_code(status: u32) -> u8 {
    ((status >> 24) & 0xFF) as u8
}

/// Extract slot ID from a Command Completion Event TRB control field.
pub fn trb_slot_id(control: u32) -> u8 {
    ((control >> 24) & 0xFF) as u8
}

// ============================================================================
// Ring Management
// ============================================================================

/// Number of TRBs per ring (last one is a Link TRB).
pub const RING_SIZE: usize = 256;

/// A TRB ring (Command Ring, Transfer Ring, or Event Ring).
/// The ring is a DMA-allocated array of TRBs with a Link TRB at the end.
#[derive(Debug)]
pub struct TrbRing {
    /// Virtual address of the TRB array.
    pub virt: usize,
    /// Physical address of the TRB array (for hardware).
    pub phys: usize,
    /// Number of TRBs (including Link TRB for producer rings).
    pub size: usize,
    /// Current producer/consumer index.
    pub enqueue_index: usize,
    /// Dequeue index (for event ring consumer).
    pub dequeue_index: usize,
    /// Current cycle state (toggles on ring wrap).
    pub cycle: bool,
}

impl TrbRing {
    /// Create a new ring descriptor (caller must allocate DMA memory).
    pub fn new(virt: usize, phys: usize, size: usize) -> Self {
        Self {
            virt,
            phys,
            size,
            enqueue_index: 0,
            dequeue_index: 0,
            cycle: true, // PCS = 1 initially
        }
    }

    /// Get pointer to TRB at index.
    pub fn trb_at(&self, index: usize) -> *mut Trb {
        unsafe { (self.virt as *mut Trb).add(index) }
    }

    /// Read a TRB at index.
    pub fn read_trb(&self, index: usize) -> Trb {
        unsafe { core::ptr::read_volatile(self.trb_at(index)) }
    }

    /// Write a TRB at index.
    pub fn write_trb(&self, index: usize, trb: Trb) {
        unsafe { core::ptr::write_volatile(self.trb_at(index), trb) }
    }

    /// Enqueue a TRB onto a producer ring (Command/Transfer).
    /// Returns the physical address of the enqueued TRB.
    /// The last slot is reserved for the Link TRB.
    pub fn enqueue(&mut self, mut trb: Trb) -> Option<u64> {
        if self.enqueue_index >= self.size - 1 {
            return None; // ring full (Link TRB slot)
        }

        trb.set_cycle(self.cycle);
        self.write_trb(self.enqueue_index, trb);

        let trb_phys = self.phys as u64 + (self.enqueue_index as u64) * (Trb::SIZE as u64);
        self.enqueue_index += 1;

        // If we've hit the Link TRB, wrap around
        if self.enqueue_index >= self.size - 1 {
            // The Link TRB should already be in place
            self.enqueue_index = 0;
            self.cycle = !self.cycle;
        }

        Some(trb_phys)
    }

    /// Initialize a producer ring: zero all TRBs, place Link TRB at end.
    pub fn init_producer(&mut self) {
        // Zero all TRBs
        for i in 0..self.size {
            self.write_trb(i, Trb::zero());
        }

        // Place Link TRB at the last position
        let link = Trb::link(self.phys as u64, true);
        self.write_trb(self.size - 1, link);

        self.enqueue_index = 0;
        self.cycle = true;
    }

    /// Initialize a consumer ring (Event Ring): zero all TRBs, no Link TRB.
    pub fn init_consumer(&mut self) {
        for i in 0..self.size {
            self.write_trb(i, Trb::zero());
        }
        self.dequeue_index = 0;
        self.cycle = true;
    }

    /// Check if the next event TRB is ready (cycle bit matches our expected).
    pub fn event_ready(&self) -> bool {
        let trb = self.read_trb(self.dequeue_index);
        trb.cycle() == self.cycle
    }

    /// Dequeue the next event TRB (consumer side).
    pub fn dequeue_event(&mut self) -> Option<Trb> {
        let trb = self.read_trb(self.dequeue_index);
        if trb.cycle() != self.cycle {
            return None; // no new event
        }

        self.dequeue_index += 1;
        if self.dequeue_index >= self.size {
            self.dequeue_index = 0;
            self.cycle = !self.cycle;
        }

        Some(trb)
    }

    /// Physical address of the current dequeue pointer (for ERDP updates).
    pub fn dequeue_phys(&self) -> u64 {
        self.phys as u64 + (self.dequeue_index as u64) * (Trb::SIZE as u64)
    }

    /// Total DMA size needed for the ring.
    pub fn dma_size(num_trbs: usize) -> usize {
        num_trbs * Trb::SIZE
    }
}

// ============================================================================
// Event Ring Segment Table Entry (16 bytes)
// ============================================================================

/// Event Ring Segment Table Entry.
/// The table itself must be 64-byte aligned.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct EventRingSegmentTableEntry {
    /// Ring Segment Base Address (64-byte aligned).
    pub ring_base: u64,
    /// Ring Segment Size (number of TRBs in this segment).
    pub ring_size: u16,
    /// Reserved.
    pub _reserved1: u16,
    /// Reserved.
    pub _reserved2: u32,
}

impl EventRingSegmentTableEntry {
    pub const SIZE: usize = 16;

    pub fn new(ring_base: u64, ring_size: u16) -> Self {
        Self {
            ring_base,
            ring_size,
            _reserved1: 0,
            _reserved2: 0,
        }
    }
}

// ============================================================================
// Device Context Base Address Array (DCBAA)
// ============================================================================

/// DCBAA — array of 64-bit pointers to Device Contexts.
/// Entry 0 = Scratchpad Buffer Array pointer (or 0 if no scratchpad).
/// Entry 1..MaxSlots = Device Context pointers.
/// Must be 64-byte aligned. Size = (MaxSlots + 1) * 8 bytes.
pub const DCBAA_ENTRY_SIZE: usize = 8;

/// Compute required DCBAA size in bytes.
pub fn dcbaa_size(max_slots: u8) -> usize {
    (max_slots as usize + 1) * DCBAA_ENTRY_SIZE
}

// ============================================================================
// Device Context (32-byte or 64-byte entries per CSZ)
// ============================================================================

/// Slot Context — first entry in a Device Context (32 bytes for CSZ=0).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SlotContext {
    /// DW0: [19:0] Route String, [23:20] Speed, [24] Multi-TT, [25] Hub,
    ///       [31:27] Context Entries
    pub dw0: u32,
    /// DW1: [15:0] Max Exit Latency, [23:16] Root Hub Port Number,
    ///       [31:24] Number of Ports (if hub)
    pub dw1: u32,
    /// DW2: [31:22] TT Hub Slot ID, [21:16] TT Port Number,
    ///       [15:14] TT Think Time, [13:4] Interrupter Target
    pub dw2: u32,
    /// DW3: [7:0] USB Device Address, [31:27] Slot State
    pub dw3: u32,
    /// Reserved DW4-DW7.
    pub _reserved: [u32; 4],
}

impl SlotContext {
    pub fn zero() -> Self {
        Self {
            dw0: 0,
            dw1: 0,
            dw2: 0,
            dw3: 0,
            _reserved: [0; 4],
        }
    }

    /// Get slot state from DW3.
    pub fn slot_state(&self) -> u8 {
        ((self.dw3 >> 27) & 0x1F) as u8
    }

    /// Get USB device address from DW3.
    pub fn device_address(&self) -> u8 {
        (self.dw3 & 0xFF) as u8
    }
}

/// Slot state values.
pub const SLOT_STATE_DISABLED: u8 = 0;
pub const SLOT_STATE_DEFAULT: u8 = 1;
pub const SLOT_STATE_ADDRESSED: u8 = 2;
pub const SLOT_STATE_CONFIGURED: u8 = 3;

/// Endpoint Context — 32 bytes per endpoint (for CSZ=0).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct EndpointContext {
    /// DW0: [2:0] EP State, [9:8] Mult, [14:10] MaxPStreams,
    ///       [15] LSA, [23:16] Interval, [31:24] MaxESITPayloadHi
    pub dw0: u32,
    /// DW1: [2:1] Error Count, [5:3] EP Type, [7] HID,
    ///       [15:8] Max Burst Size, [31:16] Max Packet Size
    pub dw1: u32,
    /// DW2: [0] DCS, [63:4] TR Dequeue Pointer Lo
    pub tr_dequeue_lo: u32,
    /// DW3: TR Dequeue Pointer Hi
    pub tr_dequeue_hi: u32,
    /// DW4: [15:0] Average TRB Length, [31:16] Max ESIT Payload Lo
    pub dw4: u32,
    /// Reserved DW5-DW7.
    pub _reserved: [u32; 3],
}

impl EndpointContext {
    pub fn zero() -> Self {
        Self {
            dw0: 0,
            dw1: 0,
            tr_dequeue_lo: 0,
            tr_dequeue_hi: 0,
            dw4: 0,
            _reserved: [0; 3],
        }
    }

    /// Get endpoint state from DW0.
    pub fn ep_state(&self) -> u8 {
        (self.dw0 & 0x7) as u8
    }
}

/// Endpoint state values.
pub const EP_STATE_DISABLED: u8 = 0;
pub const EP_STATE_RUNNING: u8 = 1;
pub const EP_STATE_HALTED: u8 = 2;
pub const EP_STATE_STOPPED: u8 = 3;
pub const EP_STATE_ERROR: u8 = 4;

/// Endpoint type values (for DW1[5:3]).
pub const EP_TYPE_CONTROL: u8 = 4;
pub const EP_TYPE_ISOCH_OUT: u8 = 1;
pub const EP_TYPE_BULK_OUT: u8 = 2;
pub const EP_TYPE_INTERRUPT_OUT: u8 = 3;
pub const EP_TYPE_ISOCH_IN: u8 = 5;
pub const EP_TYPE_BULK_IN: u8 = 6;
pub const EP_TYPE_INTERRUPT_IN: u8 = 7;

/// A full Device Context: Slot Context + up to 31 Endpoint Contexts.
/// For CSZ=0 (32-byte contexts), total = 32 * 32 = 1024 bytes.
/// For CSZ=1 (64-byte contexts), total = 64 * 32 = 2048 bytes.
pub const DEVICE_CTX_SIZE_32: usize = 32 * 32; // 1024 bytes
pub const DEVICE_CTX_SIZE_64: usize = 64 * 32; // 2048 bytes

/// Input Context: Input Control Context + Slot Context + Endpoint Contexts.
/// Used for Address Device and Configure Endpoint commands.
/// Input Control Context is the first 32/64-byte entry.
pub const INPUT_CTX_SIZE_32: usize = 33 * 32; // 1056 bytes
pub const INPUT_CTX_SIZE_64: usize = 33 * 64; // 2112 bytes

/// Input Control Context (first entry of Input Context).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct InputControlContext {
    /// Drop Context Flags — bit N=1 means drop endpoint N.
    pub drop_flags: u32,
    /// Add Context Flags — bit N=1 means add endpoint N.
    /// Bit 0 = Slot Context, Bit 1 = EP0, etc.
    pub add_flags: u32,
    /// Reserved.
    pub _reserved: [u32; 5],
    /// Configuration Value, Interface Number, Alternate Setting.
    pub dw7: u32,
}

impl InputControlContext {
    pub fn zero() -> Self {
        Self {
            drop_flags: 0,
            add_flags: 0,
            _reserved: [0; 5],
            dw7: 0,
        }
    }
}

// ============================================================================
// Scratchpad Buffers
// ============================================================================

/// Scratchpad Buffer Array — array of 64-bit physical addresses.
/// One entry per scratchpad buffer. Each buffer is one page (PAGESIZE).
/// The array pointer goes into DCBAA[0].
pub const SCRATCHPAD_ENTRY_SIZE: usize = 8;
