# FABRIC OS — THE ESTATE
## AI-Coordinated Microkernel Fabric
### Master Context Document v2.0

**Owner:** Dshon Smith / Obelus Labs LLC
**Brand:** "Welcome to The Estate, Powered by Fabric OS"
**Target:** Developers, power users, the future
**License:** GPL kernel (free forever), proprietary premium agents (Android model)

---

## 1. CORE IDENTITY

Fabric OS is a ground-up operating system built on an AI-coordinated microkernel architecture. It is NOT Linux-based. The kernel is ~12K lines of Rust today (Phase 5B), targeting ~50K at full completion. Every subsystem communicates through a single typed message bus. Security is capability-based (no root, no superuser, no ACLs). AI prediction runs in the kernel but is fully detachable — the OS works without it.

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
║  │              MICROKERNEL (Ring 0, ~12K LOC Rust)         │    ║
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

## 18. OCRB — ORBITAL COMPUTE READINESS BENCHMARK

OCRB becomes Fabric's resilience verification framework. Stress-tests every phase before it ships.

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

### ORI Score
Orbital Readiness Index: composite 0-100 score. Must be ≥80 for each phase to ship.

---

## 19. THE ESTATE — AGENT MAP

### Free Agents (Ship with OS)

| Agent | Role | Fabric Subsystem |
|-------|------|-----------------|
| **Butler** | Root supervisor, process lifecycle, crash recovery | Init → Supervision tree root |
| **Maid** | System maintenance, cleanup, optimization | Maintenance daemon (GC, logs, temp) |
| **Groundskeeper** | Hardware monitoring, sensors, AI prediction feed | HAL + AI prediction layer |
| **Chauffeur** | Network management, connectivity, Aether policies | Network stack + Aether |
| **Concierge** | User-facing communication, notifications, intent parsing | UI/notification subsystem |
| **Archivist** | Storage management, file organization, versioning | FabricFS service |

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

---

## 20. PROJECT INTEGRATION MAP

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

**OCRB** → Resilience verification framework (stress-tests every phase)
**ACS** → Human override governance (Layer 3 of governance stack)
**Aether** → Chauffeur's network policy brain (multi-uplink coordination)

### Sidelined (Architecture Supports Future Integration)

**Daedalus** → Spatial semantic browser. Not in current build phases. But FabricFS semantic model + Concierge intent parsing + Archivist versioning provide the foundation. When ready, Daedalus plugs in as a premium Estate agent.

---

## 21. MONETIZATION MODEL (Android Model)

### Free Forever (GPL)
- Fabric OS kernel
- Message bus
- Capability manager
- FabricFS
- FabricScript
- Butler, Maid, Groundskeeper, Chauffeur, Concierge, Archivist
- WTP civic data connectors
- Veritas verification pipeline
- Guardian financial monitoring
- OCRB benchmark suite

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

## 22. PARALLEL TRACK: THE COUNCIL (Standalone)

Ship the Council governance engine as a standalone daemon for Linux/Mac/Windows.

**Target:** OpenClaw and other open-source AI projects that need governance.
**Value:** Any multi-agent system can drop in a three-tier governance engine.
**Ship:** Independent of Fabric OS timeline. Can generate revenue and reputation early.

---

## 23. SECURITY DEFENSES (COMPLETE LIST)

From two rounds of adversarial review by Gemini and ChatGPT:

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

## 24. BUILD PHASES

### Phase 0 — Bare Metal Boot (QEMU)
- Bootloader (UEFI → kernel handoff)
- Serial console output ("Hello from Fabric")
- Physical memory detection
- Page allocator (buddy system)
- Virtual memory (page tables, mapping)
- Basic kernel heap
- **OCRB Gate:** Memory allocator stress test (ORI ≥ 80)
- **Done when:** Boots in QEMU, allocates memory, prints to serial

### Phase 1 — Capability Engine
- CapabilityToken implementation (all fields)
- Token generation, validation, revocation
- Delegation chains
- Budget enforcement
- Nonce tracking
- HMAC signing
- In-memory capability store
- **OCRB Gate:** Capability storm test — 10K concurrent requests (ORI ≥ 80)
- **Done when:** Can create, delegate, validate, revoke, and budget capabilities

### Phase 2 — IPC Bus
- Typed message format
- Bus router (capability-validated routing)
- Zero-copy message passing
- Sequence number tracking
- HMAC verification per message
- Separate monitor tap (read-only)
- AuditEntry generation (hash-chained)
- **OCRB Gate:** Byzantine message test + bus flood test (ORI ≥ 80)
- **Done when:** Two processes can exchange typed messages through the bus with full audit

### Phase 3 — Scheduler + Process Model
- Process struct implementation
- Intent-aware scheduling
- Priority inheritance
- Supervision tree (one-for-one, one-for-all, rest-for-one)
- Restart intensity tracking
- Process lifecycle (spawn, run, block, terminate)
- Butler as root supervisor
- **OCRB Gate:** CPU saturation + process crash/restart storm (ORI ≥ 80)
- **Done when:** Butler supervises child processes, restarts crashes, respects intensity limits

### Phase 4 — Userspace Drivers + HAL
- HAL trait definitions
- Userspace driver framework
- Interrupt dispatch via IPC
- Serial driver (userspace)
- RAM disk driver (userspace)
- Timer driver (userspace)
- Basic framebuffer driver
- IOMMU configuration
- **OCRB Gate:** Driver crash isolation test (driver dies, kernel lives) (ORI ≥ 80)
- **Done when:** Drivers run in userspace, communicate via bus, crash without taking down kernel

