// V8 FabricOS Platform Shim Header
// C/C++ FFI interface between V8 and FabricOS kernel

#ifndef V8_FABRICOS_SHIM_H
#define V8_FABRICOS_SHIM_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// ============================================================================
// Platform API (matches v8_platform Rust module)
// ============================================================================

// Memory allocation
typedef enum {
    V8_MEM_REGULAR = 0,
    V8_MEM_EXECUTABLE = 1,
    V8_MEM_LARGE_PAGES = 2,
} v8_memory_type_t;

void* v8_fabricos_alloc(size_t size);
void* v8_fabricos_alloc_aligned(size_t size, size_t alignment);
void* v8_fabricos_alloc_executable(size_t size);
void v8_fabricos_free(void* ptr, size_t size);
void* v8_fabricos_realloc(void* ptr, size_t old_size, size_t new_size);

// Memory protection
bool v8_fabricos_protect_executable(void* ptr, size_t size);
bool v8_fabricos_protect_readonly(void* ptr, size_t size);

// ============================================================================
// Threading API
// ============================================================================

typedef uint64_t v8_thread_id_t;
typedef void (*v8_thread_fn_t)(void* arg);

v8_thread_id_t v8_fabricos_create_thread(v8_thread_fn_t entry, void* arg);
v8_thread_id_t v8_fabricos_create_thread_with_priority(
    v8_thread_fn_t entry, 
    void* arg, 
    int priority  // 0=low, 1=normal, 2=high
);
int v8_fabricos_join_thread(v8_thread_id_t id);
void v8_fabricos_yield(void);
v8_thread_id_t v8_fabricos_current_thread(void);
void v8_fabricos_set_thread_name(v8_thread_id_t id, const char* name);
void v8_fabricos_sleep_ms(uint32_t ms);
void v8_fabricos_sleep_us(uint32_t us);

// ============================================================================
// Time API
// ============================================================================

uint64_t v8_fabricos_monotonic_time(void);      // nanoseconds
uint64_t v8_fabricos_monotonic_time_ms(void);   // milliseconds
uint64_t v8_fabricos_monotonic_time_us(void);   // microseconds
void v8_fabricos_sleep(uint32_t ms);
void v8_fabricos_sleep_ns(uint64_t ns);

// High-resolution timing for profiling
uint64_t v8_fabricos_profile_timer(void);
uint64_t v8_fabricos_cycles_to_ns(uint64_t cycles);

// ============================================================================
// I/O and Entropy API
// ============================================================================

typedef enum {
    V8_LOG_VERBOSE = 0,
    V8_LOG_DEBUG = 1,
    V8_LOG_INFO = 2,
    V8_LOG_WARNING = 3,
    V8_LOG_ERROR = 4,
    V8_LOG_FATAL = 5,
} v8_log_level_t;

void v8_fabricos_log(v8_log_level_t level, const char* message);
void v8_fabricos_read_entropy(uint8_t* buf, size_t len);
uint32_t v8_fabricos_random_u32(void);
uint64_t v8_fabricos_random_u64(void);
double v8_fabricos_random_f64(void);
uint64_t v8_fabricos_hash_seed(void);

// ============================================================================
// File Operations (for snapshot loading)
// ============================================================================

typedef struct {
    int fd;
    uint64_t size;
} v8_file_handle_t;

v8_file_handle_t* v8_fabricos_open_file(const char* path);
size_t v8_fabricos_read_file(v8_file_handle_t* handle, void* buf, size_t len);
void v8_fabricos_close_file(v8_file_handle_t* handle);

// ============================================================================
// FPU/CPU Features
// ============================================================================

void v8_fabricos_fpu_initialize(void);
void v8_fabricos_fpu_save_state(void* buffer);   // 512 bytes for xsave
void v8_fabricos_fpu_restore_state(void* buffer);
bool v8_fabricos_cpu_has_sse42(void);
bool v8_fabricos_cpu_has_avx(void);
bool v8_fabricos_cpu_has_avx2(void);

// ============================================================================
// Platform Initialization
// ============================================================================

typedef struct {
    uint32_t worker_threads;
    uint64_t heap_size_limit;
    bool use_huge_pages;
} v8_platform_config_t;

bool v8_fabricos_platform_init(const v8_platform_config_t* config);
void v8_fabricos_platform_shutdown(void);
bool v8_fabricos_platform_initialized(void);

// ============================================================================
// V8 Integration Helpers
// ============================================================================

// Called by V8 during Isolate creation
void* v8_fabricos_isolate_init(void);
void v8_fabricos_isolate_dispose(void* data);

// Stack guard handling
void v8_fabricos_stack_guard_init(void);
void v8_fabricos_stack_guard_check(void);

// Exception handling
void v8_fabricos_abort(void);
void v8_fabricos_fatal_error(const char* location, const char* message);

#ifdef __cplusplus
}
#endif

#endif // V8_FABRICOS_SHIM_H
