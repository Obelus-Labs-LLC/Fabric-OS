# FABRIC OS — STABLE INTERFACE CONTRACT v1.0

## Wire Format Specification for Typed Message Bus and Capability Engine

**Owner:** Dshon Smith / Obelus Labs LLC
**Status:** ACTIVE — all implementations MUST conform to this contract
**Version:** 1.0 (Phase 1)
**Alignment:** All header structs are 64 bytes, cache-line aligned

---

## 1. DESIGN PRINCIPLES

1. **64-Byte Cache Lines**: Every core struct is exactly 64 bytes with `#[repr(C, align(64))]`
2. **40 + 24 Split**: 40 bytes active fields, 24 bytes reserved (including 8-byte extension pointer)
3. **Extension Pointer**: `extension_ptr` (u64) at offset 40 allows variable-length metadata without breaking the fixed header
4. **Reserved Fields**: 16 bytes at offsets 48-63 are zero-filled. Future phases carve fields from this space.
5. **Little-Endian**: All multi-byte fields are little-endian (native x86_64 byte order)
6. **HMAC Separation**: Cryptographic signatures are kernel-internal. Wire tokens do NOT carry HMACs.

---

## 2. CAPABILITY TOKEN (64 bytes)

Authorizes a process to perform operations on a kernel resource.

```
Offset  Size  Field            Type       Description
------  ----  ---------------  ---------  ------------------------------------------
 0       2    version          u16        Schema version (currently 1)
 2       2    permissions      u16        Bitflags: R|W|X|Grant|Revoke + 11 reserved
 4       4    owner            u32        ProcessId of token holder
 8       8    id               u64        Globally unique capability ID (0 = invalid)
16       8    resource         u64        ResourceId (upper 16 bits = kind)
24       8    delegated_from   u64        Parent capability ID (0 = root token)
32       4    nonce            u32        Replay prevention counter
36       4    expires          u32        Ticks until expiry (0 = never expires)
                                          ──── 40 bytes active ────
40       8    extension_ptr    u64        Pointer to extended metadata (0 = none)
48      16    _reserved        [u8; 16]   MUST be zero. Future: budget, agent tags
                                          ──── 24 bytes reserved ────
                                          ──── 64 bytes total ────
```

### Permission Bits (u16)

```
Bit  Name     Description
---  -------  -----------
 0   READ     Read access to resource
 1   WRITE    Write/modify access
 2   EXECUTE  Execute/invoke access
 3   GRANT    May delegate this capability to other processes
 4   REVOKE   May revoke child capabilities
5-15 (reserved — must be zero in v1)
```

### Resource ID Encoding (u64)

```
Bits 63-48: Resource Kind
  0x0001 = Memory region
  0x0002 = IPC channel
  0x0003 = Hardware device
  0x0004 = Filesystem object
  0x0005 = Network endpoint
  0x0006-0xFFFF = Reserved

Bits 47-0: Resource-specific identifier
```

### Kernel-Internal Fields (NOT in wire format)

These fields are stored in the kernel's CapabilityStore alongside each token:

- `hmac: [u8; 32]` — HMAC-SHA3-256 of the 40 active bytes
- `budget: Option<Budget>` — Rate limit configuration (max_uses, interval_ticks)
- `created_at: u64` — Monotonic tick when token was created

The HMAC key NEVER leaves Ring 0. Wire tokens are validated by ID lookup.

---

## 3. MESSAGE HEADER (64 bytes)

Fixed-size header for IPC messages on the Typed Message Bus.

```
Offset  Size  Field            Type       Description
------  ----  ---------------  ---------  ------------------------------------------
 0       2    version          u16        Schema version (currently 1)
 2       2    msg_type         u16        TypeId identifying payload schema
 4       4    sender           u32        Source ProcessId
 8       4    receiver         u32        Destination ProcessId
12       4    payload_len      u32        Byte length of payload (0 = signal msg)
16       8    capability_id    u64        Capability authorizing this message
24       8    sequence         u64        Monotonic per-sender (gap = attack)
32       8    timestamp        u64        Monotonic tick at send time
                                          ──── 40 bytes active ────
40       8    extension_ptr    u64        Pointer to payload + intent + msg HMAC
48      16    _reserved        [u8; 16]   MUST be zero. Future: intent, priority
                                          ──── 24 bytes reserved ────
                                          ──── 64 bytes total ────
```

### Extension Area (pointed to by extension_ptr)

When `extension_ptr != 0`, it points to a contiguous memory region containing:

```
Offset  Size          Field
------  -----------   -----
 0      payload_len   Payload data (typed by msg_type)
 N      32            HMAC-SHA3-256 of (header active bytes + payload)
 N+32   variable      Intent struct (Phase 3)
```

### Reserved Field Roadmap

The 16-byte `_reserved` area at offsets 48-63 will absorb these fields in future phases:

```
Phase 3: intent_category (u8), priority (u8), energy_class (u8) — 3 bytes
Phase 3: deadline (u32) — 4 bytes
Phase 9: agent_tag (u64) — 8 bytes for Estate agent routing
Remaining: 1 byte
```

---

## 4. SHARED IDENTITY TYPES

```
Type          Size  Description
-----------   ----  -----------
CapabilityId  u64   Capability token ID (0 = invalid/none)
ResourceId    u64   Kernel resource (upper 16 = kind, lower 48 = specific)
ProcessId     u32   Process identity (0 = kernel)
TypeId        u16   Message type identifier
Timestamp     u64   Monotonic tick counter
Perm          u16   Permission bitflags
```

---

## 5. VERSIONING RULES

1. The `version` field at offset 0 of every struct identifies the schema version
2. Version 1 is defined by this document
3. Future versions MUST maintain backward compatibility for the first 40 bytes
4. New fields are carved from the 24-byte reserved area
5. The `extension_ptr` mechanism allows unlimited extensibility without version bumps
6. Receivers MUST ignore unknown fields in the reserved area
7. Receivers MUST NOT reject messages with a higher version number than expected

---

## 6. IMPLEMENTATION

The authoritative Rust implementation lives in the `fabric_types` crate:

```
fabric_types/
  src/
    lib.rs          — Crate root, re-exports
    ids.rs          — CapabilityId, ResourceId, ProcessId, TypeId, Timestamp
    capability.rs   — CapabilityToken, Perm, Budget
    message.rs      — MessageHeader
```

All structs use `#[repr(C, align(64))]` and include compile-time size assertions:
```rust
const _: () = assert!(core::mem::size_of::<CapabilityToken>() == 64);
const _: () = assert!(core::mem::align_of::<CapabilityToken>() == 64);
```

---

*This document is the single source of truth for Fabric OS wire formats.*
*Last updated: 2026-03-01 — Version 1.0*