### Phase 5 — Governance Stack

**Phase 5A — Deterministic Governance (ships first)**
- Tier 1 rules engine (YAML/TOML parser)
- Rule evaluation pipeline
- Integration with bus router (pre-route policy check)
- Constitution loader (external YAML file)
- Constitution signature verification (Dilithium)
- ACS lifecycle state machine (ACTIVE/DEGRADED/CONTINGENCY/EMERGENCY)
- Dead-man switch implementation
- 24-hour amendment cooling period
- **OCRB Gate:** Rule evaluation under load + constitution tamper test (ORI ≥ 80)
- **Done when:** Deterministic rules govern bus traffic, constitution is verified, ACS lifecycle works

**Phase 5B — Adaptive Governance (ships second)**
- Tier 2 local model integration (ONNX runtime or similar)
- Tier 3 panel coordination
- GPU temporal isolation
- VRAM zeroing
- Weight hash verification
- Golden decisions test suite
- Drift detection
- Per-agent training caps
- Override decay mechanism
- Learning loop (decisions → fine-tuning → regression test → deploy)
- **OCRB Gate:** Model poisoning resistance test + adversarial decision test (ORI ≥ 80)
- **Done when:** Three-tier Council makes decisions, learning works with all defenses active

### Phase 6 — FabricFS
- Content-addressable storage backend
- Metadata store (tags, relations, versions)
- Semantic query engine
- POSIX compatibility layer
- Encryption at rest (AES-256-GCM)
- Archivist agent integration
- Version history
- **OCRB Gate:** IO flood + metadata corruption test (ORI ≥ 80)
- **Done when:** Files can be stored, tagged, queried, versioned, and accessed via POSIX compat

### Phase 7 — Network Stack
- TCP/IP implementation (userspace)
- UDP, DNS resolver
- TLS 1.3
- QUIC support
- Per-process network policies (capability-enforced)
- Chauffeur agent integration
- Aether policy engine (multi-uplink)
- Bandwidth allocation per Intent priority
- Failover logic
- **OCRB Gate:** Network flood + uplink failover test (ORI ≥ 80)
- **Done when:** Processes can make network requests with capability enforcement, Aether manages uplinks

### Phase 8 — FabricScript Shell
- Lexer/parser
- Typed pipeline engine
- Capability-scoped commands
- Intent-aware execution
- Pattern matching
- Bus command integration
- Tab completion
- Script file execution
- **OCRB Gate:** Shell injection test + malformed input stress test (ORI ≥ 80)
- **Done when:** User can interact with Fabric OS through FabricScript, run scripts, pipe data

### Phase 9 — Estate Agents + Service Integration

**Phase 9A — WTP Integration**
- 14 civic data connectors as bus services
- Rate limiting per connector
- Data normalization layer
- Concierge integration (civic notifications)

**Phase 9B — Veritas Integration**
- Claim extraction pipeline as bus service
- Evidence source connectors
- Scoring engine
- Knowledge graph
- Cross-agent verification routing

**Phase 9C — Guardian Integration**
- Financial monitoring services
- Whale detector, trap detector, risk manager
- Sentiment analysis, crash intelligence
- On-chain analysis
- Quartermaster/Oracle feed

**Phase 9D — Premium Agents**
- Oracle (predictive analytics)
- Alchemist (data transformation)
- Therapist (mood + biometrics)
- Quartermaster (procurement + gaming)
- Curator (shopping + recommendations)
- Premium licensing/subscription infrastructure

### Phase 10 — AI Prediction Layer
- Memory LSTM
- IO LSTM
- Contention GNN
- Anomaly Autoencoder
- Prediction hint pipeline (models → scheduler/memory manager)
- Groundskeeper agent integration
- Model serving infrastructure
- Detachment testing (disable all models, verify OS functions normally)
- **OCRB Gate:** Full system stress test with and without AI prediction (ORI ≥ 80 both ways)
- **Done when:** AI prediction improves scheduling/memory by measurable margin AND OS works perfectly without it

---

## 25. POST-PHASE TARGETS

- Estate Marketplace launch
- Third-party agent SDK
- Daedalus integration (spatial browser)
- Mobile port (ARM64)
- RISC-V port
- Council standalone daemon release
- Formal verification completion (Coq/Lean 4 proofs for cap + IPC)

---

## 26. EXISTING CODE LOCATIONS

```
C:/Users/dshon/Projects/
├── WeThePeople-App/          # WTP civic platform (14 connectors, services)
│   ├── connectors/           # 14 government data connectors
│   └── services/             # Auth, enrichment, extraction, LLM, ops
├── veritas-app/              # Veritas verification engine
│   └── src/veritas/          # Claim extract, verify, evidence, scoring
├── Guardian-Desktop/         # Guardian financial monitoring
│   └── backend/              # v5 engine, whale/trap/risk/sentiment/onchain
├── Guardian-Desktop-UI/      # Guardian frontend
├── Guardian-Mobile/          # Guardian mobile app
├── WeThePeople-Repo/         # WTP GitHub repo (cleaned up)
├── FabricOS/                 # THIS FILE — master context
├── Betting_Engine/           # Not bundled (separate project)
├── HedgeBrain/               # NOT bundled
├── HB_Futures/               # NOT bundled
└── HedgeBrain-App/           # NOT bundled
```

---

*This document is the single source of truth for the Fabric OS — The Estate project. Hand this to any AI assistant chat session to give it full context for implementation work.*

*Last updated: 2026-03-01*
*Version: 2.0*
