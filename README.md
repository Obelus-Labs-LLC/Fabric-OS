# FABRIC OS — THE ESTATE
## AI-Coordinated Microkernel Fabric
### Master Context Document v4.0

**Owner:** Dshon Smith / Obelus Labs LLC
**Brand:** "Welcome to The Estate, Powered by Fabric OS"
**Target:** Developers, power users, the future
**License:** GPL kernel (free forever), proprietary premium agents (Android model)
**Strategic Goal:** Daily-driver OS with AI-native browser. User sovereignty, local-first, intent-driven.
**Pitch:** ChromeOS security with macOS polish, AI-native from day one.

| | |
|:---|:---|
| **Current State** | 17 kernel phases complete (0-16), all SRI 100/100. TLS 1.3 + Window Manager complete. |
| **Kernel LOC** | ~30K lines Rust, zero warnings, boots clean |
| **Key Milestone** | Phase 16: Window Manager with overlapping windows, z-ordering, taskbar, Alt+Tab. Desktop environment ready. |
| **Timeline** | Completion-based, not calendar. |

---

## 1. CORE IDENTITY

Fabric OS is a ground-up operating system built on an AI-coordinated microkernel architecture. It is NOT Linux-based. The kernel is ~25K lines of Rust today (Phase 14 complete), targeting ~50K at full completion. Every subsystem communicates through a single typed message bus. Security is capability-based (no root, no superuser, no ACLs). AI prediction runs in the kernel but is fully detachable — the OS works without it.

The Estate is a modular AI agent ecosystem that runs on top of Fabric OS — 11 specialized agents (Butler, Maid, Groundskeeper, Chauffeur, Concierge, Archivist, Oracle, Alchemist, Therapist, Quartermaster, Curator) providing everything from system supervision to financial monitoring to personal wellness.

### Four v0.1 Minimum Differentiators
What makes Fabric meaningfully different from Linux + AppArmor:
1. **Capability-only security** — no root, no superuser. Every action requires an unforgeable token
2. **Bus-mediated IPC** — one door. All communication goes through a single typed message bus
3. **Immutable external constitution** — AI safety rules that agents cannot forget or mutate
4. **Hash-chained audit log** — append-only, SHA-3-256, tamper-detectable

---

## 2. MICROKERNEL SPECIFICATION

### Ring 0 — Only 5 Subsystems (~12K lines Rust, Phase 5B)

```
┌─────────────────────────────────────────────────────┐
│                   RING 0 (Kernel)                    │
│                                                      │
│  ┌──────────┐ ┌──────────┐ ┌────────────────────┐   │
│  │ Memory   │ │ IPC Bus  │ │ Capability Manager │   │
│  │ Manager  │ │ Router   │ │                    │   │
│  └──────────┘ └──────────┘ └────────────────────┘   │
│  ┌──────────┐ ┌──────────┐                          │
│  │Scheduler │ │ Interrupt│                          │
│  │          │ │ Dispatch │                          │
│  └──────────┘ └──────────┘                          │
└─────────────────────────────────────────────────────┘
```

1. **Memory Manager** — physical frame allocator, virtual memory mapping, zero-copy page sharing between processes
2. **IPC Bus Router** — typed message routing, zero-copy transfers, capability validation on every message, message sequence numbers + HMAC signing
3. **Capability Manager** — token generation/validation/revocation, hierarchical delegation, budgeted capabilities (rate limits enforced at router level), nonce-based replay prevention
4. **Scheduler** — intent-aware scheduling (processes declare goals), priority inheritance, deadline-aware, energy-aware hints
5. **Interrupt Dispatch** — hardware interrupt routing to userspace drivers via IPC

### What Runs in Userspace (Everything Else)
- Drivers (all of them — disk, network, GPU, USB, etc.)
- File system (FabricFS)
- Network stack
- AI prediction layer
- All Estate agents
- Council governance
- Shell (FabricScript)

---

## 3. CORE DATA STRUCTURES

### CapabilityToken
```rust
struct CapabilityToken {
    id: u128,                    // Globally unique
    resource: ResourceId,        // What this grants access to
    permissions: BitFlags<Perm>, // Read, Write, Execute, Grant, Revoke
    owner: ProcessId,            // Who holds this
    delegated_from: Option<u128>,// Parent capability (delegation chain)
    expires: Option<Timestamp>,  // TTL
    budget: Option<Budget>,      // Rate limit: max N uses per interval
    nonce: u64,                  // Replay prevention
    hmac: [u8; 32],              // Integrity verification
}
```

### Message (IPC)
```rust
struct Message {
    sender: ProcessId,
    receiver: ProcessId,
    capability: CapabilityToken,  // Must be valid or message is dropped
    msg_type: TypeId,             // Strongly typed — no raw bytes
    payload: TypedPayload,        // Zero-copy when possible
    sequence: u64,                // Monotonic per-sender, gap = attack
    intent: Intent,               // Why this message exists
    hmac: [u8; 32],               // Signed by sender's capability
}
```

### Process
```rust
struct Process {
    pid: ProcessId,
    capabilities: Vec<CapabilityToken>,  // Everything this process can do
    intent: Intent,                       // Declared goal
    behavioral_profile: BehavioralProfile,// Runtime pattern tracking
    supervisor: ProcessId,                // Erlang-style supervision tree
    children: Vec<ProcessId>,
    state: ProcessState,                  // Running, Blocked, Suspended, Terminated
    energy_class: EnergyClass,            // Battery, Balanced, Performance
}
```

### Intent
```rust
struct Intent {
    category: IntentCategory,  // Compute, IO, Network, Storage, Display, AI
    priority: Priority,        // Critical, High, Normal, Low, Background
    deadline: Option<Timestamp>,
    description: String,       // Human-readable: "Downloading firmware update"
}
```

### BehavioralProfile
```rust
struct BehavioralProfile {
    avg_cpu_per_burst: f32,
    avg_memory_allocated: u64,
    avg_messages_per_second: f32,
    capability_usage_pattern: Vec<(ResourceId, f32)>,  // resource → frequency
    anomaly_score: f32,         // 0.0 = normal, 1.0 = definitely compromised
    last_updated: Timestamp,
}
```

### AuditEntry (Hash-Chained)
```rust
struct AuditEntry {
    sequence: u64,              // Monotonic
    timestamp: Timestamp,
    actor: ProcessId,
    action: AuditAction,       // CapGranted, CapRevoked, MessageSent, PolicyViolation, etc.
    target: ResourceId,
    capability_used: u128,
    prev_hash: [u8; 32],       // SHA-3-256 of previous entry
    hash: [u8; 32],            // SHA-3-256 of this entry (includes prev_hash)
}
```

### PredictionHint (AI → Scheduler)
```rust
struct PredictionHint {
    source: PredictionModel,   // MemoryLSTM, IoLSTM, ContentionGNN, AnomalyAutoencoder
    target: ProcessId,
    prediction: PredictionType,// WillNeedMemory(bytes), WillBlock(duration), Anomalous(score)
    confidence: f32,           // 0.0–1.0
    timestamp: Timestamp,
}
```

---

## 4. SYSTEM ARCHITECTURE DIAGRAM

