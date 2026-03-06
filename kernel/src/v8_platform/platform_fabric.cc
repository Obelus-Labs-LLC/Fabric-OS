// V8 Platform Implementation for FabricOS
// This file implements V8's platform.h interface using FabricOS syscalls

#include "platform_fabric.h"
#include "v8_fabricos_shim.h"

#include <cstring>
#include <new>

namespace v8 {
namespace platform {

// ============================================================================
// FabricOS Platform Implementation
// ============================================================================

FabricOSPlatform::FabricOSPlatform() 
    : tracing_controller_(nullptr),
      page_allocator_(new FabricOSPageAllocator()) {
}

FabricOSPlatform::~FabricOSPlatform() {
    delete page_allocator_;
}

PageAllocator* FabricOSPlatform::GetPageAllocator() {
    return page_allocator_;
}

void FabricOSPlatform::OnCriticalMemoryPressure() {
    // Trigger GC or memory compaction
    // Called when system is low on memory
}

int FabricOSPlatform::NumberOfWorkerThreads() {
    // Return number of available CPU cores
    // For FabricOS, default to 4 worker threads
    return 4;
}

std::shared_ptr<TaskRunner> FabricOSPlatform::GetForegroundTaskRunner(
    Isolate* isolate) {
    // Main thread task runner
    // In FabricOS, this is the browser's main thread
    if (!foreground_runner_) {
        foreground_runner_ = std::make_shared<FabricOSTaskRunner>(true);
    }
    return foreground_runner_;
}

std::shared_ptr<TaskRunner> FabricOSPlatform::GetBackgroundTaskRunner(
    Isolate* isolate) {
    // Background thread task runner
    if (!background_runner_) {
        background_runner_ = std::make_shared<FabricOSTaskRunner>(false);
    }
    return background_runner_;
}

void FabricOSPlatform::WaitForBackgroundTasks(Isolate* isolate) {
    // Wait for all background tasks to complete
    // Called during isolate shutdown
}

void FabricOSPlatform::CallOnWorkerThread(std::unique_ptr<Task> task) {
    // Schedule task on worker thread pool
    // For now, execute synchronously (FabricOS has limited threads)
    task->Run();
}

void FabricOSPlatform::CallOnWorkerThread(std::unique_ptr<IdleTask> task) {
    // Schedule idle task
    // Not implemented for bare metal - execute immediately
    task->Run(0.0);  // No idle time available
}

void FabricOSPlatform::CallDelayedOnWorkerThread(std::unique_ptr<Task> task,
                                                  double delay_in_seconds) {
    // Delayed task execution
    // Convert to ms and sleep
    uint64_t ms = static_cast<uint64_t>(delay_in_seconds * 1000);
    v8_fabricos_sleep_ms(static_cast<uint32_t>(ms));
    task->Run();
}

bool FabricOSPlatform::IdleTasksEnabled(Isolate* isolate) {
    // Idle tasks not supported in bare metal environment
    return false;
}

double FabricOSPlatform::MonotonicallyIncreasingTime() {
    // Return time in seconds
    return static_cast<double>(v8_fabricos_monotonic_time()) / 1e9;
}

double FabricOSPlatform::CurrentClockTimeMillis() {
    return static_cast<double>(v8_fabricos_monotonic_time_ms());
}

StackTracePrinter FabricOSPlatform::GetStackTracePrinter() {
    // Return function that prints stack trace
    return []() {
        v8_fabricos_log(V8_LOG_ERROR, "Stack trace not available in bare metal");
    };
}

try_catch_handler_t FabricOSPlatform::GetTryCatchHandler() {
    // Exception handler for V8
    return nullptr;  // Use default handler
}

// ============================================================================
// Page Allocator Implementation
// ============================================================================

FabricOSPageAllocator::FabricOSPageAllocator()
    : page_size_(4096),
      allocate_page_size_(4096) {
}

FabricOSPageAllocator::~FabricOSPageAllocator() {
}

size_t FabricOSPageAllocator::AllocatePageSize() {
    return allocate_page_size_;
}

size_t FabricOSPageAllocator::CommitPageSize() {
    return page_size_;
}

void* FabricOSPageAllocator::AllocatePages(void* hint, size_t size,
                                            size_t alignment,
                                            PageMode page_mode) {
    (void)hint;
    
    // Allocate aligned memory
    void* ptr = v8_fabricos_alloc_aligned(size, alignment);
    if (!ptr) return nullptr;
    
    // Handle page mode
    switch (page_mode) {
        case PageMode::kReadExecute:
            v8_fabricos_protect_executable(ptr, size);
            break;
        case PageMode::kReadOnly:
            v8_fabricos_protect_readonly(ptr, size);
            break;
        default:
            break;
    }
    
    return ptr;
}

bool FabricOSPageAllocator::FreePages(void* address, size_t size) {
    v8_fabricos_free(address, size);
    return true;
}

bool FabricOSPageAllocator::ReleasePages(void* address, size_t size,
                                          size_t new_size) {
    // Partial release not directly supported
    // Free and reallocate if needed
    (void)address;
    (void)size;
    (void)new_size;
    return false;
}

bool FabricOSPageAllocator::SetPermissions(void* address, size_t size,
                                            PageMode page_mode) {
    switch (page_mode) {
        case PageMode::kReadExecute:
            return v8_fabricos_protect_executable(address, size);
        case PageMode::kReadOnly:
            return v8_fabricos_protect_readonly(address, size);
        default:
            return true;
    }
}

bool FabricOSPageAllocator::DiscardSystemPages(void* address, size_t size) {
    // Hint to OS that pages can be discarded
    // Not applicable for bare metal
    (void)address;
    (void)size;
    return true;
}

// ============================================================================
// Task Runner Implementation
// ============================================================================

FabricOSTaskRunner::FabricOSTaskRunner(bool is_foreground)
    : is_foreground_(is_foreground),
      terminated_(false) {
}

FabricOSTaskRunner::~FabricOSTaskRunner() {
}

void FabricOSTaskRunner::PostTask(std::unique_ptr<Task> task) {
    if (terminated_) return;
    
    if (is_foreground_) {
        // Foreground tasks run immediately on main thread
        task->Run();
    } else {
        // Background tasks - use worker thread
        v8_fabricos_create_thread(
            [](void* arg) {
                auto* t = static_cast<Task*>(arg);
                t->Run();
                delete t;
            },
            task.release()
        );
    }
}

void FabricOSTaskRunner::PostDelayedTask(std::unique_ptr<Task> task,
                                          double delay_in_seconds) {
    uint64_t ms = static_cast<uint64_t>(delay_in_seconds * 1000);
    v8_fabricos_sleep_ms(static_cast<uint32_t>(ms));
    PostTask(std::move(task));
}

void FabricOSTaskRunner::PostIdleTask(std::unique_ptr<IdleTask> task) {
    // Idle tasks not supported - run immediately with no idle time
    task->Run(0.0);
}

bool FabricOSTaskRunner::IdleTasksEnabled() {
    return false;
}

void FabricOSTaskRunner::Terminate() {
    terminated_ = true;
}

// ============================================================================
// Tracing Controller (stub)
// ============================================================================

tracing::TracingController* FabricOSPlatform::GetTracingController() {
    // Tracing not implemented for bare metal
    return nullptr;
}

}  // namespace platform
}  // namespace v8

// ============================================================================
// C API for Rust FFI
// ============================================================================

extern "C" {

// Create and return platform instance
void* v8_platform_create() {
    return new (std::nothrow) v8::platform::FabricOSPlatform();
}

void v8_platform_destroy(void* platform) {
    delete static_cast<v8::platform::FabricOSPlatform*>(platform);
}

// Initialize V8 with FabricOS platform
int v8_initialize_fabricos() {
    v8::platform::FabricOSPlatform* platform = 
        new (std::nothrow) v8::platform::FabricOSPlatform();
    
    if (!platform) return -1;
    
    v8::V8::InitializePlatform(platform);
    return v8::V8::Initialize() ? 0 : -1;
}

void v8_shutdown_fabricos() {
    v8::V8::Dispose();
    v8::V8::ShutdownPlatform();
}

}  // extern "C"
