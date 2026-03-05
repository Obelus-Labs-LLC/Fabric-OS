#![no_std]
#![no_main]

extern crate alloc;

mod address_space;
mod bus;
mod butler_state;
mod capability;
mod council;
mod display;
mod drivers;
mod elf;
mod governance;
mod hal;
mod handle;
mod io;
mod keyboard;
mod memory;
mod network;
#[path = "stress/mod.rs"]
mod ocrb;
mod gaming;
mod panic;
mod pci;
mod process;
mod serial;
mod vfs;
mod virtio;
mod vmx;
mod wm;
mod x86;
use limine::BaseRevision;
use limine::request::{MemoryMapRequest, HhdmRequest, ModuleRequest, FramebufferRequest};

#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
static MEMORY_MAP: MemoryMapRequest = MemoryMapRequest::new();

#[used]
static HHDM: HhdmRequest = HhdmRequest::new();

#[used]
static MODULES: ModuleRequest = ModuleRequest::new();

#[used]
static FRAMEBUFFER: FramebufferRequest = FramebufferRequest::new();

#[no_mangle]
extern "C" fn _start() -> ! {
    serial::init();

    serial_println!("[FABRIC] ============================================");
    serial_println!("[FABRIC]   Fabric OS v1.6.0 — Phase 21a (xHCI USB Controller)");
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

    // STRESS Memory Stress Gate
    serial_println!();
    ocrb::run_phase0_gate();

    // Phase 1: Capability Engine
    serial_println!();
    capability::init();
    capability_self_test();

    // STRESS Capability Storm Gate
    serial_println!();
    ocrb::run_phase1_gate();

    // Phase 2: Typed Message Bus
    serial_println!();
    bus::init();
    bus_self_test();

    // STRESS Bus Byzantine + Flood Gate
    serial_println!();
    ocrb::run_phase2_gate();

    // Phase 3: Process Model + Scheduler
    serial_println!();
    process::init();
    process_self_test();

    // STRESS Process + Scheduler Storm Gate
    serial_println!();
    ocrb::run_phase3_gate();

    // Phase 4: Userspace Drivers + HAL
    // Clean up Phase 3 STRESS state before HAL init
    process::TABLE.lock().clear();
    process::SCHEDULER.lock().clear();
    bus::BUS.lock().clear();
    capability::STORE.lock().clear();
    process::init(); // Re-init Butler for HAL

    serial_println!();
    hal::init();
    driver_self_test();

    // STRESS Driver Isolation Gate
    serial_println!();
    ocrb::run_phase4_gate();

    // Phase 5A: Deterministic Governance
    // Clean up Phase 4 STRESS state
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

    // STRESS Governance Gate
    serial_println!();
    ocrb::run_phase5a_gate();

    // Phase 5B: Adaptive Governance (AI Council)
    // Clean up Phase 5A STRESS state
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

    // STRESS Council Gate
    serial_println!();
    ocrb::run_phase5b_gate();

    // Phase 6: Per-Process Address Spaces + Handle ABI
    // Clean up Phase 5B STRESS state
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

    // STRESS Phase 6 Gate
    serial_println!();
    ocrb::run_phase6_gate();

    // Phase 7: Hardware Interrupts + Userspace Execution
    // Clean up Phase 6 STRESS state
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

    // Brief wait to confirm timer is ticking (~20ms spin ensures ≥10 ticks
    // with APIC timer at ~2ms/tick on QEMU)
    for _ in 0..2_000_000 {
        core::hint::spin_loop();
    }
    let ticks = x86::idt::tick_count();
    serial_println!("[PHASE7] Timer ticks after brief wait: {}", ticks);

    // Disable interrupts for self-tests (avoid interference)
    x86::disable_interrupts();

    phase7_self_test();

    // STRESS Phase 7 Gate
    serial_println!();
    ocrb::run_phase7_gate();

    // Phase 8: VFS + RAM Filesystem + Initramfs
    // Clean up Phase 7 STRESS state
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

    // STRESS Phase 8 Gate
    serial_println!();
    ocrb::run_phase8_gate();

    // Phase 9: Network Stack (Loopback)
    serial_println!();
    serial_println!("[PHASE9] ============================================");
    serial_println!("[PHASE9]   Phase 9 — Network Stack (Loopback)");
    serial_println!("[PHASE9] ============================================");

    // Initialize network subsystem
    network::init();

    phase9_self_test();

    // STRESS Phase 9 Gate
    serial_println!();
    ocrb::run_phase9_gate();

    // Phase 10: Display System (Framebuffer + Compositor)
    serial_println!();
    serial_println!("[PHASE10] ============================================");
    serial_println!("[PHASE10]   Phase 10 — Display System");
    serial_println!("[PHASE10] ============================================");

    // Initialize display from Limine framebuffer
    if let Some(fb_response) = FRAMEBUFFER.get_response() {
        if let Some(fb) = fb_response.framebuffers().next() {
            let info = display::FramebufferInfo::new(
                fb.addr(),
                fb.width(),
                fb.height(),
                fb.pitch(),
                fb.bpp(),
                fb.red_mask_shift(),
                fb.green_mask_shift(),
                fb.blue_mask_shift(),
            );
            display::init(info);
        } else {
            serial_println!("[PHASE10] WARNING: No framebuffers in response");
        }
    } else {
        serial_println!("[PHASE10] WARNING: No framebuffer response from bootloader");
    }

    phase10_self_test();

    // STRESS Phase 10 Gate
    serial_println!();
    ocrb::run_phase10_gate();

    serial_println!();
    serial_println!("[FABRIC] Phase 10 complete. Display system verified.");

    // Phase 11: NIC + Keyboard
    serial_println!();
    serial_println!("[PHASE11] ============================================");
    serial_println!("[PHASE11]   Phase 11 — NIC + Keyboard");
    serial_println!("[PHASE11] ============================================");

    // Initialize IO APIC (route IRQ1->vec33 keyboard, IRQ11->vec43 virtio-net)
    x86::ioapic::init();

    // Initialize PS/2 keyboard
    keyboard::init();

    // PCI bus scan
    let pci_devices = pci::init();

    // --- Enumerate ALL Ethernet controllers for diagnostics ---
    let mut eth_count = 0u32;
    for dev in &pci_devices {
        if drivers::e1000e::is_ethernet_controller(dev) {
            eth_count += 1;
            serial_println!(
                "[PHASE11] NIC #{}: {:02x}:{:02x}.{} {:04x}:{:04x} IRQ={} — {}",
                eth_count, dev.bus, dev.device, dev.function,
                dev.vendor_id, dev.device_id, dev.irq_line,
                drivers::e1000e::nic_name(dev)
            );
        }
    }
    if eth_count == 0 {
        // Also check for VirtIO-net (class 0x02 but sometimes class 0x00 on QEMU)
        for dev in &pci_devices {
            if dev.is_virtio_net() {
                eth_count += 1;
                serial_println!(
                    "[PHASE11] NIC #{}: {:02x}:{:02x}.{} {:04x}:{:04x} IRQ={} — VirtIO-net",
                    eth_count, dev.bus, dev.device, dev.function,
                    dev.vendor_id, dev.device_id, dev.irq_line
                );
            }
        }
    }
    if eth_count == 0 {
        serial_println!("[PHASE11] WARNING: No Ethernet controllers found on PCI bus");
    }

    // --- Initialize NIC: e1000e first → VirtIO fallback ---
    let mut nic_found = false;

    // Try e1000e (Intel I217/I218/I219 family)
    for dev in &pci_devices {
        if drivers::e1000e::is_e1000e(dev) {
            serial_println!("[PHASE11] Initializing e1000e (0x{:04x})...", dev.device_id);
            if let Some(nic) = drivers::e1000e::E1000eDriver::init_from_pci(dev) {
                serial_println!("[PHASE11] e1000e initialized — link={}",
                    if nic.link_up { "UP" } else { "DOWN" });
                network::nic_trait::register_nic(alloc::boxed::Box::new(nic));
                nic_found = true;
            } else {
                serial_println!("[PHASE11] WARNING: e1000e init failed");
            }
            break;
        }
    }

    // Log Realtek detection (not yet driven — Phase 20B)
    if !nic_found {
        for dev in &pci_devices {
            if drivers::e1000e::is_realtek_nic(dev) {
                serial_println!("[PHASE11] DETECTED: Realtek NIC {:04x}:{:04x} — {} (no driver yet, Phase 20B)",
                    dev.vendor_id, dev.device_id, drivers::e1000e::nic_name(dev));
                serial_println!("[PHASE11] ACTION: Implement Realtek RTL8168 driver (BAR0=0x{:08x} IRQ={})",
                    dev.bars[0], dev.irq_line);
                break;
            }
        }
    }

    // Fallback: try VirtIO-net (QEMU)
    if !nic_found {
        for dev in &pci_devices {
            if dev.is_virtio_net() {
                serial_println!("[PHASE11] Initializing virtio-net...");
                if let Some(nic) = virtio::net::VirtioNet::init(dev) {
                    serial_println!("[PHASE11] virtio-net initialized");
                    let adapter = network::virtio_nic_adapter::VirtioNicAdapter::new(nic);
                    network::nic_trait::register_nic(alloc::boxed::Box::new(adapter));
                    nic_found = true;
                } else {
                    serial_println!("[PHASE11] WARNING: virtio-net init failed");
                }
                break;
            }
        }
    }

    if !nic_found {
        serial_println!("[PHASE11] WARNING: No supported NIC driver — network unavailable");
    }

    // Send ARP request for gateway (10.0.2.2)
    if network::nic_trait::has_nic() {
        network::arp::arp_request([10, 0, 2, 2]);
    }

    serial_println!("[PHASE11] Phase 11 initialization complete");

    // STRESS Phase 11 Gate
    serial_println!();
    ocrb::run_phase11_gate();

    // Phase 12: Wire NIC to Network Stack
    serial_println!();
    serial_println!("[PHASE12] ============================================");
    serial_println!("[PHASE12]   Phase 12 — NIC Integration");
    serial_println!("[PHASE12] ============================================");

    // Blocking ARP resolve for gateway (fills ARP table)
    if network::nic_trait::has_nic() {
        serial_println!("[PHASE12] Resolving gateway MAC via ARP...");
        match network::arp::arp_resolve([10, 0, 2, 2]) {
            Some(mac) => {
                serial_println!(
                    "[PHASE12] Gateway 10.0.2.2 -> {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
                );
            }
            None => {
                serial_println!("[PHASE12] WARNING: ARP resolve for gateway timed out");
            }
        }

        // DNS resolve test
        serial_println!("[PHASE12] Resolving example.com via DNS...");
        match network::dns::dns_resolve("example.com") {
            Some(ip) => {
                serial_println!(
                    "[PHASE12] example.com -> {}.{}.{}.{}",
                    ip[0], ip[1], ip[2], ip[3]
                );
            }
            None => {
                serial_println!("[PHASE12] WARNING: DNS resolve for example.com failed");
            }
        }
    } else {
        serial_println!("[PHASE12] No NIC available, skipping ARP/DNS");
    }

    serial_println!("[PHASE12] Phase 12 initialization complete");

    // STRESS Phase 12 Gate
    serial_println!();
    ocrb::run_phase12_gate();

    // Phase 13: TCP Reliability & Async I/O
    serial_println!();
    serial_println!("[PHASE13] ============================================");
    serial_println!("[PHASE13]   Phase 13 — TCP Reliability & Async I/O");
    serial_println!("[PHASE13] ============================================");
    serial_println!("[PHASE13] TCP retransmit queue: per-socket, Jacobson/Karels RTO");
    serial_println!("[PHASE13] Karn's algorithm: skip RTT on retransmitted segments");
    serial_println!("[PHASE13] poll() syscall: SYS_POLL=24, POLLIN/POLLOUT/POLLHUP");
    serial_println!("[PHASE13] DNS cache: 32-entry LRU, TTL-based expiry");
    serial_println!("[PHASE13] DNS retry: 3 attempts, pseudo-random txn IDs");
    serial_println!("[PHASE13] Phase 13 initialization complete");

    // STRESS Phase 13 Gate
    serial_println!();
    ocrb::run_phase13_gate();

    // Phase 15: TLS/HTTPS Foundation
    serial_println!();
    serial_println!("[PHASE15] ============================================");
    serial_println!("[PHASE15]   Phase 15 — TLS/HTTPS Foundation");
    serial_println!("[PHASE15] ============================================");
    serial_println!("[PHASE15] Crypto: X25519, ChaCha20-Poly1305, HKDF-SHA256");
    serial_println!("[PHASE15] TLS 1.3: ClientHello, key schedule, encrypted records");
    serial_println!("[PHASE15] Syscalls: tls_connect(25), tls_send(26), tls_recv(27), tls_close(28)");
    serial_println!("[PHASE15] Session table: 8 concurrent TLS sessions");
    serial_println!("[PHASE15] Phase 15 initialization complete");

    // STRESS Phase 15 Gate
    serial_println!();
    ocrb::run_phase15_gate();

    // Phase 16: Window Manager Foundation
    serial_println!();
    serial_println!("[PHASE16] ============================================");
    serial_println!("[PHASE16]   Phase 16 — Window Manager Foundation");
    serial_println!("[PHASE16] ============================================");
    serial_println!("[PHASE16] Window table: {} slots, z-ordered compositing", wm::MAX_WINDOWS);
    serial_println!("[PHASE16] Decorations: title bar ({}px), close button, 1px border", wm::TITLE_BAR_HEIGHT);
    serial_println!("[PHASE16] Taskbar: {}px at bottom, window list with focus highlight", wm::TASKBAR_HEIGHT);
    serial_println!("[PHASE16] Input: Alt+Tab cycle, Alt+F4 close, per-window event queues");
    serial_println!("[PHASE16] Syscalls: wm_create(29), wm_destroy(30), wm_blit(31), wm_move_resize(32), wm_focus(33), wm_event(34)");
    serial_println!("[PHASE16] Phase 16 initialization complete");

    // STRESS Phase 16 Gate
    serial_println!();
    ocrb::run_phase16_gate();

    // Phase 17: VMX Foundation (Linux VM Bridge)
    serial_println!();
    serial_println!("[PHASE17] ============================================");
    serial_println!("[PHASE17]   Phase 17 — VMX Foundation (Linux VM Bridge)");
    serial_println!("[PHASE17] ============================================");

    vmx::init();

    serial_println!("[PHASE17] VMX mode: {:?}", vmx::capability());
    serial_println!("[PHASE17] VM table: {} slots, software emulation", vmx::guest::MAX_VMS);
    serial_println!("[PHASE17] EPT: 4-level guest-physical address translation");
    serial_println!("[PHASE17] Emulator: HLT, CPUID, NOP, CLI/STI, IN/OUT, MOV, JMP");
    serial_println!("[PHASE17] Phase 17 initialization complete");

    // STRESS Phase 17 Gate
    serial_println!();
    ocrb::run_phase17_gate();

    // Phase 18: Gaming & Media
    serial_println!();
    serial_println!("[PHASE18] ============================================");
    serial_println!("[PHASE18]   Phase 18 — Gaming & Media");
    serial_println!("[PHASE18] ============================================");

    gaming::init();

    serial_println!("[PHASE18] Audio: software PCM mixer ({} source slots)", gaming::audio::MAX_SOURCES);
    serial_println!("[PHASE18] Gamepad: {} virtual slots, keyboard-mapped", gaming::gamepad::MAX_GAMEPADS);
    serial_println!("[PHASE18] Codecs: RGB/RGBA/BGR pixel formats, RLE compression");
    serial_println!("[PHASE18] Streaming: video/audio/input protocol, {} client slots", gaming::client::MAX_CLIENTS);
    serial_println!("[PHASE18] Phase 18 initialization complete");

    // STRESS Phase 18 Gate
    serial_println!();
    ocrb::run_phase18_gate();

    // Phase 19: Driver Framework
    serial_println!();
    serial_println!("[PHASE19] ============================================");
    serial_println!("[PHASE19]   Phase 19 — Driver Framework");
    serial_println!("[PHASE19] ============================================");
    hal::driver_framework_init();
    serial_println!("[PHASE19] Phase 19 initialization complete");

    // STRESS Phase 19 Gate
    serial_println!();
    ocrb::run_phase19_gate();

    // Phase 20A: Ethernet Driver (e1000e)
    serial_println!();
    serial_println!("[PHASE20A] ============================================");
    serial_println!("[PHASE20A]   Phase 20A — Ethernet Driver (e1000e)");
    serial_println!("[PHASE20A] ============================================");
    serial_println!("[PHASE20A] NicDriver trait: abstract NIC interface");
    serial_println!("[PHASE20A] VirtIO adapter: wraps VirtioNet as NicDriver");
    serial_println!("[PHASE20A] e1000e driver: Intel I217/I218/I219 + Broadwell");
    serial_println!("[PHASE20A] Realtek detection: RTL8168/8169/8101/8111GR");
    {
        let nic_guard = network::nic_trait::ACTIVE_NIC.lock();
        match nic_guard.as_ref() {
            Some(n) => {
                let mac = n.mac_address();
                serial_println!("[PHASE20A] Active NIC: {}", n.name());
                serial_println!("[PHASE20A] MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
            }
            None => {
                serial_println!("[PHASE20A] Active NIC: none");
            }
        }
    }
    serial_println!("[PHASE20A] Phase 20A initialization complete");

    // STRESS Phase 20A Gate
    serial_println!();
    ocrb::run_phase20a_gate();

    // Phase 21a: USB xHCI Host Controller Bringup
    serial_println!();
    serial_println!("[PHASE21A] ============================================");
    serial_println!("[PHASE21A]   Phase 21a — xHCI Host Controller Bringup");
    serial_println!("[PHASE21A] ============================================");
    {
        match drivers::usb::xhci::probe_pci(&pci_devices) {
            Some(ctrl) => {
                let status = if ctrl.running { "OPERATIONAL" } else { "NOT RUNNING" };
                serial_println!("[PHASE21A] xHCI controller: {}", status);
                serial_println!("[PHASE21A] xHCI version: {}.{}.{}",
                    (ctrl.caps.hci_version >> 8) & 0xFF,
                    (ctrl.caps.hci_version >> 4) & 0xF,
                    ctrl.caps.hci_version & 0xF);
                serial_println!("[PHASE21A] Ports: {} max, Slots: {} max",
                    ctrl.caps.max_ports, ctrl.caps.max_slots);
                if ctrl.running {
                    serial_println!("[PHASE21A] Target reached: USBSTS.HCH=0, CNR=0");
                }
            }
            None => {
                serial_println!("[PHASE21A] No xHCI controller initialized (may not be present)");
            }
        }
    }
    serial_println!("[PHASE21A] Phase 21a initialization complete");

    // Re-initialize process table for production use (STRESS tests left stale state)
    process::TABLE.lock().clear();
    process::SCHEDULER.lock().clear();
    bus::BUS.lock().clear();
    capability::STORE.lock().clear();
    process::init();

    // Launch Loom from initramfs
    serial_println!();
    serial_println!("[LOOM] Searching for Loom binary in initramfs...");
    if let Some(module_response) = MODULES.get_response() {
        let modules = module_response.modules();
        if !modules.is_empty() {
            let module = &modules[0];
            let base = module.addr() as *const u8;
            let size = module.size() as usize;
            let archive = unsafe { core::slice::from_raw_parts(base, size) };

            // Parse CPIO to find bin/loom
            match vfs::cpio::parse_cpio(archive) {
                Ok(entries) => {
                    let mut found = false;
                    for entry in &entries {
                        let name = entry.name;
                        // Match "bin/loom" or "./bin/loom"
                        let clean = if name.starts_with(b"./") { &name[2..] } else { name };
                        if clean == b"bin/loom" && !entry.is_directory {
                            serial_println!("[LOOM] Found bin/loom ({} bytes)", entry.data.len());
                            found = true;
                            match process::spawn_elf(
                                fabric_types::ProcessId::BUTLER,
                                entry.data,
                                "loom",
                            ) {
                                Ok(pid) => {
                                    serial_println!("[LOOM] Spawned Loom as pid:{}", pid.0);
                                }
                                Err(e) => {
                                    serial_println!("[LOOM] Failed to spawn: {:?}", e);
                                }
                            }
                            break;
                        }
                    }
                    if !found {
                        serial_println!("[LOOM] bin/loom not found in initramfs");
                    }
                }
                Err(e) => {
                    serial_println!("[LOOM] Failed to parse initramfs: {:?}", e);
                }
            }
        } else {
            serial_println!("[LOOM] No modules loaded");
        }
    } else {
        serial_println!("[LOOM] No module response from bootloader");
    }

    // Re-initialize APIC timer for Loom scheduling
    x86::apic::send_eoi();
    x86::apic::start_timer(0x20000);

    // Enable interrupts and trigger timer to kickstart scheduling
    unsafe { core::arch::asm!("sti"); }

    // Brief spin to let APIC timer fire naturally
    for _ in 0..500_000 {
        core::hint::spin_loop();
    }

    // Fallback: software INT 32 to force schedule if timer hasn't fired
    unsafe { core::arch::asm!("int 32"); }

    serial_println!("[FABRIC] Entering idle loop (tick={}).",
        x86::idt::tick_count());

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

    // Clean up for STRESS
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

fn phase9_self_test() {
    use crate::network::addr::{Ipv4Addr, SocketAddr};

    // Test 1: Loopback address check
    assert!(Ipv4Addr::LOOPBACK.is_loopback(), "127.0.0.1 should be loopback");
    serial_println!("[PHASE9] Self-test: loopback address — OK");

    // Test 2: Ring buffer basic operation
    {
        let mut rb = crate::network::buffer::RingBuffer::new();
        assert!(rb.is_empty());
        let written = rb.write(b"test");
        assert_eq!(written, 4);
        let mut buf = [0u8; 4];
        let read = rb.read(&mut buf);
        assert_eq!(read, 4);
        assert_eq!(&buf, b"test");
    }
    serial_println!("[PHASE9] Self-test: ring buffer — OK");

    // Test 3: Internet checksum
    {
        // RFC 1071 example: checksum of all-zeros should work
        let data = [0u8; 20];
        let cksum = crate::network::checksum::internet_checksum(&data);
        // Checksum of all zeros is 0xFFFF (ones complement of zero)
        assert_eq!(cksum, 0xFFFF);
    }
    serial_println!("[PHASE9] Self-test: internet checksum — OK");

    // Test 4: Socket table available
    {
        let table = crate::network::SOCKETS.lock();
        // Should be empty at init
        // (count might not be 0 if STRESS tests ran, but the table should be accessible)
        let _ = table.count();
    }
    serial_println!("[PHASE9] Self-test: socket table accessible — OK");

    serial_println!("[PHASE9] All Phase 9 self-tests passed");
}

fn phase10_self_test() {
    // Test 1: Display state initialized
    {
        let disp = display::DISPLAY.lock();
        assert!(disp.is_some(), "Display should be initialized");
    }
    serial_println!("[PHASE10] Self-test: display initialized — OK");

    // Test 2: Framebuffer dimensions valid
    {
        let disp = display::DISPLAY.lock();
        if let Some(ref ds) = *disp {
            assert!(ds.fb.width > 0, "Framebuffer width should be > 0");
            assert!(ds.fb.height > 0, "Framebuffer height should be > 0");
            assert!(ds.fb.bpp >= 24, "Framebuffer bpp should be >= 24");
        }
    }
    serial_println!("[PHASE10] Self-test: framebuffer valid — OK");

    // Test 3: Surface allocated
    {
        let disp = display::DISPLAY.lock();
        if let Some(ref ds) = *disp {
            assert!(ds.surface.width > 0, "Surface width should be > 0");
            assert!(ds.surface.buffer.len() > 0, "Surface buffer should be allocated");
        }
    }
    serial_println!("[PHASE10] Self-test: surface allocated — OK");

    // Test 4: Color encoding
    {
        let disp = display::DISPLAY.lock();
        if let Some(ref ds) = *disp {
            let packed = display::Color::WHITE.to_packed(&ds.fb);
            assert!(packed != 0, "White should not be zero");
        }
    }
    serial_println!("[PHASE10] Self-test: color encoding — OK");

    serial_println!("[PHASE10] All Phase 10 self-tests passed");
}

fn halt() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