```
╔══════════════════════════════════════════════════════════════════╗
║                        THE ESTATE                                ║
║                                                                  ║
║  ┌─────────┐ ┌─────────┐ ┌──────────┐ ┌──────────┐ ┌────────┐  ║
║  │ Oracle  │ │Alchemist│ │Therapist │ │Quartermst│ │Curator │  ║
║  │(Premium)│ │(Premium)│ │(Premium) │ │(Premium) │ │(Premium│  ║
║  └────┬────┘ └────┬────┘ └────┬─────┘ └────┬─────┘ └───┬────┘  ║
║       │           │           │             │           │        ║
║  ┌────┴────┐ ┌────┴────┐ ┌───┴───┐ ┌──────┴──────┐            ║
║  │ Butler  │ │  Maid   │ │Concge │ │  Archivist  │            ║
║  │(Superv.)│ │(Maint.) │ │(Comms)│ │  (FabricFS) │            ║
║  └────┬────┘ └────┬────┘ └───┬───┘ └──────┬──────┘            ║
║       │           │           │             │                    ║
╠═══════╪═══════════╪═══════════╪═════════════╪════════════════════╣
║       │     TYPED MESSAGE BUS (IPC)         │                    ║
║  ═════╪═══════════╪═══════════╪═════════════╪══════════════════  ║
╠═══════╪═══════════╪═══════════╪═════════════╪════════════════════╣
║       │           │           │             │                    ║
║  ┌────┴────┐ ┌────┴────┐ ┌───┴───────┐ ┌──┴──────────┐        ║
║  │ Ground- │ │Chauffeur│ │ Council   │ │ AI Predict  │        ║
║  │ keeper  │ │(Network)│ │(Governanc)│ │ Layer       │        ║
║  └────┬────┘ └────┬────┘ └───┬───────┘ └──┬──────────┘        ║
║       │           │           │             │                    ║
╠═══════╪═══════════╪═══════════╪═════════════╪════════════════════╣
║       │           │           │             │                    ║
║  ┌────┴────────────┴───────────┴─────────────┴──────────────┐    ║
║  │              MICROKERNEL (Ring 0, ~20K LOC Rust)         │    ║
║  │  Memory │ IPC Bus │ CapManager │ Scheduler │ Interrupts  │    ║
║  └──────────────────────────────────────────────────────────┘    ║
║                                                                  ║
║  ┌──────────────────────────────────────────────────────────┐    ║
║  │                    HARDWARE (HAL)                         │    ║
║  │   x86_64 │ ARM64 │ RISC-V │ GPU │ TPM │ Sensors         │    ║
║  └──────────────────────────────────────────────────────────┘    ║
╚══════════════════════════════════════════════════════════════════╝
```

---

## 5. BUS ARCHITECTURE

All communication flows through the typed message bus. No exceptions.

### Message Flow
```
Process A → [Message + Capability] → Bus Router → Validate Capability → Route → Process B
                                        │
                                  [Audit Entry]
                                        │
                                  [Hash Chain]
```

### Bus Security
- Every message requires a valid CapabilityToken
- Bus router validates capability before routing (confused deputy protection)
- Every message gets an AuditEntry in the hash-chained log
- Message sequence numbers are monotonic per-sender — gap = potential replay attack
- HMAC signing prevents message tampering in transit
- Separate monitor taps (read-only) for observability — monitor cannot inject messages
- Budgeted capabilities enforce rate limits at the router level

---

## 6. DATA FLOW EXAMPLES

### Normal Flow: App Reads a File
```
1. App sends Message{type: FileRead, path: "notes.txt"} with CapabilityToken{resource: FabricFS, perm: Read}
2. Bus Router validates capability → valid, routes to FabricFS service
3. FabricFS resolves semantic path → retrieves file → sends response
4. Bus Router routes response back to App
5. AuditEntry logged: App read notes.txt at timestamp T, cap C, hash H
```

### Attack Flow: Compromised Process Attempts Escalation
```
1. Compromised process sends Message{type: CapGrant} attempting to self-grant Write to kernel memory
2. Bus Router validates capability → process has no Grant permission for kernel memory → REJECTED
3. AuditEntry logged: VIOLATION — unauthorized CapGrant attempt
4. Anomaly detector updates BehavioralProfile → anomaly_score spikes
5. If score > threshold → Council notified → potential isolation/termination
```

### ChatGPT's 5-Step Attack Chain Defense Trace
```
Step 1: Attacker compromises a low-privilege userspace service
  → Defense: Capability-only. Compromised service has ONLY its assigned tokens. No root to escalate to.

Step 2: Attacker tries to read bus traffic from other services
  → Defense: Bus monitor taps are read-only and separate from the data plane. Monitor cannot inject. Attacker's service can only see its own messages.

Step 3: Attacker floods the bus with messages to cause DoS
  → Defense: Budgeted capabilities. Bus router enforces rate limits per-capability. Flood detected → cap budget exhausted → messages dropped. Escalation flood defense: 3+ denied requests in 60s → auto-quarantine.

Step 4: Attacker attempts to poison the Council's AI model
  → Defense: Per-agent training caps (max N gradient updates per day). Weight hash verification before every inference. Drift detection (cosine similarity to golden snapshot). Override decay (AI overrides expire, decay back to deterministic rules).

Step 5: Attacker tries to modify the constitution to allow their behavior
  → Defense: Constitution is immutable external YAML, signed with CRYSTALS-Dilithium, anchored to TPM. 24-hour cooling period for any amendment. Chaos-state constitution lock (amendments blocked during elevated/chaos states). 4-hour auto-revert emergency override path.
```

---

## 7. GLOBAL SAFETY STATE MACHINE

Five states, strict priority ordering: **Lockdown > Chaos > Safe > Elevated > Normal**

```
┌──────────┐   anomaly burst   ┌──────────┐   3+ alarms   ┌──────────┐
│  NORMAL  │ ────────────────→ │ ELEVATED │ ───────────→  │  CHAOS   │
│          │ ←──────────────── │          │ ←───────────── │          │
└──────────┘   clear for 5min  └──────────┘   resolved     └──────────┘
                                                               │
                                              manual trigger   │
                                                               ▼
                                                          ┌──────────┐
                                                          │ LOCKDOWN │
                                                          │(human req│
                                                          │ to exit) │
                                                          └──────────┘
                                                               │
                                              admin confirms   │
                                                               ▼
                                                          ┌──────────┐
                                                          │   SAFE   │
                                                          │(burns to │
                                                          │ Normal)  │
                                                          └──────────┘
```

### State Behaviors
- **Normal:** Full AI prediction, full agent autonomy, full learning
- **Elevated:** AI prediction active, learning paused, Council Tier 3 activated, constitution amendments blocked
- **Chaos:** AI detached, deterministic rules only, all learning frozen, all non-essential agents suspended, constitution locked
- **Lockdown:** Everything stopped. Human required to confirm exit. Nuclear option.
- **Safe:** Post-lockdown. Burns down to Normal over 30 minutes. Gradual re-enablement.

---

## 8. THREE-LAYER GOVERNANCE STACK

```
Layer 3 (Human): ACS — Authority Continuity Specification
  └── Human override governance. Dead-man switches. Succession protocols.
      Lifecycle: ACTIVE → DEGRADED (missed heartbeat) → CONTINGENCY (alternate) → EMERGENCY (lockdown)

Layer 2 (AI): Council — Local-First Adaptive Governance
  └── Three-tier decision engine. Governs agent behavior, resource allocation, conflict resolution.

Layer 1 (Software): Anomaly Detector — Behavioral Analysis
  └── Four tiny ML models. Detects deviations from established behavioral profiles.
```

### Three-Tier Council (Local-First, No External APIs)

**Tier 1 — Deterministic Rules (0ms latency)**
- YAML/TOML ruleset
- Pattern matching: IF condition THEN action
- Handles 80%+ of decisions
- Cannot be overridden by AI
- Examples: "Process requesting >2GB memory without prior allocation pattern → deny", "Network access from process with no network capability → block"

**Tier 2 — Local Small Model (~3B parameters, 10-50ms)**
- Runs on CPU or iGPU
- Fine-tuned on system decisions
- Handles ambiguous cases Tier 1 can't resolve
- Single model, fast inference
- LoRA fine-tuning for adaptation (max N gradient updates/day)
- Examples: "Is this memory allocation pattern legitimate for a video editor?", "Should this new network connection be allowed given recent behavior?"

**Tier 3 — Local Panel (3 models, 100ms-2s)**
- Three diverse local models deliberate
- Majority vote required
- Used for high-stakes decisions only
- GPU temporal isolation: pause non-system GPU workloads during Tier 3
- VRAM zeroed on model unload (prevent remanence attacks)
- Weight hash verification before every inference
- Examples: "Should we revoke all capabilities from this process?", "Is this a coordinated attack across multiple services?"

**Optional Tier 4 — External Override (network required, human-approved)**
- External LLM consultation only with explicit human pre-authorization
- Used for novel situations the local Council hasn't seen
- Results cached locally for future Tier 2/3 training
- Never auto-enabled

