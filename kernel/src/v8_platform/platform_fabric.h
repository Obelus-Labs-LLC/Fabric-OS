// V8 Platform Implementation for FabricOS
// Header file for platform_fabric.cc

#ifndef V8_PLATFORM_FABRIC_H
#define V8_PLATFORM_FABRIC_H

#include "v8/include/v8-platform.h"
#include "v8/include/v8.h"

#include <atomic>
#include <memory>

namespace v8 {
namespace platform {

// Forward declarations
class FabricOSPageAllocator;
class FabricOSTaskRunner;

// ============================================================================
// FabricOS Platform
// ============================================================================

class FabricOSPlatform : public Platform {
public:
    FabricOSPlatform();
    ~FabricOSPlatform() override;

    // Page allocator
    PageAllocator* GetPageAllocator() override;
    void OnCriticalMemoryPressure() override;

    // Worker threads
    int NumberOfWorkerThreads() override;
    
    // Task runners
    std::shared_ptr<TaskRunner> GetForegroundTaskRunner(
        Isolate* isolate) override;
    std::shared_ptr<TaskRunner> GetBackgroundTaskRunner(
        Isolate* isolate) override;
    
    void WaitForBackgroundTasks(Isolate* isolate) override;
    void CallOnWorkerThread(std::unique_ptr<Task> task) override;
    void CallOnWorkerThread(std::unique_ptr<IdleTask> task) override;
    void CallDelayedOnWorkerThread(std::unique_ptr<Task> task,
                                    double delay_in_seconds) override;
    
    bool IdleTasksEnabled(Isolate* isolate) override;

    // Time
    double MonotonicallyIncreasingTime() override;
    double CurrentClockTimeMillis() override;

    // Stack traces
    StackTracePrinter GetStackTracePrinter() override;
    
    // Exception handling
    try_catch_handler_t GetTryCatchHandler() override;

    // Tracing
    tracing::TracingController* GetTracingController() override;

private:
    tracing::TracingController* tracing_controller_;
    FabricOSPageAllocator* page_allocator_;
    std::shared_ptr<FabricOSTaskRunner> foreground_runner_;
    std::shared_ptr<FabricOSTaskRunner> background_runner_;
};

// ============================================================================
// Page Allocator
// ============================================================================

class FabricOSPageAllocator : public PageAllocator {
public:
    FabricOSPageAllocator();
    ~FabricOSPageAllocator() override;

    size_t AllocatePageSize() override;
    size_t CommitPageSize() override;
    
    void* AllocatePages(void* hint, size_t size, size_t alignment,
                        PageMode page_mode) override;
    bool FreePages(void* address, size_t size) override;
    bool ReleasePages(void* address, size_t size, size_t new_size) override;
    bool SetPermissions(void* address, size_t size, PageMode page_mode) override;
    bool DiscardSystemPages(void* address, size_t size) override;

private:
    size_t page_size_;
    size_t allocate_page_size_;
};

// ============================================================================
// Task Runner
// ============================================================================

class FabricOSTaskRunner : public TaskRunner {
public:
    explicit FabricOSTaskRunner(bool is_foreground);
    ~FabricOSTaskRunner() override;

    void PostTask(std::unique_ptr<Task> task) override;
    void PostDelayedTask(std::unique_ptr<Task> task,
                         double delay_in_seconds) override;
    void PostIdleTask(std::unique_ptr<IdleTask> task) override;
    bool IdleTasksEnabled() override;

    void Terminate();

private:
    bool is_foreground_;
    std::atomic<bool> terminated_;
};

}  // namespace platform
}  // namespace v8

// ============================================================================
// C API
// ============================================================================

#ifdef __cplusplus
extern "C" {
#endif

void* v8_platform_create(void);
void v8_platform_destroy(void* platform);
int v8_initialize_fabricos(void);
void v8_shutdown_fabricos(void);

#ifdef __cplusplus
}
#endif

#endif  // V8_PLATFORM_FABRIC_H
