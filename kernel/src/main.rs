#![no_std]
#![no_main]

extern crate alloc;

mod address_space;
mod bus;
mod butler_state;
mod capability;
mod council;
mod elf;
mod governance;
mod hal;
mod handle;
mod memory;
mod ocrb;
mod panic;
mod process;
mod serial;
mod vfs;
mod x86;
use limine::BaseRevision;
use limine::request::{MemoryMapRequest, HhdmRequest, ModuleRequest};

#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
static MEMORY_MAP: MemoryMapRequest = MemoryMapRequest::new();

#[used]
static HHDM: HhdmRequest = HhdmRequest::new();

#[used]
static MODULES: ModuleRequest = ModuleRequest::new();

#[no_mangle]
extern "C" fn _start() -> ! {
    serial::init();

    serial_println!("[FABRIC] ============================================");
    serial_println!("[FABRIC]   Fabric OS v0.4.0 — Phase 8 (VFS + Filesystem)");
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

    // Phase 4: Userspace Drivers + HAL
    // Clean up Phase 3 OCRB state before HAL init
    process::TABLE.lock().clear();
    process::SCHEDULER.lock().clear();
    bus::BUS.lock().clear();
    capability::STORE.lock().clear();
    process::init(); // Re-init Butler for HAL

    serial_println!();
    hal::init();
    driver_self_test();

    // OCRB Driver Isolation Gate
    serial_println!();
    ocrb::run_phase4_gate();

    // Phase 5A: Deterministic Governance
    // Clean up Phase 4 OCRB state
    process::TABLE.lock().clear();
    process::SCHEDULER.lock().clear();
    bus::BUS.lock().clear();
    capability::STORE.lock().clear();
    process::init(); // Re-init Butler

    serial_println!();
    governance::init();
    governance_self_test();

    // Hardware-enforce constitution: set CR0.WP so .rodata is read-only at CPU level
    governance::wp_protect::wp_enable();
    serial_println!("[GOV] CR0.WP enabled — constitution hardware-protected");

    // OCRB Governance Gate
    serial_println!();
    ocrb::run_phase5a_gate();

    // Phase 5B: Adaptive Governance (AI Council)
    // Clean up Phase 5A OCRB state
    governance::GOVERNANCE.lock().clear();
    process::TABLE.lock().clear();
    process::SCHEDULER.lock().clear();
    bus::BUS.lock().clear();
    capability::STORE.lock().clear();
    process::init(); // Re-init Butler
    governance::init();

    serial_println!();
    council::init();
    council_self_test();

    // OCRB Council Gate
    serial_println!();
    ocrb::run_phase5b_gate();

    // Phase 6: Per-Process Address Spaces + Handle ABI
    // Clean up Phase 5B OCRB state
    governance::GOVERNANCE.lock().clear();
    council::COUNCIL.lock().clear();
    process::TABLE.lock().clear();
    process::SCHEDULER.lock().clear();
    bus::BUS.lock().clear();
    capability::STORE.lock().clear();
    process::init(); // Re-init Butler
    governance::init();
    council::init();

    serial_println!();
    serial_println!("[PHASE6] ============================================");
    serial_println!("[PHASE6]   Phase 6 — Memory Isolation + Handle ABI");
    serial_println!("[PHASE6] ============================================");

    // Initialize Phase 6 subsystems
    address_space::init();
    handle::init();
    butler_state::init();
    phase6_self_test();

    // OCRB Phase 6 Gate
    serial_println!();
    ocrb::run_phase6_gate();

    // Phase 7: Hardware Interrupts + Userspace Execution
    // Clean up Phase 6 OCRB state
    governance::GOVERNANCE.lock().clear();
    council::COUNCIL.lock().clear();
    process::TABLE.lock().clear();
    process::SCHEDULER.lock().clear();
    bus::BUS.lock().clear();
    capability::STORE.lock().clear();
    process::init(); // Re-init Butler
    governance::init();
    council::init();

    serial_println!();
    serial_println!("[PHASE7] ============================================");
    serial_println!("[PHASE7]   Phase 7 — Hardware Interrupts + Userspace");
    serial_println!("[PHASE7] ============================================");

    // Phase 7A: GDT + TSS + IDT + APIC
    x86::init();

    // Phase 7B: SYSCALL/SYSRET
    x86::init_syscall();

    // Start APIC timer (enables preemptive scheduling)
    // Use conservative initial count — ~1kHz on most QEMU configs
    x86::apic::start_timer(0x20000);

    // Enable interrupts
    x86::enable_interrupts();
    serial_println!("[PHASE7] Interrupts enabled (STI)");

    // Brief wait to confirm timer is ticking
    for _ in 0..100_000 {
        core::hint::spin_loop();
    }
    let ticks = x86::idt::tick_count();
    serial_println!("[PHASE7] Timer ticks after brief wait: {}", ticks);

    // Disable interrupts for self-tests (avoid interference)
    x86::disable_interrupts();

    phase7_self_test();

    // OCRB Phase 7 Gate
    serial_println!();
    ocrb::run_phase7_gate();

    // Phase 8: VFS + RAM Filesystem + Initramfs
    // Clean up Phase 7 OCRB state
    governance::GOVERNANCE.lock().clear();
    council::COUNCIL.lock().clear();
    process::TABLE.lock().clear();
    process::SCHEDULER.lock().clear();
    bus::BUS.lock().clear();
    capability::STORE.lock().clear();
    process::init(); // Re-init Butler
    governance::init();
    council::init();

    serial_println!();
    serial_println!("[PHASE8] ============================================");
    serial_println!("[PHASE8]   Phase 8 — VFS + RAM Filesystem");
    serial_println!("[PHASE8] ============================================");

    // Initialize VFS (mounts tmpfs at /, devfs at /dev)
    vfs::init();

    // Load initramfs if a module was provided by the bootloader
    if let Some(module_response) = MODULES.get_response() {
        let modules = module_response.modules();
        if !modules.is_empty() {
            let module = &modules[0];
            let base = module.addr() as *const u8;
            let size = module.size() as usize;
            let archive = unsafe { core::slice::from_raw_parts(base, size) };
            serial_println!("[PHASE8] Loading initramfs ({} bytes)", size);
            vfs::load_initramfs(archive);
        } else {
            serial_println!("[PHASE8] No initramfs module provided (skipping)");
        }
    } else {
        serial_println!("[PHASE8] No modules response from bootloader (skipping initramfs)");
    }

    phase8_self_test();

    // OCRB Phase 8 Gate
    serial_println!();
    ocrb::run_phase8_gate();

    serial_println!();
    serial_println!("[FABRIC] Phase 8 complete. VFS + filesystem verified.");
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

fn driver_self_test() {
    use fabric_types::{
        DriverOp, DriverRequest, DeviceClass, MessageHeader,
        ProcessId, ResourceId, TypeId, Timestamp,
    };

    let ramdisk_res = ResourceId::new(ResourceId::KIND_DEVICE | 0x03);

    // Verify 4 drivers registered
    assert_eq!(hal::driver_count(), 4);

    // Quick smoke test: send a Write to the ramdisk, dispatch, read back
    let ramdisk_pid = hal::driver_pid(ramdisk_res).expect("ramdisk pid");

    // Build a write request with 4 bytes of test data
    let mut request = DriverRequest::zeroed();
    request.operation = DriverOp::Write;
    request.device_class = DeviceClass::BlockStorage;
    request.offset = 0;
    request.length = 4;

    let req_bytes = request.to_bytes();
    let test_data: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

    // Combine request + data into payload
    let mut payload = alloc::vec::Vec::new();
    payload.extend_from_slice(&req_bytes);
    payload.extend_from_slice(&test_data);

    // We need a sender process — use Butler
    let sender = ProcessId::BUTLER;

    // Get Butler's capability for this send (create a temporary IPC cap)
    let send_cap = capability::create(
        ResourceId::new(ResourceId::KIND_IPC | 0x100),
        fabric_types::Perm::READ | fabric_types::Perm::WRITE,
        sender,
        None,
        None,
    ).expect("create self-test cap");

    let mut header = MessageHeader::zeroed();
    header.version = MessageHeader::VERSION;
    header.msg_type = TypeId::DRIVER_REQUEST;
    header.sender = sender;
    header.receiver = ramdisk_pid;
    header.capability_id = send_cap.0;
    header.payload_len = payload.len() as u32;
    header.sequence = 1;
    header.timestamp = Timestamp(0);

    bus::send(&header, Some(&payload), 1).expect("self-test send");

    // Dispatch the message to the driver
    hal::dispatch_one(ramdisk_res);

    // Check response in Butler's inbox
    let resp_env = bus::receive(sender);
    assert!(resp_env.is_some(), "self-test: no response from ramdisk");

    // Clean up the temp cap
    let _ = capability::revoke(send_cap.0);

    serial_println!("[HAL] Self-test: register/send/dispatch/response — OK");
}

fn council_self_test() {
    let council = council::COUNCIL.lock();
    assert!(council.weights.verify_all());
    let hashes = council.weights.weight_hashes();
    // Verify all 3 models have distinct non-zero hashes
    assert_ne!(hashes[0], [0u8; 32]);
    assert_ne!(hashes[0], hashes[1]);
    assert_ne!(hashes[1], hashes[2]);
    drop(council);

    serial_println!("[COUNCIL] Self-test: models/hashes/integrity — OK");
}

fn governance_self_test() {
    use fabric_types::governance::SafetyState;

    // Verify constitution loaded
    let gov = governance::GOVERNANCE.lock();
    assert_eq!(gov.rules.rule_count(), 10);
    assert!(gov.verify_constitution());
    let state = gov.safety.state();
    assert_eq!(state, SafetyState::Normal);
    drop(gov);

    serial_println!("[GOV] Self-test: constitution/hash/state — OK");
}

fn phase6_self_test() {
    use fabric_types::ProcessId;

    // Test 1: Address space create/destroy
    let addr_space = address_space::AddressSpace::create()
        .expect("create address space");
    assert!(addr_space.is_active());
    assert!(addr_space.verify_kernel_mappings());
    serial_println!("[PHASE6] Self-test: address space create + kernel mapping verify — OK");

    // Drop to clean up (calls destroy)
    drop(addr_space);

    // Test 2: Handle table alloc/resolve/release
    {
        let mut table = process::TABLE.lock();
        let butler = table.get_mut(ProcessId::BUTLER).expect("Butler PCB");

        let handle = butler.handle_table.alloc(42).expect("alloc handle");
        let cap_id = butler.handle_table.resolve(handle).expect("resolve handle");
        assert_eq!(cap_id, 42);

        butler.handle_table.release(handle).expect("release handle");

        // Stale handle should fail
        let stale_result = butler.handle_table.resolve(handle);
        assert!(stale_result.is_err());
    }
    serial_println!("[PHASE6] Self-test: handle alloc/resolve/release/stale — OK");

    // Test 3: Butler state block
    {
        let mgr = butler_state::BUTLER_STATE.lock();
        assert!(mgr.is_initialized());
        let block = mgr.load().expect("load butler state");
        assert!(block.is_valid());
        assert!(block.verify_checksum());
    }
    serial_println!("[PHASE6] Self-test: butler state persist/load/verify — OK");

    // Test 4: Break-glass (just verify it's inactive at boot)
    assert!(!governance::break_glass_active());
    serial_println!("[PHASE6] Self-test: break-glass inactive at boot — OK");

    serial_println!("[PHASE6] All Phase 6 self-tests passed");
}

fn phase7_self_test() {
    // Test 1: GDT loaded (verify we can read back the entries)
    let gdt_entries = x86::gdt::raw_entries();
    assert_ne!(gdt_entries[1], 0, "Kernel CS entry should not be zero");
    serial_println!("[PHASE7] Self-test: GDT loaded — OK");

    // Test 2: TSS RSP0 accessible (IST1 should be non-zero after init)
    let ist1 = x86::tss::get_ist1();
    assert_ne!(ist1, 0, "TSS IST1 should be set");
    serial_println!("[PHASE7] Self-test: TSS IST1 set — OK");

    // Test 3: IDT entries present
    let idt_entries = x86::idt::raw_entries();
    assert!(idt_entries[0].is_present(), "IDT entry 0 should be present");
    assert!(idt_entries[32].is_present(), "IDT entry 32 (timer) should be present");
    serial_println!("[PHASE7] Self-test: IDT entries present — OK");

    // Test 4: APIC initialized
    assert!(x86::apic::is_initialized(), "APIC should be initialized");
    serial_println!("[PHASE7] Self-test: APIC initialized — OK");

    // Test 5: SYSCALL MSRs configured
    let efer = x86::syscall::read_efer();
    assert!(efer & 1 != 0, "EFER.SCE should be set");
    serial_println!("[PHASE7] Self-test: SYSCALL EFER.SCE set — OK");

    // Test 6: Timer fired
    let ticks = x86::idt::tick_count();
    assert!(ticks > 0, "Timer should have fired at least once");
    serial_println!("[PHASE7] Self-test: Timer fired ({} ticks) — OK", ticks);

    serial_println!("[PHASE7] All Phase 7 self-tests passed");
}

fn phase8_self_test() {
    // Test 1: VFS initialized — verify mounts exist
    {
        let mounts = vfs::MOUNTS.lock();
        assert!(mounts.count() >= 2, "Should have at least 2 mounts (/ and /dev)");
    }
    serial_println!("[PHASE8] Self-test: VFS mounts present — OK");

    // Test 2: Devfs devices exist
    {
        let devfs = vfs::DEVFS.lock();
        assert!(devfs.is_initialized(), "Devfs should be initialized");
        assert!(devfs.null_inode().is_valid(), "/dev/null inode should be valid");
        assert!(devfs.zero_inode().is_valid(), "/dev/zero inode should be valid");
        assert!(devfs.random_inode().is_valid(), "/dev/random inode should be valid");
    }
    serial_println!("[PHASE8] Self-test: devfs devices present — OK");

    // Test 3: Tmpfs initialized
    {
        let tmpfs = vfs::TMPFS.lock();
        assert!(tmpfs.is_initialized(), "Tmpfs should be initialized");
        assert!(tmpfs.root_inode().is_valid(), "Tmpfs root inode should be valid");
    }
    serial_println!("[PHASE8] Self-test: tmpfs initialized — OK");

    // Test 4: Path resolution works
    {
        let result = vfs::ops::resolve_path(b"/dev/null");
        assert!(result.is_ok(), "/dev/null should resolve");
    }
    serial_println!("[PHASE8] Self-test: path resolution — OK");

    serial_println!("[PHASE8] All Phase 8 self-tests passed");
}

fn halt() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