### Council Learning Defenses
- **Golden decisions regression suite:** Curated set of known-correct decisions. Tested after every model update. Any regression → rollback.
- **Drift detection:** Cosine similarity between current model weights and golden snapshot. Drift beyond threshold → freeze + alert.
- **Per-agent training caps:** Max N gradient updates per agent per day. Prevents training data poisoning via volume.
- **Override decay:** AI overrides of deterministic rules automatically expire. Decay back to Tier 1 rules unless re-confirmed.
- **Weight promotion transparency:** Local model updates are logged in audit chain. Before/after hashes recorded.

### ACS — Authority Continuity Specification

Governs HUMAN overrides. Completes the three-layer stack (software → AI → human).

**Lifecycle States:**
```
ACTIVE → DEGRADED → CONTINGENCY → EMERGENCY
  │         │            │             │
  │    missed beat   alt authority   lockdown
  │         │            │             │
  └─────────┴────────────┴─────────────┘
              heartbeat restored
```

- Dead-man switch: Primary authority must check in periodically
- Succession chain: If primary goes dark, pre-designated alternates activate
- Emergency lockdown: If all authorities unreachable, system enters Lockdown state
- Constitutional amendments require ACS ACTIVE state + 24-hour cooling period

---

## 9. AI PREDICTION LAYER

Four tiny models, all running in userspace, fully detachable:

1. **Memory LSTM** — predicts which pages a process will need next. Feeds prefetch hints to Memory Manager.
2. **IO LSTM** — predicts upcoming disk/network IO patterns. Feeds scheduling hints.
3. **Contention GNN** — models process interaction graph. Predicts resource contention before it happens.
4. **Anomaly Autoencoder** — learns normal behavioral profiles. Flags deviations. Feeds anomaly scores to Council.

### Detachment Guarantee
If all four models are disabled (Chaos state, hardware failure, user preference):
- Scheduler falls back to priority-based round-robin
- Memory manager uses reactive allocation only
- IO scheduling uses FIFO
- Anomaly detection relies on deterministic Tier 1 rules only
- **The OS continues to function.** AI is optimization, not dependency.

---

## 10. HARDWARE ABSTRACTION LAYER (HAL)

### Target Architectures
- **Primary:** x86_64 (development + desktop)
- **Secondary:** ARM64 (mobile + embedded)
- **Tertiary:** RISC-V (future-proofing)

### HAL Contract
Every hardware driver:
- Runs in userspace (not kernel)
- Communicates only via message bus
- Requires capabilities for hardware access
- Is supervised by Butler (crash → restart via supervision tree)

### Hardware Requirements
- TPM 2.0 (constitution anchoring, secure boot chain)
- IOMMU (DMA isolation for userspace drivers)
- CPU with ring separation (Ring 0/3 minimum)
- GPU optional (enhances Tier 2/3 Council, AI prediction)

---

## 11. ENERGY-AWARE SUBSYSTEM

### Energy Classes
- **Battery:** Aggressive power saving. AI prediction scaled down. Non-essential agents hibernated.
- **Balanced:** Default. Full AI prediction. All agents active.
- **Performance:** Maximum throughput. GPU fully engaged. All Tiers active.

### Scheduler Integration
- Processes declare energy class in Intent
- Scheduler coalesces low-priority work to minimize wake-ups
- AI prediction layer hints when to batch IO operations
- Groundskeeper (agent) monitors thermal + battery state, recommends transitions

---

## 12. FABRICSCRIPT — SHELL LANGUAGE

Single shell language for Fabric OS. Not bash. Not PowerShell.

### Key Features
- **Typed pipelines:** Data flows through pipes with type information. `files | where size > 1MB | sort by modified` — each stage knows the schema.
- **Intent-aware:** Commands declare intent. `fetch url --intent=background` tells scheduler to deprioritize.
- **Capability-scoped:** Shell session has capabilities. `grant network to curl` gives curl network access for one invocation.
- **Pattern matching:** Built-in match expressions. `match file.ext { "rs" => compile, "md" => render, _ => open }`
- **Bus integration:** Shell commands are bus messages. `send storage.read {path: "notes.txt"}` directly invokes FabricFS.

### Example Session
```fabricscript
# List files semantically
files tagged "work" modified this-week
  | sort by relevance
  | take 10

# Grant temporary network access and fetch
grant network to fetch for 30s
fetch "https://api.example.com/data" --intent=background
  | parse json
  | where .status == "active"
  | save as "active_items" tagged "api-data"

# System introspection
processes | where anomaly_score > 0.5 | inspect capabilities
```

---

## 13. FABRICFS — SEMANTIC FILE SYSTEM

Not a traditional path-based filesystem. Files have:
- **Tags:** Arbitrary metadata (`work`, `draft`, `2026`, `project:fabric`)
- **Relations:** Files can link to other files (`references`, `derived-from`, `supersedes`)
- **Versions:** Every modification creates a new version. Full history.
- **Queries:** Find files by semantic query, not path. `files tagged "work" and "draft" modified after 2026-01-01`
- **Compatibility:** POSIX translation layer for legacy apps. `/fabricfs/compat/` presents traditional path view.

### Storage Backend
- Content-addressable storage (hash-based deduplication)
- Metadata stored separately from content (fast queries)
- Encryption at rest (AES-256-GCM, per-file keys)

---

## 14. NETWORK STACK + AETHER

### Network Stack
- Runs entirely in userspace
- TCP/IP, UDP, DNS, TLS 1.3, QUIC
- Capability-required for all network access
- Per-process network policies

### Aether Integration
Aether becomes the Chauffeur's network policy brain:
- Multi-uplink coordination (WiFi + Cellular + Ethernet)
- Bandwidth allocation per-process based on Intent priority
- Network policy enforcement (which processes can access which domains)
- Failover logic (uplink dies → seamless switch)
- QoS classification mapped to Intent categories

---

## 15. MULTI-AGENT ORCHESTRATION

### Supervision Trees (Erlang-Inspired)
```
Butler (Root Supervisor)
├── Maid (System Maintenance)
│   ├── GC Worker
│   ├── Log Rotator
│   └── Temp Cleaner
├── Groundskeeper (AI + Sensors)
│   ├── Memory LSTM
│   ├── IO LSTM
│   ├── Contention GNN
│   └── Anomaly Autoencoder
├── Chauffeur (Network)
│   ├── TCP Stack
│   ├── DNS Resolver
│   └── Aether Policy Engine
├── Concierge (User Communication)
│   ├── Notification Service
│   └── Intent Parser
├── Archivist (Storage)
│   ├── FabricFS Core
│   ├── Metadata Index
│   └── Version Manager
└── Council (Governance)
    ├── Tier 1 Rules Engine
    ├── Tier 2 Local Model
    └── Tier 3 Panel Coordinator
```

### Supervision Rules
- **one-for-one:** If child crashes, restart only that child
- **one-for-all:** If child crashes, restart all children in group
- **rest-for-one:** If child crashes, restart it and all children started after it
- Max restart intensity: N restarts in T seconds. Exceeded → escalate to parent supervisor.

---

## 16. FORMAL VERIFICATION STRATEGY

### Verified Components (Coq / Lean 4)
- Capability token generation and validation
- IPC message routing correctness
- Capability delegation chain integrity

### Model Checked Components (TLA+)
- Memory manager (no double-free, no use-after-free)
- Scheduler (no starvation, no priority inversion)
- State machine transitions (Global Safety State Machine)

### Fuzz Tested
- Bus router (malformed messages)
- Capability manager (forged tokens)
- FabricFS (corrupted metadata)

---

## 17. CRYPTOGRAPHY

All post-quantum ready:
- **Key exchange:** CRYSTALS-Kyber (ML-KEM)
- **Digital signatures:** CRYSTALS-Dilithium (ML-DSA)
- **Symmetric encryption:** AES-256-GCM
- **Hashing:** SHA-3-256
- **Constitution signing:** Dilithium + TPM anchoring
- **Audit chain:** SHA-3-256 hash chain
- **Capability HMAC:** HMAC-SHA-3-256

