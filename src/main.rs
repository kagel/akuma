#![no_std]
#![no_main]

extern crate alloc;

mod allocator;
mod boot;
mod console;
mod exceptions;
mod executor;
mod gic;
mod irq;
mod network;
mod tests;
mod threading;
mod timer;
mod virtio_hal;

use alloc::string::ToString;

use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    console::print("\n\n!!! PANIC !!!\n");
    if let Some(location) = info.location() {
        console::print("Location: ");
        console::print(location.file());
        console::print(":");
        console::print(&location.line().to_string());
        console::print("\n");
    }
    console::print("Message: ");
    console::print(&alloc::format!("{}\n", info.message()));
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_start(_dtb_ptr: usize) -> ! {
    const RAM_BASE: usize = 0x40000000;

    // DTB pointer workaround: QEMU with -device loader puts DTB at 0x44000000
    // But we can't safely read it yet before setting up, so use after heap init

    // let ram_size = match detect_memory_size(dtb_ptr) {
    //     Ok(size) => {
    //         console::print("Detected RAM: ");
    //         console::print(&(size / 1024 / 1024).to_string());
    //         console::print(" MB\n");
    //         size
    //     }
    //     Err(e) => {
    //         console::print("Error detecting memory: ");
    //         console::print(e);
    //         console::print("\nUsing default 32 MB\n");
    //         32 * 1024 * 1024
    //     }
    // };

    let ram_size = 128 * 1024 * 1024; // 128 MB

    let code_and_stack = ram_size / 16; // 1/16 of total RAM
    let heap_start = RAM_BASE + code_and_stack;

    let heap_size = if ram_size > code_and_stack {
        ram_size - code_and_stack
    } else {
        console::print("Not enough RAM for heap\n");
        loop {}
    };

    if let Err(e) = allocator::init(heap_start, heap_size) {
        console::print("Allocator init failed: ");
        console::print(e);
        console::print("\n");
        loop {}
    }

    console::print("Heap initialized: ");
    console::print(&(heap_size / 1024 / 1024).to_string());
    console::print(" MB\n");

    // Initialize GIC (Generic Interrupt Controller)
    gic::init();
    console::print("GIC initialized\n");

    // Set up exception vectors and enable IRQs
    exceptions::init();
    console::print("IRQ handling enabled\n");

    // Skip executor - using threads instead
    // executor::init();

    // Initialize timer
    timer::init();
    console::print("Timer initialized\n");

    // Check timer hardware
    let freq = timer::read_frequency();
    console::print("Timer frequency: ");
    console::print(&freq.to_string());
    console::print(" Hz\n");

    // Read UTC time from PL031 RTC hardware
    if timer::init_utc_from_rtc() {
        console::print("UTC time initialized from RTC\n");
    } else {
        console::print("Warning: RTC not available, UTC time not set\n");
    }

    console::print("Current UTC time: ");
    console::print(&timer::utc_iso8601());
    console::print("\n");

    console::print("Uptime: ");
    console::print(&(timer::uptime_us() / 1_000_000).to_string());
    console::print(" seconds\n");

    // Initialize threading (IRQs still enabled, timer not yet configured)
    console::print("Initializing threading...\n");
    threading::init();
    console::print("Threading system initialized\n");

    // Enable timer-driven preemptive multitasking via SGI
    // 1. Enable SGI 0 for scheduling (SGIs are always enabled, but register a dummy handler)
    console::print("Configuring scheduler SGI...\n");
    gic::enable_irq(gic::SGI_SCHEDULER);

    // 2. Timer IRQ (PPI 14, maps to IRQ 30) will trigger the SGI
    console::print("Registering timer IRQ...\n");
    irq::register_handler(30, |irq| timer::timer_irq_handler(irq));

    console::print("Enabling timer...\n");
    timer::enable_timer_interrupts(10_000); // 10ms intervals
    console::print("Preemptive scheduling enabled (10ms timer -> SGI)\n");

    // Run system tests (includes allocator tests)
    if !tests::run_all() {
        console::print("\n!!! SYSTEM TESTS FAILED - HALTING !!!\n");
        loop {
            unsafe {
                core::arch::asm!("wfi");
            }
        }
    }

    // Network thread - cooperative, yields when appropriate
    extern "C" fn network_thread() -> ! {
        console::print("[Net] Starting...\n");

        match network::init(0) {
            Ok(()) => console::print("[Net] Initialized\n"),
            Err(e) => console::print(&alloc::format!("[Net] Failed: {}\n", e)),
        }

        // Network main loop - poll and yield
        loop {
            network::poll();
            threading::yield_now();
        }
    }

    // Spawn network as cooperative thread
    console::print("Spawning network thread (cooperative)...\n");
    threading::spawn_cooperative(network_thread).expect("Failed to spawn network thread");
    console::print("Network thread spawned\n");

    // Thread 0 becomes the idle loop - yield continuously
    console::print("[Idle] Entering idle loop\n");
    loop {
        threading::yield_now();
    }
}
