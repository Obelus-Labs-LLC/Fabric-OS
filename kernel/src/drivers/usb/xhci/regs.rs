//! xHCI Register Definitions — Phase 21a.
//!
//! MMIO register layout per xHCI Specification 1.2.
//! Four register spaces: Capability, Operational, Runtime, Doorbell.
//! All offsets are relative to the base of each register space.

#![allow(dead_code)]

// ============================================================================
// Capability Registers (BAR0 + 0x00, read-only)
// ============================================================================

/// CAPLENGTH — Capability Register Length (1 byte).
/// Offset from BAR0 to the start of Operational Registers.
pub const CAP_CAPLENGTH: usize = 0x00;

/// HCIVERSION — Host Controller Interface Version (2 bytes at 0x02).
/// e.g., 0x0100 = xHCI 1.0, 0x0110 = xHCI 1.1, 0x0120 = xHCI 1.2.
pub const CAP_HCIVERSION: usize = 0x02;

/// HCSPARAMS1 — Structural Parameters 1 (4 bytes at 0x04).
///   [31:24] MaxPorts — max number of root hub ports
///   [17:8]  MaxIntrs — max number of interrupters
///   [7:0]   MaxSlots — max number of device slots
pub const CAP_HCSPARAMS1: usize = 0x04;

/// HCSPARAMS2 — Structural Parameters 2 (4 bytes at 0x08).
///   [31:27] MaxScratchpadBufsHi
///   [26:25] SPR (Scratchpad Restore)
///   [7:4]   MaxScratchpadBufsLo
///   [3:0]   IST (Isochronous Scheduling Threshold)
pub const CAP_HCSPARAMS2: usize = 0x08;

/// HCSPARAMS3 — Structural Parameters 3 (4 bytes at 0x0C).
///   [31:16] U2 Device Exit Latency
///   [7:0]   U1 Device Exit Latency
pub const CAP_HCSPARAMS3: usize = 0x0C;

/// HCCPARAMS1 — Capability Parameters 1 (4 bytes at 0x10).
///   [31:16] xECP — xHCI Extended Capabilities Pointer (DWORDs from BAR0)
///   [12]    CIC — Compliance Transition Capability
///   [11]    LEC — Large ESIT Payload Capability
///   [10]    LTC — Latency Tolerance Messaging Capability
///   [9]     NSS — No Secondary SID Support
///   [8]     PAE — Parse All Event Data
///   [7]     SPC — Short Packet Capability (stopped - short packet)
///   [6]     SEC — Stopped EDTLA Capability
///   [5]     FSC — Force Save Context Capability
///   [4]     MaxPSASize — Max Primary Stream Array Size
///   [3]     PPC — Port Power Control
///   [2]     CSZ — Context Size (0=32-byte, 1=64-byte contexts)
///   [1]     BNC — BW Negotiation Capability
///   [0]     AC64 — 64-bit Addressing Capability
pub const CAP_HCCPARAMS1: usize = 0x10;

/// DBOFF — Doorbell Offset Register (4 bytes at 0x14).
/// Offset from BAR0 to Doorbell Array. Must be DWORD-aligned.
pub const CAP_DBOFF: usize = 0x14;

/// RTSOFF — Runtime Register Space Offset (4 bytes at 0x18).
/// Offset from BAR0 to Runtime Registers. Must be 32-byte aligned.
pub const CAP_RTSOFF: usize = 0x18;

/// HCCPARAMS2 — Capability Parameters 2 (4 bytes at 0x1C). xHCI 1.1+.
pub const CAP_HCCPARAMS2: usize = 0x1C;

// ── Capability field extraction helpers ─────────────────────────────

/// Extract MaxSlots from HCSPARAMS1.
pub fn hcs1_max_slots(val: u32) -> u8 {
    (val & 0xFF) as u8
}

/// Extract MaxIntrs from HCSPARAMS1.
pub fn hcs1_max_intrs(val: u32) -> u16 {
    ((val >> 8) & 0x3FF) as u16
}

/// Extract MaxPorts from HCSPARAMS1.
pub fn hcs1_max_ports(val: u32) -> u8 {
    ((val >> 24) & 0xFF) as u8
}

/// Extract IST from HCSPARAMS2.
pub fn hcs2_ist(val: u32) -> u8 {
    (val & 0xF) as u8
}

/// Extract MaxScratchpadBufs from HCSPARAMS2 (hi:lo combined).
pub fn hcs2_max_scratchpad_bufs(val: u32) -> u16 {
    let hi = ((val >> 21) & 0x1F) as u16;
    let lo = ((val >> 27) & 0x1F) as u16;
    (hi << 5) | lo
}