---

## 18. STRESS — SYSTEM THREAT RESILIENCE & EXTREME STRESS SUITE

STRESS is a reliability benchmarking framework designed to evaluate how computational workloads behave when foundational operating assumptions are violated by environmental and systemic constraints. Unlike terrestrial benchmarks—which typically assume continuous power, stable connectivity, and rare environmental disruption—STRESS focuses on resilience and behavioral stability under persistent stress, rather than performance optimization, throughput, or cost efficiency. STRESS provides a reproducible, comparative methodology for observing how systems fail, degrade, contain errors, and recover when exposed to structured environmental pressure. The framework produces a composite metric, the **Stress Resilience Index (SRI)**, representing a system's demonstrated behavioral stability under a defined stress regime.

### Five Stress Regimes
1. **CPU Saturation** — all cores pinned at 100%
2. **Memory Pressure** — allocation/deallocation storms
3. **IO Flood** — disk and network at maximum throughput
4. **Capability Storm** — thousands of concurrent capability requests
5. **Byzantine Messages** — malformed, replayed, out-of-order bus messages

### Five Behavioral Proxies
1. **Throughput Stability** — does performance degrade gracefully?
2. **Latency Percentiles** — p50, p95, p99 under stress
3. **Error Rate** — do errors increase proportionally or exponentially?
4. **Recovery Time** — how fast does the system return to baseline?
5. **State Consistency** — do capabilities, audit logs, and process states remain consistent?

### SRI Score
Operational Resilience Index: composite 0-100 score. Must be ≥80 for each phase to ship.

---

## 19. DRIVER ARCHITECTURE

Native drivers, userspace, message bus.

### HAL Contract
- Drivers run in Ring 3, not kernel
- MMIO/PIO via capability tokens
- IRQ delivered as messages
- DMA buffers allocated by kernel, mapped to driver

### Driver SDK
- Rust templates for PCI probe, register access
- IRQ handler boilerplate
- DMA scatter-gather helpers

### Verification
- Each driver has STRESS gate
- Hardware-in-the-loop testing on reference platform
- Community drivers: self-certification + review

---

## 20. THE ESTATE — AGENT MAP

### Free Agents (Ship with OS)

| Agent | Role | Fabric Subsystem |
|-------|------|-----------------|
| **Butler** | Root supervisor, process lifecycle, crash recovery | Init → Supervision tree root |
| **Maid** | System maintenance, cleanup, optimization | Maintenance daemon (GC, logs, temp) |
| **Groundskeeper** | Hardware monitoring, sensors, AI prediction feed | HAL + AI prediction layer |
| **Chauffeur** | Network management, connectivity, Aether policies | Network stack + Aether |
| **Concierge** | User-facing communication, notifications, intent parsing | UI/notification subsystem |
| **Archivist** | Persistent memory, observation, context across sessions | FabricFS + audit chain |

### Premium Agents (Monetized)

| Agent | Role | Revenue Model |
|-------|------|--------------|
| **Oracle** | Predictive analytics, personal insights, trend analysis | Subscription |
| **Alchemist** | Data transformation, format conversion, synthesis | Subscription |
| **Therapist** | Mood check-ins + biometric integration, wellness tracking | Subscription |
| **Quartermaster** | Resource procurement, deal-finding, gaming optimization | Subscription |
| **Curator** | Shopping agent, recommendation engine, taste learning | Subscription |

