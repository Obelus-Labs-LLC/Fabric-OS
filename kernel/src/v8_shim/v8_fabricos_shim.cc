// V8 FabricOS Platform Shim Implementation
// C++ wrapper calling into Rust kernel via FFI

#include "v8_fabricos_shim.h"
#include <string.h>

// ============================================================================
// External FFI declarations (Rust side)
// These functions are implemented in kernel/src/v8_platform/mod.rs
// ============================================================================

extern "C" {
    // Memory
    extern void* v8_alloc(size_t size);
    extern void* v8_alloc_aligned(size_t size, size_t alignment);
    extern void* v8_alloc_executable(size_t size);
    extern void v8_free(void* ptr, size_t size);
    extern void* v8_realloc(void* ptr, size_t old_size, size_t new_size);
    extern bool v8_protect_executable(void* ptr, size_t size);
    extern bool v8_protect_readonly(void* ptr, size_t size);
    
    // Threads
    typedef uint64_t ThreadId;
    extern ThreadId v8_create_thread(void (*entry)(void*), void* arg);
    extern ThreadId v8_create_thread_with_priority(void (*entry)(void*), void* arg, int priority);
    extern int v8_join_thread(ThreadId id);
    extern void v8_yield(void);
    extern ThreadId v8_current_thread(void);
    extern void v8_set_thread_name(ThreadId id, const char* name);
    extern void v8_sleep_ms(uint32_t ms);
    extern void v8_sleep_us(uint32_t us);
    
    // Time
    extern uint64_t v8_monotonic_time(void);
    extern uint64_t v8_monotonic_time_ms(void);
    extern uint64_t v8_monotonic_time_us(void);
    extern void v8_sleep(uint32_t ms);
    extern void v8_sleep_ns(uint64_t ns);
    extern uint64_t v8_profile_timer(void);
    extern uint64_t v8_cycles_to_ns(uint64_t cycles);
    
    // I/O
    typedef enum {
        V8_LOG_LEVEL_VERBOSE = 0,
        V8_LOG_LEVEL_DEBUG = 1,
        V8_LOG_LEVEL_INFO = 2,
        V8_LOG_LEVEL_WARNING = 3,
        V8_LOG_LEVEL_ERROR = 4,
        V8_LOG_LEVEL_FATAL = 5,
    } V8LogLevel;
    extern void v8_log_message(V8LogLevel level, const char* message);
    extern void v8_read_entropy(uint8_t* buf, size_t len);
    extern uint32_t v8_random_u32(void);
    extern uint64_t v8_random_u64(void);
    extern double v8_random_f64(void);
    extern uint64_t v8_hash_seed(void);
    
    // Platform
    extern bool v8_platform_init(const void* config);
    extern void v8_platform_shutdown(void);
}

// ============================================================================
// Memory Implementation
// ============================================================================

void* v8_fabricos_alloc(size_t size) {
    return v8_alloc(size);
}

void* v8_fabricos_alloc_aligned(size_t size, size_t alignment) {
    // v8_alloc already ensures 4KB alignment
    // For larger alignments, we need special handling
    if (alignment <= 4096) {
        return v8_alloc(size);
    }
    // Allocate extra space for alignment
    size_t alloc_size = size + alignment;
    void* raw = v8_alloc(alloc_size);
    if (!raw) return nullptr;
    
    uintptr_t addr = reinterpret_cast<uintptr_t>(raw);
    uintptr_t aligned = (addr + alignment - 1) & ~(alignment - 1);
    return reinterpret_cast<void*>(aligned);
}

void* v8_fabricos_alloc_executable(size_t size) {
    return v8_alloc_executable(size);
}

void v8_fabricos_free(void* ptr, size_t size) {
    v8_free(ptr, size);
}

void* v8_fabricos_realloc(void* ptr, size_t old_size, size_t new_size) {
    return v8_realloc(ptr, old_size, new_size);
}

bool v8_fabricos_protect_executable(void* ptr, size_t size) {
    return v8_protect_executable(ptr, size);
}

bool v8_fabricos_protect_readonly(void* ptr, size_t size) {
    return v8_protect_readonly(ptr, size);
}

// ============================================================================
// Threading Implementation
// ============================================================================

v8_thread_id_t v8_fabricos_create_thread(v8_thread_fn_t entry, void* arg) {
    return v8_create_thread(entry, arg);
}

v8_thread_id_t v8_fabricos_create_thread_with_priority(
    v8_thread_fn_t entry, 
    void* arg, 
    int priority) {
    return v8_create_thread_with_priority(entry, arg, priority);
}

int v8_fabricos_join_thread(v8_thread_id_t id) {
    return v8_join_thread(id);
}

void v8_fabricos_yield(void) {
    v8_yield();
}

v8_thread_id_t v8_fabricos_current_thread(void) {
    return v8_current_thread();
}

void v8_fabricos_set_thread_name(v8_thread_id_t id, const char* name) {
    v8_set_thread_name(id, name);
}

void v8_fabricos_sleep_ms(uint32_t ms) {
    v8_sleep_ms(ms);
}

void v8_fabricos_sleep_us(uint32_t us) {
    v8_sleep_us(us);
}

// ============================================================================
// Time Implementation
// ============================================================================

uint64_t v8_fabricos_monotonic_time(void) {
    return v8_monotonic_time();
}

uint64_t v8_fabricos_monotonic_time_ms(void) {
    return v8_monotonic_time_ms();
}

