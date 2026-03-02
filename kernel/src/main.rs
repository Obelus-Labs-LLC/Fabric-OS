#![no_std]
#![no_main]

extern crate alloc;

mod bus;
mod capability;
mod memory;
mod ocrb;
mod panic;
mod process;
mod serial;
use limine::BaseRevision;
use limine::request::{MemoryMapRequest, HhdmRequest};

#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
static MEMORY_MAP: MemoryMapRequest = MemoryMapRequest::new();

#[used]
static HHDM: HhdmRequest = HhdmRequest::new();

#[no_mangle]
extern "C" fn _start() -> ! {
    serial::init();

    serial_println!("[FABRIC] ============================================");
    serial_println!("[FABRIC]   Fabric OS v0.1.0 — Phase 3 (Scheduler + Process Model)");
    serial_println!("[FABRIC]   AI-Coordinated Microkernel Fabric");
    serial_println!("[FABRIC]   (c) Obelus Labs LLC");
    serial_println!("[FABRIC] ============================================");
    serial_println!("[FABRIC] Serial console initialized (COM1, 115200 8N1)");

    assert!(BASE_REVISION.is_supported());
    serial_println!("[FABRIC] Limine boot protocol revision verified");

    // Initialize Higher-Half Direct Map
    let hhdm_response = HHDM.get_response().expect("HHDM response missing");
    let hhdm_offset = hhdm_response.offset();
    memory::init_hhdm(hhdm_offset);
    serial_println!();
    serial_println!("[VMEM] Higher-Half Direct Map offset: 0x{:x}", hhdm_offset);

    // Parse and display memory map
    let mmap_response = MEMORY_MAP.get_response().expect("Memory map missing");
    let entries = mmap_response.entries();

    serial_println!();
    serial_println!("[MEMORY] Scanning physical memory map...");

    let mut usable_regions = [(0u64, 0u64); 32];
    let mut region_count = 0;
    let mut total_usable: u64 = 0;

    for entry in entries {
        let kind_str = entry_type_name(entry.entry_type);
        let size_kib = entry.length / 1024;

        serial_println!(
            "[MEMORY] Region 0x{:016x}-0x{:016x}: {} ({} KiB)",
            entry.base,
            entry.base + entry.length - 1,
            kind_str,
            size_kib
        );

        if entry.entry_type == limine::memory_map::EntryType::USABLE {
            if region_count < usable_regions.len() {
                usable_regions[region_count] = (entry.base, entry.length);
                region_count += 1;
            }
            total_usable += entry.length;
        }
    }

    let total_frames = total_usable / memory::PAGE_SIZE as u64;
    serial_println!(
        "[MEMORY] Total usable: {} MiB ({} frames)",
        total_usable / (1024 * 1024),
        total_frames
    );

    // Initialize buddy frame allocator
    serial_println!();
    memory::frame::init(&usable_regions[..region_count]);

    // Frame allocator self-test
    serial_println!();
    if let Some(frame) = memory::frame::allocate_frame() {
        serial_println!("[MEMORY] Self-test: allocated frame at {}", frame);
        memory::frame::deallocate_frame(frame);
        serial_println!("[MEMORY] Self-test: deallocated frame — OK");
    } else {
        serial_println!("[MEMORY] Self-test: FAILED — could not allocate frame");
    }

    // Page mapper self-test
    serial_println!();
    serial_println!("[VMEM] Page mapper initialized");
    vmem_self_test();

    // Initialize kernel heap
    serial_println!();
    memory::heap::init();
    heap_self_test();

    // OCRB Memory Stress Gate
    serial_println!();
    ocrb::run_phase0_gate();

    // Phase 1: Capability Engine
    serial_println!();
    capability::init();
    capability_self_test();

    // OCRB Capability Storm Gate
    serial_println!();
    ocrb::run_phase1_gate();

    // Phase 2: Typed Message Bus
    serial_println!();
    bus::init();
    bus_self_test();

    // OCRB Bus Byzantine + Flood Gate
    serial_println!();
    ocrb::run_phase2_gate();

    // Phase 3: Process Model + Scheduler
    serial_println!();
    process::init();
    process_self_test();

    // OCRB Process + Scheduler Storm Gate
    serial_println!();
    ocrb::run_phase3_gate();

    serial_println!();
    serial_println!("[FABRIC] Phase 3 complete. Process model verified.");
    serial_println!("[FABRIC] Halting.");

    halt();
}

fn entry_type_name(t: limine::memory_map::EntryType) -> &'static str {
    use limine::memory_map::EntryType;
    match t {
        EntryType::USABLE => "Usable",
        EntryType::RESERVED => "Reserved",
        EntryType::ACPI_RECLAIMABLE => "ACPI Reclaimable",
        EntryType::ACPI_NVS => "ACPI NVS",
        EntryType::BAD_MEMORY => "Bad Memory",
        EntryType::BOOTLOADER_RECLAIMABLE => "Bootloader Reclaimable",
        EntryType::EXECUTABLE_AND_MODULES => "Kernel/Modules",
        EntryType::FRAMEBUFFER => "Framebuffer",
        _ => "Unknown",
    }
}

