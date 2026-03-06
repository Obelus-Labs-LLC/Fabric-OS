# V8 Integration for FabricOS

## Overview
This document describes the V8 JavaScript engine integration for FabricOS kernel (L23 phase).

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    JavaScript Application                    │
├─────────────────────────────────────────────────────────────┤
│  V8 JavaScript Engine (libv8.a)                             │
│  ├─ Parser, Compiler, GC, Runtime                           │
│  └─ Snapshot (snapshot_blob.bin)                            │
├─────────────────────────────────────────────────────────────┤
│  V8 Platform Shim (platform_fabric.cc)                      │
│  ├─ Page Allocator (DMA-based)                              │
│  ├─ Task Runner (kernel threads)                            │
│  └─ Time/Entropy (kernel syscalls)                          │
├─────────────────────────────────────────────────────────────┤
│  C FFI Bridge (v8_fabricos_shim.cc)                         │
│  ├─ v8_fabricos_alloc/free                                  │
│  ├─ v8_fabricos_create_thread                               │
│  ├─ v8_fabricos_monotonic_time                              │
│  └─ v8_fabricos_read_entropy                                │
├─────────────────────────────────────────────────────────────┤
│  Rust Platform (kernel/src/v8_platform/)                    │
│  ├─ memory.rs (DMA allocation)                              │
│  ├─ threads.rs (scheduler integration)                      │
│  ├─ time.rs (monotonic timing)                              │
│  └─ io.rs (entropy/logging)                                 │
├─────────────────────────────────────────────────────────────┤
│  FabricOS Kernel                                            │
│  └─ Syscalls (memory, threads, time)                        │
└─────────────────────────────────────────────────────────────┘
```

## Directory Structure

```
FabricOS/
├── kernel/src/v8_platform/      # Rust platform implementation
│   ├── mod.rs                   # Main platform module
│   ├── memory.rs                # DMA-based memory allocation
│   ├── threads.rs               # Thread management
│   ├── time.rs                  # Monotonic timing
│   ├── io.rs                    # Entropy and logging
│   ├── stubs.rs                 # Stub implementations
│   └── platform_fabric.cc       # C++ platform implementation
│   └── platform_fabric.h        # C++ platform header
├── kernel/src/v8_shim/          # C/C++ FFI shims
│   ├── v8_fabricos_shim.h       # C FFI header
│   └── v8_fabricos_shim.cc      # C++ implementation
├── scripts/
│   ├── build_v8.sh              # V8 cross-compile script
│   ├── build_openlibm.sh        # OpenLibm build script
│   └── setup_v8_windows.ps1     # Windows setup helper
├── vendor/
│   ├── v8/v8/                   # V8 source (cloned)
│   ├── openlibm/                # Math library (cloned)
│   └── depot_tools/             # Build tools (cloned)
└── build/
    ├── v8/
    │   ├── libv8.a              # V8 static library (output)
    │   └── snapshot_blob.bin    # Embedded snapshot (output)
    └── openlibm/
        └── libopenlibm.a        # Math library (output)
```

## Build Instructions

### Prerequisites
- Linux with WSL2 or Docker (Windows)
- 16GB+ RAM
- 50GB free disk space
- `depot_tools` (fetched automatically)

### Step 1: Setup (Windows)
```powershell
# Run setup script
.\scripts\setup_v8_windows.ps1 -Method wsl
```

### Step 2: Fetch Sources
```bash
# In WSL
cd /mnt/c/Users/dshon/Projects/FabricOS
export PATH="$PWD/vendor/depot_tools:$PATH"

# Fetch V8
fetch v8
cd v8
git checkout 12.4.254.19
gclient sync

# Fetch OpenLibm
cd ..
git clone https://github.com/JuliaMath/openlibm.git
cd openlibm
git checkout v0.8.1
```

### Step 3: Build OpenLibm
```bash
cd /mnt/c/Users/dshon/Projects/FabricOS
bash scripts/build_openlibm.sh
```

### Step 4: Build V8
```bash
cd /mnt/c/Users/dshon/Projects/FabricOS
bash scripts/build_v8.sh
```

## FFI Signatures (for Rust/C Interop)

### Memory
```c
void* v8_alloc(size_t size);
void* v8_alloc_executable(size_t size);
void v8_free(void* ptr, size_t size);
void* v8_realloc(void* ptr, size_t old_size, size_t new_size);
```

### Threads
```c
uint64_t v8_create_thread(void (*entry)(void*), void* arg);
int v8_join_thread(uint64_t id);
void v8_yield(void);
uint64_t v8_current_thread(void);
```

### Time
```c
uint64_t v8_monotonic_time(void);      // nanoseconds
uint64_t v8_monotonic_time_ms(void);   // milliseconds
void v8_sleep(uint32_t ms);
```

### Entropy
```c
void v8_read_entropy(uint8_t* buf, size_t len);
uint64_t v8_random_u64(void);
uint64_t v8_hash_seed(void);
```

## Patches Applied

### V8 GN Args (out/fabricos/args.gn)
```gn
# Disable stdlib dependencies
use_custom_libcxx = false
use_sysroot = false

# Bare metal features
v8_enable_i18n_support = false
v8_enable_gdbjit = false
v8_use_external_startup_data = false

# Build type
is_component_build = false
is_static = true

# Flags
extra_cflags = [
    "-DV8_OS_FABRICOS",
    "-nostdlib",
    "-ffreestanding",
]
```

## Output Files

After successful build:
- `build/v8/libv8.a` - V8 static library (~100MB)
- `build/v8/snapshot_blob.bin` - Embedded snapshot (~1MB)
- `build/openlibm/libopenlibm.a` - Math library (~500KB)

## Integration with Kernel

The kernel's `build.rs` will:
1. Link `libv8.a` and `libopenlibm.a`
2. Include `snapshot_blob.bin` in initramfs
3. Export FFI symbols for V8 platform

## Status

| Component | Status |
|:---|:---|
| Rust platform (v8_platform) | ✅ Implemented |
| C/C++ shims (v8_shim) | ✅ Implemented |
| V8 build script | ✅ Ready |
| OpenLibm build script | ✅ Ready |
| V8 source fetch | ⏳ Pending |
| Cross-compile | ⏳ Pending |
| Link test | ⏳ Pending |

## Handoff Checklist

- [x] vendor/v8/ directory structure defined
- [x] vendor/openlibm/ build script created
- [x] kernel/src/v8_shim/ C/C++ shims written
- [x] kernel/src/v8_platform/platform_fabric.cc created
- [x] scripts/build_v8.sh build script ready
- [ ] libv8.a built successfully
- [ ] snapshot_blob.bin generated
- [ ] Link test passed with kernel

## Notes

1. **V8 Version**: Using 12.4.254.19 (stable release)
2. **Target**: x86_64-unknown-none (bare metal)
3. **Math Library**: OpenLibm provides `sin`, `cos`, `sqrt` without libm
4. **Memory**: DMA-based allocation with 4KB page alignment
5. **Threads**: Kernel scheduler integration (max 8 V8 threads)
6. **Time**: RDTSC-based monotonic timing

## References

- V8 Docs: https://v8.dev/docs/embed
- OpenLibm: https://github.com/JuliaMath/openlibm
- FabricOS Kernel: `kernel/src/v8_platform/mod.rs`
