/**
 * v8_fabricos.h — FFI Contract Between FabricOS Rust Kernel and V8 C++ Engine
 *
 * This header defines TWO interfaces:
 *
 * 1. IMPORTS: Functions provided BY the Rust kernel (v8_fabricos_*)
 *    - Implemented in kernel/src/v8_platform/mod.rs::ffi
 *    - Called by platform_fabric.cc and libc_shim.c
 *    - These are the L13.5 platform hooks
 *
 * 2. EXPORTS: Functions provided BY the C++ side (fabricos_v8_*)
 *    - Implemented in platform_fabric.cc
 *    - Called by kernel/src/js_engine/mod.rs via FFI
 *    - These wrap the V8 C++ API in a flat C interface
 *
 * All functions use C calling convention. All pointers are validated at
 * the boundary. The FPU is enabled (SSE2) when these functions execute —
 * the Rust side handles save/restore via FpuGuard.
 */

#ifndef V8_FABRICOS_H
#define V8_FABRICOS_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* =========================================================================
 * SECTION 1: IMPORTS FROM RUST KERNEL (L13.5 Platform Interface)
 *
 * These symbols are #[no_mangle] extern "C" in v8_platform/mod.rs::ffi.
 * The C/C++ shim layer calls these for OS services.
 * ========================================================================= */

/** Allocate page-aligned memory. Returns NULL on failure.
 *  Backs to DMA manager for physical memory if available. */
void* v8_fabricos_alloc(size_t size);

/** Free previously allocated memory. ptr must be from v8_fabricos_alloc. */
void v8_fabricos_free(void* ptr, size_t size);

/** Get monotonic time in nanoseconds since boot.
 *  Guaranteed non-decreasing. Uses RDTSC fallback if syscall unavailable. */
uint64_t v8_fabricos_monotonic_time(void);

/** Sleep for the given number of milliseconds.
 *  <100us: spin. 100us-100ms: hybrid. >100ms: yield to scheduler. */
void v8_fabricos_sleep(uint32_t ms);

/** Fill buffer with cryptographic-quality random bytes.
 *  Sources from kernel entropy pool. buf must be non-NULL. */
void v8_fabricos_read_entropy(uint8_t* buf, size_t len);

/** Log a message to serial console.
 *  Levels: 0=Verbose, 1=Debug, 2=Info, 3=Warning, 4=Error, 5=Fatal.
 *  msg must be null-terminated UTF-8, max 256 bytes. */
void v8_fabricos_log(int level, const char* msg);

/** Create a new kernel thread. Returns thread ID (>= 1000), or 0 on failure.
 *  entry: function pointer with C calling convention.
 *  arg: opaque argument passed to entry. */
uint64_t v8_fabricos_create_thread(void (*entry)(void*), void* arg);

/** Block until thread completes. Returns 0 on success, -1 on error. */
int v8_fabricos_join_thread(uint64_t thread_id);

/** Yield the current thread's time slice to the scheduler. */
void v8_fabricos_yield(void);

/** Get a 64-bit random number from the kernel RNG. */
uint64_t v8_fabricos_random_u64(void);


/* =========================================================================
 * SECTION 2: ADDITIONAL IMPORTS (Extended FFI — add to mod.rs::ffi)
 *
 * These functions need to be added to v8_platform/mod.rs::ffi to support
 * V8's platform abstraction layer (OS::Allocate, mmap semantics, etc.)
 * ========================================================================= */

/** Allocate executable memory (RX pages for JIT code).
 *  Returns page-aligned pointer, or NULL on failure.
 *  Memory starts as RW, call v8_fabricos_protect_exec to make RX. */
void* v8_fabricos_alloc_executable(size_t size);

/** Change memory protection to read+execute (W^X enforcement).
 *  ptr must be from v8_fabricos_alloc_executable. */
int v8_fabricos_protect_exec(void* ptr, size_t size);

/** Change memory protection to read-only. */
int v8_fabricos_protect_readonly(void* ptr, size_t size);