fn vmem_self_test() {
    use memory::page_table::PageTableFlags;
    use memory::VirtAddr;

    // Pick a virtual address in an unused region for testing
    let test_virt = VirtAddr::new(0xFFFF_FFFF_C000_0000);

    // Allocate a physical frame to map
    let test_phys = memory::frame::allocate_frame().expect("alloc frame for vmem test");

    // Map it
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    memory::mapper::map(test_virt, test_phys, flags).expect("map failed");

    // Write a value through the virtual address
    unsafe {
        let ptr = test_virt.as_u64() as *mut u64;
        *ptr = 0xDEAD_BEEF_CAFE_BABE;
    }

    // Read it back
    let read_val = unsafe { *(test_virt.as_u64() as *const u64) };
    assert_eq!(read_val, 0xDEAD_BEEF_CAFE_BABE);

    // Verify translate returns the right physical address
    let translated = memory::mapper::translate(test_virt);
    assert!(translated.is_some());

    // Unmap
    let unmapped_phys = memory::mapper::unmap(test_virt).expect("unmap failed");
    assert_eq!(unmapped_phys, test_phys);

    // Verify translate returns None now
    let after_unmap = memory::mapper::translate(test_virt);
    assert!(after_unmap.is_none());

    // Return frame to allocator
    memory::frame::deallocate_frame(test_phys);

    serial_println!("[VMEM] Self-test: map/write/read/unmap — OK");
}

fn heap_self_test() {
    use alloc::{vec, vec::Vec, string::String, boxed::Box};

    // Test Vec
    let mut v: Vec<u32> = vec![1, 2, 3, 4, 5];
    v.push(6);
    assert_eq!(v.len(), 6);
    assert_eq!(v[5], 6);

    // Test Box
    let b = Box::new(42u64);
    assert_eq!(*b, 42);
    drop(b);

    // Test String
    let s = String::from("Fabric OS");
    assert_eq!(s.len(), 9);

    // Test drop and realloc
    drop(v);
    drop(s);
    let v2: Vec<u8> = vec![0xAB; 256];
    assert_eq!(v2.len(), 256);
    assert_eq!(v2[0], 0xAB);

    serial_println!("[HEAP] Self-test: Vec, Box, String — OK");
}

fn capability_self_test() {
    use capability::{ResourceId, ProcessId, Perm};

    // Create a root capability
    let cap_id = capability::create(
        ResourceId::new(ResourceId::KIND_MEMORY | 1),
        Perm::READ | Perm::WRITE | Perm::GRANT,
        ProcessId::new(1),
        None,
        None,
    ).expect("create capability");

    // Validate it
    capability::validate(cap_id.0, Perm::READ, 1).expect("validate capability");

    // Delegate a child with READ only
    let child_id = capability::delegate(
        cap_id.0,
        ProcessId::new(2),
        Perm::READ,
        None,
        None,
    ).expect("delegate capability");

    // Validate the child
    capability::validate(child_id.0, Perm::READ, 1).expect("validate child");

    // Revoke the root (should cascade to child)
    let revoked = capability::revoke(cap_id.0).expect("revoke capability");
    assert_eq!(revoked, 2); // root + child

    // Verify store is empty
    assert_eq!(capability::count(), 0);

    serial_println!("[CAP] Self-test: create/validate/delegate/revoke — OK");
}

fn bus_self_test() {
    use capability::{ResourceId, ProcessId, Perm};
    use fabric_types::{TypeId, Timestamp, MessageHeader};

    // Register two processes
    bus::register_process(ProcessId::new(1)).expect("register pid 1");
    bus::register_process(ProcessId::new(2)).expect("register pid 2");

    // Create a capability for pid 1 to send on the bus
    let cap_id = capability::create(
        ResourceId::new(ResourceId::KIND_IPC | 1),
        Perm::READ | Perm::WRITE,
        ProcessId::new(1),
        None,
        None,
    ).expect("create ipc cap");

    // Build a message header
    let mut header = MessageHeader::zeroed();
    header.version = MessageHeader::VERSION;
    header.msg_type = TypeId(1);
    header.sender = ProcessId::new(1);
    header.receiver = ProcessId::new(2);
    header.capability_id = cap_id.0;
    header.sequence = 1;
    header.timestamp = Timestamp(0);
    header.payload_len = 5;

    // Send with payload
    bus::send(&header, Some(b"hello"), 1).expect("send message");

    // Receive
    let env = bus::receive(ProcessId::new(2)).expect("receive message");
    assert_eq!(env.header.sender, ProcessId::new(1));
    assert_eq!(env.header.payload_len, 5);

    // Verify payload via arena
    if let Some(slice) = env.payload {
        let guard = bus::BUS.lock();
        let data = guard.payload(slice);
        assert_eq!(data, b"hello");
    }

    // Verify audit chain
    let (count, valid) = bus::verify_audit_chain();
    assert!(valid);
    assert!(count > 0);

    // Clean up
    bus::BUS.lock().clear();
    capability::STORE.lock().clear();

    serial_println!("[BUS] Self-test: send/receive/audit — OK");
}

fn process_self_test() {
    use fabric_types::{ProcessId, ProcessState, Intent};

    // Butler should exist at pid 1
    assert_eq!(process::get_state(ProcessId::BUTLER), Some(ProcessState::Ready));

    // Spawn a child under Butler
    let child = process::spawn(
        ProcessId::BUTLER,
        Intent::default(),
        "test child",
        None,
    ).expect("spawn test child");

    // Verify it exists and is Ready
    assert_eq!(process::get_state(child), Some(ProcessState::Ready));

    // Terminate it
    process::terminate(child).expect("terminate test child");

    // Clean up for OCRB
    process::TABLE.lock().clear();
    process::SCHEDULER.lock().clear();
    bus::BUS.lock().clear();
    capability::STORE.lock().clear();
    process::init(); // Re-init Butler

    serial_println!("[PROC] Self-test: spawn/query/terminate — OK");
}

fn halt() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
