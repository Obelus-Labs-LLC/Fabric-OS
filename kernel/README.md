# FabricOS V8 Platform (Temporary)

This directory contains the **V8 JavaScript Engine Platform Interface** for FabricOS, implemented as a standalone kernel module. This is temporary work to support V8 integration without waiting for the full FabricOS kernel repository setup.

## Context

**Why this exists:**
- Loom L23 (V8 Integration) requires a V8 platform interface
- The official FabricOS repository is not yet ready for V8 development
- This temporary implementation allows parallel development velocity

**What this is:**
- A `#![no_std]` kernel module providing OS services to V8
- Pure syscall-based implementation (no std library)
- Stubs for testing on host systems
- Ready for integration with real FabricOS kernel

**Future:**
- This code will be merged into the official FabricOS kernel repository
- The stubs will be replaced with real syscall implementations
- Loom will link against the official FabricOS V8 platform

## Architecture

```
kernel/src/v8_platform/
├── mod.rs          # Platform integration, FFI, sync primitives
├── memory.rs       # D2: DMA memory allocation (300 lines)
├── threads.rs      # D3: Threading, TLS (350 lines)
├── time.rs         # D4: Monotonic time, sleep (200 lines)
├── io.rs           # D5: Entropy, logging (250 lines)
├── stubs.rs        # Host stubs for testing
└── test_main.rs    # Test binary entry point
```

## Services Provided

### Memory (D2)
- `v8_alloc()` - 4KB aligned heap memory
- `v8_alloc_executable()` - RX pages for JIT code
- `v8_alloc_large_pages()` - 2MB huge pages
- `v8_protect_executable()` - Make memory executable

### Threading (D3)
- `v8_create_thread()` - Spawn FabricOS thread
- `v8_join_thread()` - Wait for completion
- `v8_yield()` - Yield to scheduler
- `set_v8_isolate()` / `get_v8_isolate()` - TLS for isolates

### Time (D4)
- `v8_monotonic_time()` - Nanoseconds since boot
- `v8_sleep()` - Millisecond sleep
- `v8_profile_timer()` - RDTSC for profiling
- `V8Timer` - Interval measurement

### I/O & Entropy (D5)
- `v8_read_entropy()` - Kernel RNG for Math.random
- `v8_random_u64()` - Random value generation
- `v8_log_message()` - Serial output for debugging

## Building

### Library (for linking with V8)
```bash
# Release build
cargo build --release --lib --features v8-platform

# With real FabricOS kernel (disables stubs)
cargo build --release --lib --features "v8-platform fabricos-kernel"
```

### Test Binary
```bash
# Build test executable
cargo build --features "v8-platform test-binary" --target x86_64-pc-windows-msvc
```

## Cargo Features

| Feature | Description |
|---------|-------------|
| `v8-platform` (default) | Enable V8 platform integration |
| `fabricos-kernel` | Use real kernel syscalls (disables stubs) |
| `test-binary` | Build the test executable |

## FFI for C++ V8

The module exports C-compatible functions for linking with V8:

```c
// Memory
void* v8_fabricos_alloc(size_t size);
void  v8_fabricos_free(void* ptr, size_t size);

// Time
uint64_t v8_fabricos_monotonic_time(void);
void     v8_fabricos_sleep(uint32_t ms);

// Threads
uint64_t v8_fabricos_create_thread(void (*entry)(void*), void* arg);
int      v8_fabricos_join_thread(uint64_t id);
void     v8_fabricos_yield(void);

// Entropy
void v8_fabricos_read_entropy(uint8_t* buf, size_t len);
uint64_t v8_fabricos_random_u64(void);

// Logging
void v8_fabricos_log(int level, const char* msg);
```

## Development Status

| Deliverable | Status | File |
|-------------|--------|------|
| D1: V8 Platform Interface | ✅ Complete | `mod.rs` (original) |
| D2: Memory Allocator | ✅ Complete | `memory.rs` |
| D3: Threading | ✅ Complete | `threads.rs` |
| D4: Time Services | ✅ Complete | `time.rs` |
| D5: I/O & Entropy | ✅ Complete | `io.rs` |
| D6: Platform Integration | ✅ Complete | `mod.rs` (updated) |
| D7: Build Integration | ✅ Complete | `Cargo.toml` |

## Next Steps

1. **Link with V8**: Compile V8 for `x86_64-unknown-none` and link
2. **JavaScript Execution**: Test basic JS evaluation
3. **DOM Integration**: Connect V8 to Loom's DOM
4. **Performance**: Benchmark vs Boa JS engine
5. **Merge**: Move to official FabricOS kernel repository

## License

GPL-3.0 - Same as Loom and FabricOS