/// Extract AC64 (64-bit addressing) from HCCPARAMS1.
pub fn hcc1_ac64(val: u32) -> bool {
    val & 1 != 0
}

/// Extract CSZ (context size: 0=32B, 1=64B) from HCCPARAMS1.
pub fn hcc1_csz(val: u32) -> bool {
    val & (1 << 2) != 0
}

/// Extract xECP pointer from HCCPARAMS1 (DWORD offset from BAR0).
pub fn hcc1_xecp(val: u32) -> u16 {
    ((val >> 16) & 0xFFFF) as u16
}

// ============================================================================
// Operational Registers (BAR0 + CAPLENGTH)
// ============================================================================
// All offsets below are relative to the Operational Register base.

/// USBCMD — USB Command Register (4 bytes at op_base + 0x00).
pub const OP_USBCMD: usize = 0x00;

/// USBSTS — USB Status Register (4 bytes at op_base + 0x04). Write-1-to-clear.
pub const OP_USBSTS: usize = 0x04;

/// PAGESIZE — Page Size Register (4 bytes at op_base + 0x08). Read-only.
/// Bits [15:0]: Supported page sizes as a bitmask. Bit 0 = 4KB.
pub const OP_PAGESIZE: usize = 0x08;

/// DNCTRL — Device Notification Control (4 bytes at op_base + 0x14).
pub const OP_DNCTRL: usize = 0x14;

/// CRCR — Command Ring Control Register (8 bytes at op_base + 0x18).
/// Low 32 bits: [5:4] CRR, CA, CS flags; [63:6] Command Ring Pointer.
pub const OP_CRCR_LO: usize = 0x18;
pub const OP_CRCR_HI: usize = 0x1C;

/// DCBAAP — Device Context Base Address Array Pointer (8 bytes at op_base + 0x30).
pub const OP_DCBAAP_LO: usize = 0x30;
pub const OP_DCBAAP_HI: usize = 0x34;

/// CONFIG — Configure Register (4 bytes at op_base + 0x38).
/// [7:0] MaxSlotsEn — Number of device slots enabled.
pub const OP_CONFIG: usize = 0x38;

// ── USBCMD bits ─────────────────────────────────────────────────────

/// Run/Stop — set to 1 to run the schedule.
pub const USBCMD_RS: u32 = 1 << 0;
/// Host Controller Reset.
pub const USBCMD_HCRST: u32 = 1 << 1;
/// Interrupter Enable.
pub const USBCMD_INTE: u32 = 1 << 2;
/// Host System Error Enable.
pub const USBCMD_HSEE: u32 = 1 << 3;
/// Light Host Controller Reset (xHCI 1.1+).
pub const USBCMD_LHCRST: u32 = 1 << 7;
/// Controller Save State.
pub const USBCMD_CSS: u32 = 1 << 8;
/// Controller Restore State.
pub const USBCMD_CRS: u32 = 1 << 9;
/// Enable Wrap Event.
pub const USBCMD_EWE: u32 = 1 << 10;

// ── USBSTS bits ─────────────────────────────────────────────────────

/// HC Halted — 1 when host controller is halted (RS=0 acknowledged).
pub const USBSTS_HCH: u32 = 1 << 0;
/// Host System Error — fatal error condition.
pub const USBSTS_HSE: u32 = 1 << 2;
/// Event Interrupt — event ring has pending events.
pub const USBSTS_EINT: u32 = 1 << 3;
/// Port Change Detect — port status changed.
pub const USBSTS_PCD: u32 = 1 << 4;
/// Save State Status.
pub const USBSTS_SSS: u32 = 1 << 8;
/// Restore State Status.
pub const USBSTS_RSS: u32 = 1 << 9;
/// Save/Restore Error.
pub const USBSTS_SRE: u32 = 1 << 10;
/// Controller Not Ready — 1 while controller is initializing.
pub const USBSTS_CNR: u32 = 1 << 11;
/// Host Controller Error — internal error.
pub const USBSTS_HCE: u32 = 1 << 12;

// ── CRCR bits ───────────────────────────────────────────────────────

/// Ring Cycle State.
pub const CRCR_RCS: u64 = 1 << 0;
/// Command Stop.
pub const CRCR_CS: u64 = 1 << 1;
/// Command Abort.
pub const CRCR_CA: u64 = 1 << 2;
/// Command Ring Running.
pub const CRCR_CRR: u64 = 1 << 3;
/// Pointer mask (bits [63:6]).
pub const CRCR_PTR_MASK: u64 = !0x3F;