/** Get page size (always 4096 on x86_64). */
size_t v8_fabricos_page_size(void);

/** Set thread-local storage value.
 *  key: TLS slot identifier
 *  value: pointer-sized value to store */
int v8_fabricos_tls_set(uint64_t key, uint64_t value);

/** Get thread-local storage value. Returns 0 if key not set. */
uint64_t v8_fabricos_tls_get(uint64_t key);

/** Mutex operations for V8 internal synchronization. */
typedef struct { uint64_t opaque; } fabricos_mutex_t;
int v8_fabricos_mutex_init(fabricos_mutex_t* mutex);
int v8_fabricos_mutex_lock(fabricos_mutex_t* mutex);
int v8_fabricos_mutex_unlock(fabricos_mutex_t* mutex);
int v8_fabricos_mutex_destroy(fabricos_mutex_t* mutex);

/** Condition variable operations. */
typedef struct { uint64_t opaque; } fabricos_cond_t;
int v8_fabricos_cond_init(fabricos_cond_t* cond);
int v8_fabricos_cond_wait(fabricos_cond_t* cond, fabricos_mutex_t* mutex);
int v8_fabricos_cond_signal(fabricos_cond_t* cond);
int v8_fabricos_cond_broadcast(fabricos_cond_t* cond);
int v8_fabricos_cond_destroy(fabricos_cond_t* cond);

/** Abort execution (fatal error). Never returns. */
void v8_fabricos_abort(void) __attribute__((noreturn));

/** Write to serial console (single byte). For libc shim putchar. */
void v8_fabricos_serial_write(uint8_t byte);


/* =========================================================================
 * SECTION 3: EXPORTS TO RUST KERNEL (V8 Engine Wrapper)
 *
 * Implemented in platform_fabric.cc. Called from js_engine/mod.rs.
 * These wrap the V8 C++ API (v8::Isolate, v8::Context, v8::Script, etc.)
 * into a flat C interface.
 * ========================================================================= */

/** Initialize V8 platform and engine.
 *  thread_pool_size: number of background workers (0 = auto)
 *  Returns: 0 on success, -1 on failure */
int fabricos_v8_initialize(int thread_pool_size);

/** Create a new V8 Isolate with optional startup snapshot.
 *  snapshot_data: pointer to snapshot blob (NULL for no snapshot)
 *  snapshot_len: byte length of snapshot (0 for no snapshot)
 *  Returns: opaque isolate handle, or NULL on failure */
void* fabricos_v8_create_isolate(
    const uint8_t* snapshot_data,
    size_t snapshot_len
);

/** Execute JavaScript source code in an isolate.
 *  script: UTF-8 source (not null-terminated)
 *  script_len: byte length
 *  filename: null-terminated filename for stack traces
 *  Returns: 0 on success, -1 on compile error, -2 on runtime error */
int fabricos_v8_run_script(
    void* isolate,
    const uint8_t* script,
    size_t script_len,
    const char* filename
);

/** Evaluate JS expression and return integer result.
 *  out_value: where to write the result (must be non-NULL)
 *  Returns: 0 on success, -1 if result is not a number */
int fabricos_v8_eval_int(
    void* isolate,
    const uint8_t* script,
    size_t script_len,
    int64_t* out_value
);

/** Force garbage collection in the isolate. */
void fabricos_v8_gc(void* isolate);

/** Destroy an isolate and free all its memory. */
void fabricos_v8_dispose_isolate(void* isolate);

/** Shut down V8 engine and platform. No V8 calls after this. */
void fabricos_v8_shutdown(void);

/** Get V8 version string (null-terminated, static lifetime). */
const char* fabricos_v8_version(void);

/** Get heap statistics for an isolate. All out params must be non-NULL. */
void fabricos_v8_heap_stats(
    void* isolate,
    size_t* out_used,
    size_t* out_total,
    size_t* out_limit
);


#ifdef __cplusplus
}  /* extern "C" */
#endif

#endif  /* V8_FABRICOS_H */