### Oracle Note
Oracle is user-facing and SEPARATE from the kernel AI layer. The kernel AI prediction layer (Groundskeeper's domain) is infrastructure. Oracle is a premium assistant that provides insights TO the user. Different scope, different layer.

### Archivist — Memory and Observation System

**Core Function:** Persistent user memory across sessions. Not file search. Not history. *Context.*

**Capture**
- Subscribes to kernel audit chain (already tamper-evident, hash-chained)
- Logs: file edits, app usage, agent actions, system events, user queries
- Hourly compression: AI summarizes into progressive layers

**Storage**
- SQLite: structured observations, metadata, timestamps
- Vector DB (Chroma or equivalent): semantic embeddings for similarity search
- Local only: no cloud, user controls retention

**Retrieval — Progressive Disclosure**

| Layer | Query | Response |
|:---|:---|:---|
| **Index** | "what was I doing?" | 5 topics, 20 tokens each |
| **Timeline** | "tell me about the mesh networking" | Chronological context |
| **Detail** | "show me the code from Tuesday" | Full file, diff, reasoning |

**Interface**
- Natural language: Concierge voice or text
- FabricScript: `memory where "authentication" and "bug" this week`
- Visual: timeline browser, graph view of project relationships

**Prediction**
- Groundskeeper integration: "you open Blender after these files"
- Workspace auto-setup: restore state from previous session
- Anomaly detection: "you never delete files at 3am, confirm?"

**Privacy**
- User marks `<private>` tags: excluded from compression
- Per-app exclusion: incognito mode for sensitive tools
- Retention policies: auto-delete after N days, manual wipe

**Integration Points**

| System | Relationship |
|:---|:---|
| **Kernel audit chain** | Capture source — tamper-evident observations |
| **Concierge** | Query interface — natural language memory access |
| **Groundskeeper** | Prediction input — behavioral pattern feed |
| **Oracle** | Insight generation — long-term trend analysis |
| **FabricFS** | Semantic file relations — context-aware storage |

**Implementation Status**
- Basic capture and search: Phase 16 (FabricFS and desktop environment)
- Core memory system: Phase 18 (AI Prediction Layer)
- Full prediction and workspace restore: Phase 19 (Estate marketplace maturity)

---

## 21. PROJECT INTEGRATION MAP

### Bundled with OS (Free)

**WTP (WeThePeople) → fabric.civic.* bus services**
- 14 government data connectors become bus services
- Congress, FEC, Federal Register, FTC, Google Civic, GovInfo, Healthcare.gov, Internet Archive, PatentsView, Senate LDA, Wikipedia, DataGov, DataUSA, CFPB
- Source: `C:/Users/dshon/Projects/WeThePeople-App/connectors/`
- Services: `C:/Users/dshon/Projects/WeThePeople-App/services/`

**Veritas → fabric.verify.* bus services**
- Claim extraction, verification pipeline, evidence scoring, knowledge graph
- Source: `C:/Users/dshon/Projects/veritas-app/src/veritas/`
- Every Estate agent that makes a factual claim routes through Veritas

**Guardian → fabric.finance.* bus services**
- Financial monitoring, whale detection, trap detection, risk management, sentiment analysis, crash intelligence, on-chain analysis
- Source: `C:/Users/dshon/Projects/Guardian-Desktop/backend/`
- Feeds into Quartermaster and Oracle for premium financial features

### Integrated into Architecture

**STRESS** → Resilience verification framework (stress-tests every phase)
**ACS** → Human override governance (Layer 3 of governance stack)
**Aether** → Chauffeur's network policy brain (multi-uplink coordination)

### Sidelined (Architecture Supports Future Integration)

**Daedalus** → Spatial semantic browser. Not in current build phases. But FabricFS semantic model + Concierge intent parsing + Archivist versioning provide the foundation. When ready, Daedalus plugs in as a premium Estate agent.

---

## 22. MONETIZATION MODEL (Android Model)

### Free Forever (GPL)
- Fabric OS kernel
- Message bus
- Capability manager
- FabricFS
- FabricScript
- Butler, Maid, Groundskeeper, Chauffeur, Concierge, Archivist
- WTP civic data connectors
- Veritas verification pipeline
- Guardian financial monitoring with paper trading simulation for stocks, forex, commodities, and crypto; backtesting engine with local data import; no external broker integration required
- STRESS benchmark suite

### Premium (Proprietary, Subscription)
- Oracle (predictive analytics)
- Alchemist (data transformation)
- Therapist (wellness + biometrics)
- Quartermaster (procurement + gaming)
- Curator (shopping + recommendations)

### Estate Marketplace
- Third-party developers can build and sell agents
- 80/20 revenue split (developer keeps 80%)
- All marketplace agents run in capability sandbox
- Fabric OS review process for security

---

## 23. PARALLEL TRACK: THE COUNCIL (Standalone)

Ship the Council governance engine as a standalone daemon for Linux/Mac/Windows.

**Target:** OpenClaw and other open-source AI projects that need governance.
**Value:** Any multi-agent system can drop in a three-tier governance engine.
**Ship:** Independent of Fabric OS timeline. Can generate revenue and reputation early.

---

## 24. SECURITY DEFENSES (COMPLETE LIST)

From external adversarial security review:

### Capability Security
1. Unforgeable tokens (cryptographic generation)
2. Hierarchical delegation chains
3. Budgeted capabilities (rate limits)
4. Nonce-based replay prevention
5. HMAC integrity verification
6. Confused deputy protection (intent declared with every cap usage)
7. Escalation flood defense (3+ denied requests in 60s → auto-quarantine)

### Bus Security
8. Message sequence numbers (monotonic, gap = attack)
9. HMAC signing per message
10. Separate read-only monitor taps (observer cannot inject)
11. Capability validation on every message before routing
12. Zero-copy to prevent TOCTOU attacks

### AI/Council Security
13. Golden decisions regression suite
14. Drift detection (cosine similarity)
15. Per-agent training caps (gradient updates/day)
16. Override decay (AI overrides expire)
17. Weight promotion transparency (audit-logged)
18. GPU temporal isolation during Tier 3
19. VRAM zeroing on model unload
20. Weight hash verification before inference
21. Runtime weight hash verification (detect tampering)
22. Strict schema separation for Council inputs (bus messages vs training data)

### Constitution Security
23. Immutable external YAML
24. CRYSTALS-Dilithium digital signature
25. TPM anchoring
26. 24-hour cooling period for amendments
27. Chaos-state constitution lock
28. 4-hour auto-revert emergency override

### Audit Security
29. Hash-chained append-only log (SHA-3-256)
30. Pre-training log verification (Council checks logs haven't been tampered)
31. Non-CAS recovery index (searchable metadata alongside content-addressable hashes)

### System-Wide
32. Global Safety State Machine (5 states)
33. Fatigue detection (late-night human overrides flagged for re-confirmation)
34. Action context display (user sees exactly what AI decided + why before confirmation)
35. Supervision trees with restart intensity limits

---

## 25. WHAT'S BUILT (FOUNDATION)

### Kernel (FabricOS) — 15 Phases, SRI 100/100 Each

| Phase | Deliverable | Strategic Value |
|:---|:---|:---|
| 0 | Memory management (buddy allocator, page tables, heap) | Everything stands on this |
| 1 | Capability system (tokens, delegation, revocation) | Security model |
| 2 | Message bus (typed IPC, zero-copy, audit chain) | Inter-process communication |
| 3 | Process model (PCB, spawn, supervision trees) | Multitasking |
| 4 | Drivers (UART, framebuffer, timer, ramdisk) | Hardware access |
| 5A | Governance (constitution, policy engine, ACS) | AI safety constraints |
| 5B | Council coordination (3-tier, learning, drift detection) | Distributed governance |
| 6 | Memory isolation (PML4 per process, handle tables) | Security boundary |
| 7 | Hardware interrupts + Ring 3 userspace | Real processes |
| 8 | VFS + tmpfs + devfs + initramfs | File system |
| 9 | Network stack loopback (TCP/UDP sockets, 8 syscalls) | Network foundation |
| 10 | Display (framebuffer, compositor, text, 3 display syscalls) | Pixels on screen |
| 11 | NIC + keyboard (PCI, virtio-net, PS/2, I/O ports) | Real hardware I/O |
| 12 | NIC integration (ARP, Ethernet, DNS, TCP/UDP over wire) | Real internet |
| 13 | TCP reliability (retransmit, Jacobson/Karels RTO, poll(), DNS cache) | Reliable networking |
| 14 | Loom integration (HTTP fetch end-to-end over virtio-net) | **Browser works** |
| 15 | TLS 1.3 (X25519, ChaCha20-Poly1305, HTTPS client, 4 syscalls) | **Secure web** |
| 16 | Window Manager (overlapping windows, z-ordering, taskbar, 6 syscalls) | **Desktop environment** |

**Total:** ~30K LOC kernel, zero warnings, boots clean in QEMU with virtio-net.

### Browser (Loom) — HTTP Working on FabricOS

| Deliverable | Status |
|:---|:---|
| Project structure (8 crates) | Done |
| wgpu window on Windows | Done |
| Design system parsed (temperature, typography, curves) | Done |
| Platform abstraction trait | Done |
| FabricOS syscall wrappers (socket, connect, send, recv, poll, DNS) | Done |
| Host backend (wgpu) | Done |
| FabricOS backend (framebuffer, HTTP client) | **Working** |
| DNS resolve → TCP connect → HTTP GET → response display | **Verified** |

---

## 26. WHAT WAS SKIPPED (DEBT TO PAY)

Items from the original README roadmap that were deferred in favor of practical priorities:

| Original README Phase | What Was Planned | Why Skipped | When to Address |
|:---|:---|:---|:---|
| Phase 6 | **FabricFS** (content-addressable, encrypted, semantic) | tmpfs sufficient for boot | Phase 16 — persistent storage |
| Phase 7 | **Full Network** (TCP/IP, TLS, QUIC, DNS, Aether) | Loopback sufficient initially | Phase 11/13 — real internet |
| Phase 8 | **FabricScript Shell** | No userspace to run it in | Phase 17 — after Loom works |
| Phase 9 | **Estate Agents / Service Integration** | No network, no services | Phase 19 — marketplace |
| Phase 10 | **AI Prediction Layer** (LSTMs, anomaly detection) | No data to predict | Phase 18 — telemetry mature |

---

## 27. WHAT WAS ADDED (NOT IN ORIGINAL README)

| Addition | Rationale | Strategic Value |
|:---|:---|:---|
| **Display system + syscalls** (Phase 10) | Loom needs to render | Browser is primary UI |
| **Memory isolation + handle ABI** (Phase 6) | Userspace processes need real isolation | Security boundary |
| **Hardware interrupts + Ring 3** (Phase 7) | Can't run userspace without this | Real processes |
| **VFS + tmpfs/devfs** (Phase 8) | Processes need file I/O | File system foundation |
| **Loopback network** (Phase 9) | Socket API for userspace | Network foundation |
| **Loom as separate project** | Parallel development velocity | Kernel and browser mature independently |
| **Dual-mode browser architecture** (Traditional/AI) | Differentiation from Chrome | User choice, intent-first |

---

## 28. ROADMAP TO COMPLETION

### TIER 1: CORE SYSTEM (Kernel + Loom Integration) ✅ COMPLETE

| Phase | Deliverable | Status |
|:---|:---|:---|
| **11** | Real NIC (virtio-net) + ARP + DNS + PS/2 keyboard | **Done** SRI 100 |
| **12** | NIC integration (Ethernet framing, TCP/UDP over wire) | **Done** SRI 100 |
| **13** | TCP reliability (retransmit, RTO, poll(), DNS cache) | **Done** SRI 100 |
| **14** | Loom integration: HTTP fetch end-to-end over real network | **Done** SRI 100 |

**Tier 1 Result:** Loom boots on FabricOS, resolves DNS, establishes TCP, sends HTTP GET to example.com, receives 711-byte response, exits clean.

### TIER 2: SECURE WEB + DESKTOP ✅ COMPLETE

| Phase | Deliverable | Status |
|:---|:---|:---|
| **15** | TLS 1.3 (X25519, ChaCha20-Poly1305), HTTPS client | **Done** SRI 100 |
| **16** | Window Manager Foundation (overlapping windows, z-ordering, taskbar, Alt+Tab) | **Done** SRI 100 |

**Tier 2 Result:** TLS 1.3 handshake, HTTPS over real network. Window manager with overlapping windows, decorations, z-ordering, taskbar, per-window input routing, Alt+Tab/Alt+F4. Loom runs as windowed application with WM syscalls 29-34.

### TIER 3: SYSTEM COMPLETION (FabricOS Feature Complete)

| Phase | Deliverable | Unlocks |
|:---|:---|:---|
| **16** | Window Manager Foundation | **Done** ✅ |
| **17** | VMX Foundation — software hypervisor, CPUID/EPT emulation, VM lifecycle | Linux app compatibility via VM bridge |
| **18** | Gaming & Media — audio mixer, virtual gamepad, streaming protocol, media codecs | **Done** SRI 100 |

### TIER 3B: HARDWARE ENABLEMENT

| Phase | Deliverable | Description |
|:---|:---|:---|
| **19** | Driver Framework | HAL contract, driver SDK, IRQ routing, MMIO/PIO capabilities |
| **20** | Intel Ethernet | e1000e native driver — template for all drivers |
| **21** | GPU Modesetting | Intel i915-equivalent, framebuffer, EDID, hotplug |
| **22** | NVMe Storage | Native NVMe controller, AHCI fallback |
| **23** | USB XHCI | Host controller, HID, mass storage, hubs |
| **24** | Intel WiFi | iwlwifi-equivalent, firmware management |

### TIER 3C: ECOSYSTEM

| Phase | Deliverable | Unlocks |
|:---|:---|:---|
| **25** | AI Marketplace & Agent SDK — third-party agent framework, Estate agent monetization, Sentinel security agent (Shannon integration) | Ecosystem growth, revenue |
| **26** | Advanced Browser (Servo Investigation) — Servo WebView research and prototyping, traditional mode for complex web compatibility, hybrid AI-Native (Loom) + Traditional (Servo), decision: integrate or continue custom engine | Full web compatibility fallback |

**Rationale for driver-first strategy:**
- Native drivers eliminate VM overhead for hardware access
- Each driver is a userspace service with capability tokens — microkernel architecture
- Reference hardware provides repeatable testing
- Driver marketplace enables community contributions with certification
- VM passthrough remains fallback for unsupported hardware only

**Tier 3 Done When:** Native drivers boot on reference hardware, gaming subsystem functional, AI marketplace live, Servo decision made. FabricOS is feature-complete desktop OS.

### TIER 4: SCALE (Post-Completion)

| Phase | Deliverable | Description |
|:---|:---|:---|
| **27** | ARM64/RISC-V Ports | Apple Silicon, Qualcomm Snapdragon X, RISC-V workstations |
| **28** | Enterprise Features | Fleet management, policy enforcement, LDAP/AD integration |
| **29** | Formal Verification | Kani integration, seL4-level proofs for critical paths |

### TIER 5: SYSTEM COMPLETION (Post-Phase 20)

**Accessibility**

| Target | Description |
|:---|:---|
| **Screen reader integration** | Orca or custom reader with Estate agent hooks |
| **High contrast themes** | Colorblind modes (deuteranopia, protanopia, tritanopia) |
| **Voice navigation** | Full voice control via Chauffeur agent |
| **Switch control** | Motor impairment support with scanning input |
| **Eye tracking** | Gaze-based cursor control and selection |

**Multi-User & Identity**

| Target | Description |
|:---|:---|
| **Family accounts** | Parental controls, per-child capability restrictions |
| **Guest mode** | Ephemeral storage, wiped on logout |
| **Enterprise directory** | LDAP and Active Directory integration |
| **Biometric auth** | Fingerprint, face, hardware security keys (YubiKey) |

**Power & Hardware**

| Target | Description |
|:---|:---|
| **Power management** | Sleep, hibernate, hybrid sleep, fast boot |
| **Battery health** | Monitoring, optimization, charge limit controls |
| **Thermal management** | Groundskeeper-supervised fan curves and throttling |
| **Hardware diversity** | ARM tablets, RISC-V workstations, community port program |

**Connectivity Expansion**

| Target | Description |
|:---|:---|
| **Bluetooth** | Audio (A2DP), input (HID), file transfer (OBEX), BLE sensors |
| **USB** | Mass storage, webcams, printers, scanners, Thunderbolt docks |
| **NFC** | Tap-to-pair, contactless payments, smart card auth |
| **Mesh networking** | AI-optimized routing, self-healing, peer-to-peer Fabric communication |

**Security & Compliance**

| Target | Description |
|:---|:---|
| **Hardware security keys** | YubiKey, TPM 2.0, smart card integration |
| **GDPR/CCPA tools** | Data export, right-to-deletion, consent management |
| **Forensics** | Tamper-evident audit logs, incident response, legal hold |
| **Certification paths** | FIPS 140-2 and Common Criteria evaluation targets |

**System Operations**

| Target | Description |
|:---|:---|
| **Atomic updates** | A/B partitioning, transactional system upgrades |
| **Automatic rollback** | Failure detection triggers instant revert |
| **Delta updates** | Bandwidth-efficient binary diffs |
| **Backup** | User data sync, cloud options, peer-to-peer Fabric-to-Fabric |
| **Disaster recovery** | Full system restore from minimal media |

**Developer & Community**

| Target | Description |
|:---|:---|
| **Documentation** | Comprehensive user guides, API reference, SDK docs |
| **Community** | Forums, Discord, contribution guidelines, mentorship |
| **Kernel debugger** | Tracing, profiling, live instrumentation tools |
| **Remote diagnostics** | Opt-in telemetry for support cases |
| **Hardware certification** | "Works with FabricOS" program for peripheral vendors |

**Enterprise & Education**

| Target | Description |
|:---|:---|
| **MDM integration** | Mobile device management for fleet enrollment |
| **Fleet management** | Policy enforcement, remote wipe, configuration push |
| **Audit dashboards** | Administrator visibility into capability usage |
| **Classroom tools** | Educator management, student device restrictions |
| **Institutional deployment** | Bulk provisioning, image management, PXE boot |

### TIER 6: ECOSYSTEM SCALE

**Hardware Partnerships**

| Target | Description |
|:---|:---|
| **OEM reference designs** | Board support packages for partner hardware |
| **Certification labs** | Testing infrastructure for "FabricOS Ready" badge |
| **Pre-installed devices** | Ship FabricOS as primary OS on partner hardware |

**Internationalization Framework**

| Target | Description |
|:---|:---|
| **Translation system** | Community-driven i18n with crowdsourced translations |
| **RTL language support** | Arabic, Hebrew layout and text rendering infrastructure |
| **CJK input methods** | Chinese, Japanese, Korean IME framework |
| **Localized documentation** | Multi-language docs platform with version tracking |

**Mesh Networking AI**

| Target | Description |
|:---|:---|
| **Chauffeur/Groundskeeper routing** | AI-optimized mesh path selection and load balancing |
| **Predictive failure detection** | ML-based node health monitoring, pre-emptive rerouting |
| **Self-healing topology** | Automatic mesh reconfiguration on node loss |
| **Offline Fabric communication** | Device-to-device data sync without internet connectivity |

---

## 29. USER EXPERIENCE LAYER

### Desktop Vision
Traditional desktop familiarity with new architecture underneath. Users see windows, taskbar, file browser — powered by capability-secured microkernel and AI agents.

### App Compatibility Strategy

| Tier | Approach | Examples | Timeline |
|:---|:---|:---|:---|
| **Web Apps** | Loom renders web applications natively | Office 365, Figma, Notion, Google Workspace | Working now (HTTP), HTTPS in Phase 15 |
| **Cloud Streaming** | Streaming clients for heavy apps and games | Steam Link, GeForce Now, Parsec, Moonlight | Phase 18-19 |
| **Linux VM Bridge** | VMX-based containers for Linux tools (fallback for unsupported hardware) | VS Code, terminal, GIMP, Blender, Krita | Phase 17 |
| **Long-term** | Compatibility layers for Windows/Mac apps | Explore Wine-style or translation | Post-Tier 3 |

### AI Marketplace
User-created agents that enhance productivity apps. Estate agents automate workflows across applications. Third-party developers build and sell agents via marketplace (80/20 revenue split).

---

## 30. GAMING STRATEGY

| Tier | Approach | Timeline |
|:---|:---|:---|
| **Tier 1: Web Games** | HTML5, WebGPU games via Loom | 6 months (after JS engine) |
| **Tier 2: Cloud Streaming** | GeForce Now, Xbox Cloud Gaming, Steam Link clients | 6-12 months |
| **Tier 3: Native Drivers** | Native GPU + audio drivers for local gaming (Phase 21+) | 12-24 months |

### Anti-Cheat
Negotiate with vendors (EasyAntiCheat, BattlEye) for FabricOS support. VM fallback available for games requiring a familiar Linux/Windows environment.

---

## 31. PRODUCTIVITY STRATEGY

| Timeline | Approach | Apps |
|:---|:---|:---|
| **Immediate** | Web apps in Loom | Microsoft 365 web, Google Workspace, Figma, Notion |
| **Short-term** | Cloud-streamed heavy apps | Photoshop, DaVinci Resolve via VM host streaming |
| **Medium-term** | Native via Linux VM | GIMP, Blender, Krita, LibreOffice, Inkscape |
| **AI Enhancement** | Estate agents automate workflows | Cross-app automation, intelligent file management |

---

## 32. SOFTWARE ECOSYSTEM

### Distribution Model
Fabric OS is **free and open source** (GPL kernel, MIT/Apache userspace). The OS itself will never cost money. Revenue comes from the premium agent marketplace — same model as Android (free OS, paid apps). All core system tools, drivers, and the Estate's base agent functionality ship with the OS at no cost.

### Package Manager: `fabric-pkg`

Fabric OS uses `fabric-pkg`, a declarative package manager designed for capability-aware installations.

| Feature | Implementation |
|:---|:---|
| **Format** | `.fpkg` — compressed archive with manifest, capability declarations, and signatures |
| **Dependency resolution** | SAT solver with capability-aware constraints |
| **Installation** | Atomic: snapshot → install → verify → commit (rollback on failure) |
| **Updates** | Delta patches with cryptographic chain verification |
| **Channels** | `stable`, `beta`, `nightly` — users choose risk tolerance |
| **Mirrors** | Decentralized via content-addressed storage (IPFS-compatible) |

```
fabric-pkg install loom-browser        # Install from stable
fabric-pkg install --channel=beta app  # Install from beta channel
fabric-pkg update --all                # Atomic update all packages
fabric-pkg rollback loom-browser       # Revert to previous version
fabric-pkg audit                       # Verify all package signatures
```

### Verified Builds

Every package in the official repository is built from source with **reproducible builds**:
- Build environment is deterministic (pinned toolchain, hermetic sandbox)
- Any user can verify a binary matches its source via `fabric-pkg verify <package>`
- Packages are signed with Ed25519 keys; the root signing key is held by Obelus Labs with offline backup
- Third-party packages require developer identity verification and code review for the curated tier

### Source Compilation

Users can build any package from source:
```
fabric-pkg build --from-source loom-browser
fabric-pkg build --from-source --optimize=native app  # CPU-specific optimizations
```
- Build dependencies are automatically fetched and sandboxed
- Custom CFLAGS/RUSTFLAGS supported for advanced users
- Cross-compilation targets: x86_64, aarch64, riscv64

### Onboarding Flow

First boot guides new users through four steps:

| Step | Screen | Purpose |
|:---|:---|:---|
| **1. Identity** | Create local user profile | Name, avatar, preferences — no cloud account required |
| **2. Network** | Connect WiFi/Ethernet | Connectivity setup, optional Fabric ID for sync |
| **3. Estate Tour** | Interactive agent demos | Meet Butler, Concierge, and Archivist with live examples |
| **4. Workspace** | Choose productivity preset | Developer, Creative, Business, or Minimal — pre-configures agent behavior and default apps |

Total time: under 3 minutes. No telemetry, no mandatory sign-in, no forced updates.

### Agent Marketplace

The marketplace is the primary revenue engine. It operates on a curated app-store model with clear tiers.

#### User-Created Agents
- **Agent SDK:** Open source toolkit for building custom agents in Rust or FabricScript
- **Local agents:** Users create personal automation agents that run locally (no review required)
- **Published agents:** Submitted to marketplace, reviewed for security/quality, listed in the store
- **Revenue split:** 80% to developer, 20% to Obelus Labs (industry-leading split)
- **Pricing:** Developers set their own prices; free agents encouraged

#### Premium Estate Agents
- **Core Estate agents** (Butler, Maid, Groundskeeper, etc.) ship free with base functionality
- **Premium tiers** unlock advanced features: deeper integrations, multi-device sync, priority processing
- **Subscription model:** Monthly or annual, per-agent or Estate bundle
- **Enterprise licensing:** Volume pricing for organizations deploying Fabric OS fleet-wide

#### Legal Framework
- **Developer Agreement:** Clear IP ownership (developers own their code, grant distribution license)
- **User Privacy:** Agents declare data access requirements upfront via capability tokens — users grant or deny
- **Liability:** Marketplace agents run in capability sandbox; malicious agents cannot escalate privileges
- **Content Policy:** No agents that facilitate harm, surveillance, or rights violations
- **Dispute Resolution:** Automated refund within 48 hours, human review for complex cases

#### Integration with The Estate
- **Capability tokens:** Marketplace agents request only the permissions they need (camera, network, files, etc.)
- **Message bus:** Third-party agents communicate with Estate agents via the same typed message bus as system services
- **Agent orchestration:** Butler can coordinate third-party agents alongside Estate agents in workflows
- **Quality signals:** Usage metrics, user ratings, and STRESS-style automated testing for published agents

### Ecosystem Metrics (Targets)
| Metric | Year 1 | Year 3 |
|:---|:---|:---|
| Published agents | 100+ | 5,000+ |
| Active developers | 50+ | 1,000+ |
| Revenue per developer (avg) | $500/yr | $5,000/yr |
| User agent install rate | 3 agents/user | 8 agents/user |

---

## 33. HARDWARE PLATFORM

### Current
QEMU emulation with virtio-net, virtio-gpu framebuffer, PS/2 keyboard.

### Reference Hardware (Locked)
- **Primary:** ThinkPad X1 Carbon (Intel) OR Framework 13 (Intel)
- All native drivers validated against reference hardware first
- Community ports branch from reference, follow certification program

### Target Platforms
- **Primary:** x86_64 laptops and desktops (Intel reference)
- **Secondary:** ARM64 (Apple Silicon, Qualcomm Snapdragon X)
- **Tertiary:** RISC-V workstations

### Native Driver Strategy
FabricOS uses **native userspace drivers** as the primary hardware access model. Each driver runs in Ring 3, communicates via message bus, and holds capability tokens for its MMIO/PIO regions and IRQs. VM passthrough is retained as a fallback for unsupported hardware only.

**Phase 19: Driver Framework**
- HAL contract: drivers in Ring 3, not kernel
- MMIO/PIO access via capability tokens
- IRQ delivered as messages on the bus
- DMA buffers allocated by kernel, mapped to driver address space
- Driver SDK with Rust templates for PCI probe, register access, IRQ handlers

**Phase 20: Intel Ethernet (e1000e)**
- Template native driver — establishes patterns for all subsequent drivers
- PCI BAR mapping, interrupt coalescing, DMA ring buffers
- STRESS gate with hardware-in-the-loop testing on reference platform

**Phase 21: GPU Modesetting**
- Intel i915-equivalent for reference hardware
- Framebuffer allocation, EDID parsing, display hotplug
- KMS-style modesetting API for window manager integration

**Phase 22: NVMe Storage**
- Native NVMe controller driver
- AHCI/SATA fallback for older hardware
- Submission/completion queue management, scatter-gather DMA

**Phase 23: USB XHCI**
- Host controller driver, device enumeration
- HID class (keyboards, mice, gamepads)
- Mass storage class, hub support

**Phase 24: Intel WiFi**
- iwlwifi-equivalent for reference hardware
- Firmware management and loading
- WPA3 integration with TLS subsystem

### Driver Marketplace
| Tier | Description | Cost |
|:---|:---|:---|
| **Basic** | Core drivers (Ethernet, storage, USB HID) | Free — ships with OS |
| **Premium** | Accelerated drivers (GPU compute, firmware-managed WiFi) | Paid — developer revenue share |
| **Certified** | Tested on reference hardware, STRESS gate verified | Certification badge |

### Community Ports
- Branch from reference hardware driver
- Self-certification program with STRESS gate templates
- Community review before certification badge
- "Works with FabricOS" program for peripheral vendors

### Integration Point
The Estate's Groundskeeper agent monitors hardware sensors and thermal state via native drivers. Chauffeur manages network policy across WiFi and Ethernet uplinks. Archivist handles NVMe storage with FabricFS semantic indexing.

**Status:** Driver framework architecture designed (Phase 19). Kernel supports userspace drivers via message bus. Capability model extends to hardware access (each driver holds tokens for its device MMIO regions and IRQs).

---

## 34. IMMEDIATE NEXT STEPS

### Phase 15: TLS/HTTPS Foundation — COMPLETE

| Task | Deliverable | Status |
|:---|:---|:---|
| Custom crypto primitives | X25519, ChaCha20-Poly1305, HKDF-SHA256 (bare-metal, no_std) | Done |
| TLS 1.3 client | ClientHello, ServerHello, key schedule, encrypted records | Done |
| TLS syscalls (25-28) | tls_connect, tls_send, tls_recv, tls_close | Done |
| STRESS Phase 15 gate | 10/10 tests, SRI 100/100 | Done |
| Loom HTTPS support | `https://example.com` works end-to-end |

### Success Criteria

| Check | How Verified |
|:---|:---|
| Loom connects to https://example.com | TLS handshake succeeds in serial log |
| Certificate validates | No certificate errors |
| Encrypted response reaches Loom | Response body matches HTTP version |
| STRESS Phase 15 gate passes | SRI >= 80 |

---

## 35. PITCH PREPARATION

### Demo Video Script (3 minutes)

1. **0:00-0:30** — Cold boot FabricOS in QEMU. Show serial output: 15 phases, all SRI 100/100.
2. **0:30-1:00** — Loom launches. DNS resolves. TCP handshake. HTTP response. Show the pipeline.
3. **1:00-1:30** — Architecture slide: capability security, message bus, AI governance. "No root, no superuser."
4. **1:30-2:00** — Roadmap: TLS next, then desktop, then daily driver. Show the velocity (25K LOC in weeks).
5. **2:00-2:30** — Market positioning: "ChromeOS security with macOS polish, AI-native from day one."
6. **2:30-3:00** — Call to action: investment, partnership, acquisition interest.

### Target Acquirers / Partners
- **Google (ChromeOS team):** Fabric's capability model is what ChromeOS should have been
- **HP / Dell / Lenovo:** OEM play — ship Fabric on hardware, differentiate from Windows
- **Apple:** AI-native architecture aligns with Apple Intelligence direction
- **Cloud providers (AWS, GCP, Azure):** Fabric as secure container host / edge OS
- **Automotive / IoT:** Capability-secured microkernel for safety-critical systems

### Key Metrics
- **25K LOC** Rust kernel — zero warnings, boots clean
- **15 phases** complete — all SRI 100/100
- **Working HTTP** — Loom fetches from the internet, DNS to display
- **Capability security** — no root, no superuser, unforgeable tokens


### Narrative
"We built a ground-up operating system kernel in 25K lines of Rust with zero warnings and a working browser that fetches HTTP over a real network. Every process is capability-secured. AI governance is built in. This is what ChromeOS would be if Google started over today, with macOS polish and AI-native architecture from day one."

---

## 36. EXISTING CODE LOCATIONS

```
C:/Users/dshon/Projects/
├── FabricOS/                 # THIS FILE — kernel (~20K LOC Rust)
│   ├── kernel/               # Microkernel (Phases 0-10)
│   ├── fabric_types/         # Shared types (ProcessId, SyscallNumber, etc.)
│   └── README.md             # Master context document (this file)
├── Loom/                     # AI-native browser (separate project)
│   ├── loom_core/            # Core browser engine
│   ├── loom_render/          # Rendering (wgpu)
│   ├── loom_platform/        # Platform abstraction (host + FabricOS backends)
│   └── loom_design/          # Design system
├── WeThePeople-App/          # WTP civic platform (14 connectors, services)
│   ├── connectors/           # 14 government data connectors
│   └── services/             # Auth, enrichment, extraction, LLM, ops
├── veritas-app/              # Veritas verification engine
│   └── src/veritas/          # Claim extract, verify, evidence, scoring
├── Guardian-Desktop/         # Guardian financial monitoring
│   └── backend/              # v5 engine, whale/trap/risk/sentiment/onchain
├── Guardian-Desktop-UI/      # Guardian frontend
├── Guardian-Mobile/          # Guardian mobile app
└── WeThePeople-Repo/         # WTP GitHub repo (cleaned up)
```

---

---

## TECHNICAL DEBT REGISTER

Tracked explicitly. Every item has an introduction phase, target resolution phase, and risk level. Items are added during development and closed when resolved.

| ID | Item | Phase Introduced | Phase Target | Risk | Status |
|----|------|-----------------|-------------|------|--------|
| TD-001 | XOR gradient placeholder in learning loop — training protocol proven, actual ML math is nonsensical | 5B | 18 | Medium | Acknowledged |
| TD-002 | No IDT/APIC — no real hardware boot or preemptive scheduling | 5B | 7 | Critical | **Fixed** (Phase 7) |
| TD-003 | Lock ordering by convention only, no compiler enforcement | 5B | 11+ | High | Acknowledged |
| TD-004 | Capability revocation BFS is O(n*m) — needs parent→children index for O(n) | 5B | 6 | Medium | **Fixed** (Phase 6) |
| TD-005 | Simulated models only (256-byte deterministic state machines) — no real inference | 5B | 18 | Medium | Acknowledged |
| TD-006 | STRESS tests pass in QEMU only — no real hardware testing, no SMP, no NUMA | 5B | 11+ | High | Acknowledged |
| TD-007 | README claimed ~50K LOC (actual ~12K at Phase 5B) | 5B | — | Low | **Fixed** (585a2d0) |
| TD-008 | `BTreeMap` in capability store allocates on insert — should be fixed-size slab | 5B | 16 | Medium | Acknowledged |
| TD-009 | No `#[must_use]` on critical Result types across codebase | 5B | 6 | Low | **Fixed** (Phase 6) |
| TD-010 | Buddy allocator free-list uses intrusive pointers in free frames — most fragile `unsafe` code | 0 | 11+ | High | Acknowledged |
| TD-011 | Serial output only (COM1 0x3F8) — real laptops need framebuffer console | 0 | 10 | Medium | **Fixed** (Phase 10) |
| TD-012 | No filesystem — model loading, config, logs all require persistent storage | 5B | 8 | High | **Partial** (tmpfs/devfs in Phase 8, FabricFS in Phase 16) |
| TD-013 | Network is loopback only — no real NIC, no ARP, no DNS | 9 | 11 | High | Planned |
| TD-014 | No keyboard/mouse input — user can't interact with display | 10 | 11 | High | Planned |
| TD-015 | Heap is 16MB — may need expansion for real workloads | 10 | 11+ | Medium | Acknowledged |
| TD-016 | Nightly Rust compiler ICE workaround (`RUSTFLAGS='-Awarnings'`) | 7 | — | Low | Workaround active |

---

## SUMMARY

| Category | Count | Status |
|:---|:---|:---|
| Kernel phases complete | 11 (0-10) | All SRI 100/100 |
| Loom phases complete | L0 | Bootstrapped |
| Phases to Tier 1 | 5 (11-15) | In progress |
| Phases to completion | 20 | Planned |
| Lines of code (kernel) | ~20K | Growing |
| Strategic differentiation | Dual-mode browser, capability security, user sovereignty | Unique |

---

*This document is the single source of truth for the Fabric OS — The Estate project. Hand this to any AI assistant chat session to give it full context for implementation work.*

*Last updated: 2026-03-04*
*Version: 3.0*