// ============================================================================
// Port Register Set (op_base + 0x400 + port_index * 0x10)
// ============================================================================

/// Port Status and Control Register offset (within port register set).
pub const PORT_PORTSC: usize = 0x00;
/// Port PM Status and Control.
pub const PORT_PORTPMSC: usize = 0x04;
/// Port Link Info.
pub const PORT_PORTLI: usize = 0x08;
/// Port Hardware LPM Control.
pub const PORT_PORTHLPMC: usize = 0x0C;

/// Base offset of Port Register Set array from Operational Register base.
pub const PORT_REGISTER_BASE: usize = 0x400;
/// Size of each Port Register Set.
pub const PORT_REGISTER_SIZE: usize = 0x10;

/// Compute offset of PORTSC for port `n` (0-indexed) from op_base.
pub fn portsc_offset(port_index: u8) -> usize {
    PORT_REGISTER_BASE + (port_index as usize) * PORT_REGISTER_SIZE + PORT_PORTSC
}

// ── PORTSC bits ─────────────────────────────────────────────────────

/// Current Connect Status — 1 if device is connected.
pub const PORTSC_CCS: u32 = 1 << 0;
/// Port Enabled/Disabled.
pub const PORTSC_PED: u32 = 1 << 1;
/// Over-current Active.
pub const PORTSC_OCA: u32 = 1 << 3;
/// Port Reset.
pub const PORTSC_PR: u32 = 1 << 4;
/// Port Link State [8:5] — see PortLinkState enum.
pub const PORTSC_PLS_MASK: u32 = 0xF << 5;
pub const PORTSC_PLS_SHIFT: u32 = 5;
/// Port Power — 1 if port is powered.
pub const PORTSC_PP: u32 = 1 << 9;
/// Port Speed [13:10].
pub const PORTSC_SPEED_MASK: u32 = 0xF << 10;
pub const PORTSC_SPEED_SHIFT: u32 = 10;
/// Port Link State Write Strobe.
pub const PORTSC_LWS: u32 = 1 << 16;
/// Connect Status Change (W1C).
pub const PORTSC_CSC: u32 = 1 << 17;
/// Port Enabled/Disabled Change (W1C).
pub const PORTSC_PEC: u32 = 1 << 18;
/// Warm Port Reset Change (W1C).
pub const PORTSC_WRC: u32 = 1 << 19;
/// Over-current Change (W1C).
pub const PORTSC_OCC: u32 = 1 << 20;
/// Port Reset Change (W1C).
pub const PORTSC_PRC: u32 = 1 << 21;
/// Port Link State Change (W1C).
pub const PORTSC_PLC: u32 = 1 << 22;
/// Port Config Error Change (W1C).
pub const PORTSC_CEC: u32 = 1 << 23;
/// Wake on Connect Enable.
pub const PORTSC_WCE: u32 = 1 << 25;
/// Wake on Disconnect Enable.
pub const PORTSC_WDE: u32 = 1 << 26;
/// Wake on Over-current Enable.
pub const PORTSC_WOE: u32 = 1 << 27;
/// Device Removable.
pub const PORTSC_DR: u32 = 1 << 30;
/// Warm Port Reset (USB3 only).
pub const PORTSC_WPR: u32 = 1 << 31;

/// All W1C (write-1-to-clear) change bits in PORTSC.
/// Must be preserved when writing other PORTSC fields.
pub const PORTSC_CHANGE_BITS: u32 =
    PORTSC_CSC | PORTSC_PEC | PORTSC_WRC | PORTSC_OCC | PORTSC_PRC | PORTSC_PLC | PORTSC_CEC;

/// Extract port speed from PORTSC value.
pub fn portsc_speed(val: u32) -> u8 {
    ((val & PORTSC_SPEED_MASK) >> PORTSC_SPEED_SHIFT) as u8
}

/// Extract port link state from PORTSC value.
pub fn portsc_pls(val: u32) -> u8 {
    ((val & PORTSC_PLS_MASK) >> PORTSC_PLS_SHIFT) as u8
}

/// Port speed values.
pub const SPEED_FULL: u8 = 1;  // 12 Mbps
pub const SPEED_LOW: u8 = 2;   // 1.5 Mbps
pub const SPEED_HIGH: u8 = 3;  // 480 Mbps
pub const SPEED_SUPER: u8 = 4; // 5 Gbps
pub const SPEED_SUPER_PLUS: u8 = 5; // 10 Gbps