uint64_t v8_fabricos_monotonic_time_us(void) {
    return v8_monotonic_time_us();
}

void v8_fabricos_sleep(uint32_t ms) {
    v8_sleep(ms);
}

void v8_fabricos_sleep_ns(uint64_t ns) {
    v8_sleep_ns(ns);
}

uint64_t v8_fabricos_profile_timer(void) {
    return v8_profile_timer();
}

uint64_t v8_fabricos_cycles_to_ns(uint64_t cycles) {
    return v8_cycles_to_ns(cycles);
}

// ============================================================================
// I/O Implementation
// ============================================================================

void v8_fabricos_log(v8_log_level_t level, const char* message) {
    v8_log_message(static_cast<V8LogLevel>(level), message);
}

void v8_fabricos_read_entropy(uint8_t* buf, size_t len) {
    v8_read_entropy(buf, len);
}

uint32_t v8_fabricos_random_u32(void) {
    return v8_random_u32();
}

uint64_t v8_fabricos_random_u64(void) {
    return v8_random_u64();
}

double v8_fabricos_random_f64(void) {
    return v8_random_f64();
}

uint64_t v8_fabricos_hash_seed(void) {
    return v8_hash_seed();
}

// ============================================================================
// File Operations (stubbed - files loaded by kernel)
// ============================================================================

v8_file_handle_t* v8_fabricos_open_file(const char* path) {
    // Files are embedded in initramfs, loaded by kernel
    // This is a stub - actual file loading happens in Rust
    (void)path;
    return nullptr;
}

size_t v8_fabricos_read_file(v8_file_handle_t* handle, void* buf, size_t len) {
    (void)handle;
    (void)buf;
    (void)len;
    return 0;
}

void v8_fabricos_close_file(v8_file_handle_t* handle) {
    (void)handle;
}

// ============================================================================
// FPU/CPU Features
// ============================================================================

void v8_fabricos_fpu_initialize(void) {
    // FPU initialized by kernel on context switch
}

void v8_fabricos_fpu_save_state(void* buffer) {
    // Use xsave to save FPU/SSE/AVX state
    // Buffer must be 64-byte aligned, 512 bytes minimum
    if (!buffer) return;
    
    #ifdef __x86_64__
    asm volatile(
        "xsave (%0)"
        :
        : "r"(buffer), "a"(0xFFFFFFFF), "d"(0xFFFFFFFF)
        : "memory"
    );
    #endif
}

void v8_fabricos_fpu_restore_state(void* buffer) {
    if (!buffer) return;
    
    #ifdef __x86_64__
    asm volatile(
        "xrstor (%0)"
        :
        : "r"(buffer), "a"(0xFFFFFFFF), "d"(0xFFFFFFFF)
        : "memory"
    );
    #endif
}

bool v8_fabricos_cpu_has_sse42(void) {
    #ifdef __x86_64__
    uint32_t eax, ebx, ecx, edx;
    asm volatile("cpuid"
        : "=a"(eax), "=b"(ebx), "=c"(ecx), "=d"(edx)
        : "a"(1)
    );
    return (ecx & (1 << 20)) != 0;  // SSE4.2 bit
    #else
    return false;
    #endif
}

bool v8_fabricos_cpu_has_avx(void) {
    #ifdef __x86_64__
    uint32_t eax, ebx, ecx, edx;
    asm volatile("cpuid"
        : "=a"(eax), "=b"(ebx), "=c"(ecx), "=d"(edx)
        : "a"(1)
    );
    return (ecx & (1 << 28)) != 0;  // AVX bit
    #else
    return false;
    #endif
}

bool v8_fabricos_cpu_has_avx2(void) {
    #ifdef __x86_64__
    uint32_t eax, ebx, ecx, edx;
    asm volatile("cpuid"
        : "=a"(eax), "=b"(ebx), "=c"(ecx), "=d"(edx)
        : "a"(7), "c"(0)
    );
    return (ebx & (1 << 5)) != 0;  // AVX2 bit
    #else
    return false;
    #endif
}

// ============================================================================
// Platform Initialization
// ============================================================================

static bool g_platform_initialized = false;

bool v8_fabricos_platform_init(const v8_platform_config_t* config) {
    if (g_platform_initialized) {
        return true;
    }
    
    bool result = v8_platform_init(config);
    if (result) {
        g_platform_initialized = true;
    }
    return result;
}

void v8_fabricos_platform_shutdown(void) {
    if (!g_platform_initialized) {
        return;
    }
    
    v8_platform_shutdown();
    g_platform_initialized = false;
}

bool v8_fabricos_platform_initialized(void) {
    return g_platform_initialized;
}

// ============================================================================
// V8 Integration Helpers
// ============================================================================

void* v8_fabricos_isolate_init(void) {
    // Called when V8 creates a new Isolate
    // Returns per-isolate data (nullptr for now)
    return nullptr;
}

void v8_fabricos_isolate_dispose(void* data) {
    (void)data;
}

void v8_fabricos_stack_guard_init(void) {
    // Stack limit checking initialized by kernel
}

void v8_fabricos_stack_guard_check(void) {
    // Called by V8 to check for stack overflow
    // Kernel handles this via guard pages
}

void v8_fabricos_abort(void) {
    #ifdef __x86_64__
    asm volatile("hlt");
    #endif
    __builtin_unreachable();
}

void v8_fabricos_fatal_error(const char* location, const char* message) {
    (void)location;
    (void)message;
    v8_fabricos_abort();
}