/// Port Link State values (PLS field).
pub const PLS_U0: u8 = 0;
pub const PLS_U1: u8 = 1;
pub const PLS_U2: u8 = 2;
pub const PLS_U3: u8 = 3;       // Suspended
pub const PLS_DISABLED: u8 = 4;
pub const PLS_RX_DETECT: u8 = 5;
pub const PLS_INACTIVE: u8 = 6;
pub const PLS_POLLING: u8 = 7;
pub const PLS_RECOVERY: u8 = 8;
pub const PLS_HOT_RESET: u8 = 9;
pub const PLS_COMPLIANCE: u8 = 10;
pub const PLS_TEST_MODE: u8 = 11;
pub const PLS_RESUME: u8 = 15;

// ============================================================================
// Runtime Registers (BAR0 + RTSOFF)
// ============================================================================

/// MFINDEX — Microframe Index Register (4 bytes at rts_base + 0x00).
pub const RTS_MFINDEX: usize = 0x00;

/// Interrupter Register Set base offset from Runtime Register base.
/// Interrupter N at: rts_base + 0x20 + (N * 0x20).
pub const RTS_IR_BASE: usize = 0x20;
/// Size of each Interrupter Register Set.
pub const RTS_IR_SIZE: usize = 0x20;

// ── Interrupter Register Set offsets (within each interrupter) ──────

/// Interrupter Management Register (4 bytes).
///   [31:1] Interrupt Pending (IP) — write 1 to clear.
///   [0]    Interrupt Enable (IE).
pub const IR_IMAN: usize = 0x00;

/// Interrupter Moderation Register (4 bytes).
///   [31:16] Interrupt Moderation Counter
///   [15:0]  Interrupt Moderation Interval (in 250ns units)
pub const IR_IMOD: usize = 0x04;

/// Event Ring Segment Table Size (4 bytes).
///   [15:0] Table Size (number of segments, max 256).
pub const IR_ERSTSZ: usize = 0x08;

// Reserved at 0x0C

/// Event Ring Segment Table Base Address (8 bytes at 0x10).
/// Must be 64-byte aligned.
pub const IR_ERSTBA_LO: usize = 0x10;
pub const IR_ERSTBA_HI: usize = 0x14;

/// Event Ring Dequeue Pointer (8 bytes at 0x18).
///   [63:4] Dequeue Pointer
///   [3]    Event Handler Busy (EHB) — write 1 to clear
///   [2:0]  Dequeue ERST Segment Index
pub const IR_ERDP_LO: usize = 0x18;
pub const IR_ERDP_HI: usize = 0x1C;

/// IMAN bits.
pub const IMAN_IP: u32 = 1 << 0;
pub const IMAN_IE: u32 = 1 << 1;

/// ERDP Event Handler Busy bit.
pub const ERDP_EHB: u64 = 1 << 3;

/// Compute offset of Interrupter N register from Runtime Register base.
pub fn interrupter_offset(n: u16) -> usize {
    RTS_IR_BASE + (n as usize) * RTS_IR_SIZE
}

// ============================================================================
// Doorbell Registers (BAR0 + DBOFF)
// ============================================================================

/// Each doorbell is a 4-byte register. Doorbell 0 = host controller.
/// Doorbell N (1-MaxSlots) = device slot N.
///   [31:16] DB Stream ID
///   [15:8]  Reserved
///   [7:0]   DB Target (endpoint index)
pub const DB_SIZE: usize = 4;

/// Doorbell target for Host Controller Command Ring.
pub const DB_TARGET_HC_COMMAND: u8 = 0;

/// Compute offset of doorbell for slot N from Doorbell Array base.
pub fn doorbell_offset(slot: u8) -> usize {
    (slot as usize) * DB_SIZE
}

/// Build a doorbell value.
pub fn doorbell_value(target: u8, stream_id: u16) -> u32 {
    (target as u32) | ((stream_id as u32) << 16)
}

// ============================================================================
// PCI Class/Subclass/ProgIf for xHCI
// ============================================================================

/// PCI class code for Serial Bus Controller.
pub const PCI_CLASS_SERIAL_BUS: u8 = 0x0C;
/// PCI subclass for USB Controller.
pub const PCI_SUBCLASS_USB: u8 = 0x03;
/// PCI programming interface for xHCI (USB 3.0).
pub const PCI_PROGIF_XHCI: u8 = 0x30;

/// Check if a PCI device is an xHCI controller.
pub fn is_xhci(class: u8, subclass: u8, prog_if: u8) -> bool {
    class == PCI_CLASS_SERIAL_BUS && subclass == PCI_SUBCLASS_USB && prog_if == PCI_PROGIF_XHCI
}
