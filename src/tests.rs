//! System tests for threading and other core functionality
//!
//! Run with `tests::run_all()` after scheduler initialization.
//! If tests fail, the kernel should halt.

use crate::console;
use crate::threading;
use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;

/// Run all system tests - returns true if all pass
pub fn run_all() -> bool {
    console::print("\n========== System Tests ==========\n");

    let mut all_pass = true;

    // Allocator tests (run first - fundamental)
    all_pass &= test_allocator_vec();
    all_pass &= test_allocator_box();
    all_pass &= test_allocator_large();

    // Threading tests
    all_pass &= test_scheduler_init();
    all_pass &= test_thread_stats();
    all_pass &= test_yield();
    all_pass &= test_cooperative_timeout();
    all_pass &= test_thread_cleanup();
    all_pass &= test_spawn_thread();
    all_pass &= test_spawn_and_run();
    all_pass &= test_spawn_and_cleanup();
    all_pass &= test_spawn_multiple();
    all_pass &= test_spawn_and_yield();
    all_pass &= test_spawn_cooperative();
    all_pass &= test_yield_cycle();
    all_pass &= test_mixed_cooperative_preemptible();

    console::print("\n==================================\n");
    console::print(&format!(
        "Overall: {}\n",
        if all_pass {
            "ALL TESTS PASSED"
        } else {
            "SOME TESTS FAILED"
        }
    ));
    console::print("==================================\n\n");

    all_pass
}

// ============================================================================
// Allocator Tests
// ============================================================================

/// Test: Vec allocation and basic operations
fn test_allocator_vec() -> bool {
    console::print("\n[TEST] Allocator Vec operations\n");

    // Create and populate a vector
    let mut test_vec: Vec<u32> = Vec::new();
    for i in 0..10 {
        test_vec.push(i);
    }

    // Test basic operations
    let len_ok = test_vec.len() == 10;
    console::print(&format!("  Vec length: {} (expect 10)\n", test_vec.len()));

    // Test remove and insert
    test_vec.remove(0);
    test_vec.insert(0, 99);
    let first_ok = test_vec[0] == 99;
    console::print(&format!("  First element: {} (expect 99)\n", test_vec[0]));

    // Test drop (implicit when vec goes out of scope)
    drop(test_vec);
    console::print("  Drop completed\n");

    let ok = len_ok && first_ok;
    console::print(&format!("  Result: {}\n", if ok { "PASS" } else { "FAIL" }));
    ok
}

/// Test: Box allocation
fn test_allocator_box() -> bool {
    console::print("\n[TEST] Allocator Box operations\n");

    // Allocate a boxed value
    let boxed: Box<u64> = Box::new(42);
    let val_ok = *boxed == 42;
    console::print(&format!("  Box value: {} (expect 42)\n", *boxed));

    // Allocate a boxed array
    let boxed_arr: Box<[u8; 256]> = Box::new([0xAB; 256]);
    let arr_ok = boxed_arr[0] == 0xAB && boxed_arr[255] == 0xAB;
    console::print(&format!(
        "  Box array: first=0x{:02X}, last=0x{:02X} (expect 0xAB)\n",
        boxed_arr[0], boxed_arr[255]
    ));

    drop(boxed);
    drop(boxed_arr);
    console::print("  Drop completed\n");

    let ok = val_ok && arr_ok;
    console::print(&format!("  Result: {}\n", if ok { "PASS" } else { "FAIL" }));
    ok
}

/// Test: Large allocation
fn test_allocator_large() -> bool {
    console::print("\n[TEST] Allocator large allocation\n");

    // Allocate 1MB
    const SIZE: usize = 1024 * 1024;
    console::print(&format!("  Allocating {} KB...", SIZE / 1024));

    let mut large_vec: Vec<u8> = Vec::with_capacity(SIZE);
    for _ in 0..SIZE {
        large_vec.push(0);
    }
    console::print(" done\n");

    let len_ok = large_vec.len() == SIZE;
    console::print(&format!("  Size: {} bytes\n", large_vec.len()));

    // Write and verify
    large_vec[0] = 0x12;
    large_vec[SIZE - 1] = 0x34;
    let write_ok = large_vec[0] == 0x12 && large_vec[SIZE - 1] == 0x34;
    console::print(&format!(
        "  First: 0x{:02X}, Last: 0x{:02X}\n",
        large_vec[0],
        large_vec[SIZE - 1]
    ));

    drop(large_vec);
    console::print("  Drop completed\n");

    let ok = len_ok && write_ok;
    console::print(&format!("  Result: {}\n", if ok { "PASS" } else { "FAIL" }));
    ok
}

// ============================================================================
// Threading Tests
// ============================================================================

/// Test: Scheduler is initialized
fn test_scheduler_init() -> bool {
    console::print("\n[TEST] Scheduler initialization\n");

    let count = threading::thread_count();
    let ok = count >= 1; // At least idle thread

    console::print(&format!("  Thread count: {} (expect >= 1)\n", count));
    console::print(&format!("  Result: {}\n", if ok { "PASS" } else { "FAIL" }));

    ok
}

/// Test: Thread stats work correctly
fn test_thread_stats() -> bool {
    console::print("\n[TEST] Thread statistics\n");

    let (ready, running, terminated) = threading::thread_stats();
    let ok = running >= 1; // Current thread should be running

    console::print(&format!(
        "  Ready: {}, Running: {}, Terminated: {}\n",
        ready, running, terminated
    ));
    console::print(&format!("  Result: {}\n", if ok { "PASS" } else { "FAIL" }));

    ok
}

/// Test: yield_now() works without crashing
fn test_yield() -> bool {
    console::print("\n[TEST] Yield operation\n");

    console::print("  Calling yield_now()...");
    threading::yield_now();
    console::print(" returned\n");
    console::print("  Result: PASS\n");

    true
}

/// Test: Cooperative timeout constant is set
fn test_cooperative_timeout() -> bool {
    console::print("\n[TEST] Cooperative timeout\n");

    let timeout = threading::COOPERATIVE_TIMEOUT_US;
    let ok = timeout > 0;

    console::print(&format!(
        "  Timeout: {} us ({} seconds)\n",
        timeout,
        timeout / 1_000_000
    ));
    console::print(&format!(
        "  Result: {}\n",
        if ok { "PASS" } else { "DISABLED (0)" }
    ));

    ok
}

/// Test: Cleanup function exists and doesn't crash
fn test_thread_cleanup() -> bool {
    console::print("\n[TEST] Thread cleanup\n");

    // Get initial state
    let count_before = threading::thread_count();
    let (ready, running, terminated) = threading::thread_stats();
    console::print(&format!(
        "  State: {} threads (R:{} U:{} T:{})\n",
        count_before, ready, running, terminated
    ));

    // Run cleanup (should be safe even with no terminated threads)
    let cleaned = threading::cleanup_terminated();
    console::print(&format!("  Cleaned: {} threads\n", cleaned));

    // Verify state is still valid
    let count_after = threading::thread_count();
    let (ready2, running2, terminated2) = threading::thread_stats();
    console::print(&format!(
        "  After: {} threads (R:{} U:{} T:{})\n",
        count_after, ready2, running2, terminated2
    ));

    // Test passes if:
    // 1. Count decreased by amount cleaned (or stayed same if 0 cleaned)
    // 2. At least one thread still exists (idle)
    let count_ok = count_after == count_before - cleaned;
    let has_idle = count_after >= 1;
    let ok = count_ok && has_idle;

    console::print(&format!("  Result: {}\n", if ok { "PASS" } else { "FAIL" }));

    ok
}

// Global flag for test thread communication
static mut TEST_THREAD_RAN: bool = false;

fn set_test_flag(val: bool) {
    unsafe {
        let ptr = core::ptr::addr_of_mut!(TEST_THREAD_RAN);
        core::ptr::write_volatile(ptr, val);
    }
}

fn get_test_flag() -> bool {
    unsafe {
        let ptr = core::ptr::addr_of!(TEST_THREAD_RAN);
        core::ptr::read_volatile(ptr)
    }
}

/// Test: Can spawn a thread without hanging
fn test_spawn_thread() -> bool {
    console::print("\n[TEST] Thread spawn\n");

    let count_before = threading::thread_count();
    console::print(&format!("  Threads before: {}\n", count_before));

    // Simple thread that just marks itself terminated immediately
    extern "C" fn test_thread_immediate() -> ! {
        threading::mark_current_terminated();
        loop {
            threading::yield_now();
            unsafe { core::arch::asm!("wfi") };
        }
    }

    // Try to spawn
    console::print("  Spawning test thread...");
    match threading::spawn(test_thread_immediate) {
        Ok(tid) => {
            console::print(&format!(" OK (tid={})\n", tid));

            let count_after = threading::thread_count();
            console::print(&format!("  Threads after: {}\n", count_after));

            let ok = count_after == count_before + 1;
            console::print(&format!("  Result: {}\n", if ok { "PASS" } else { "FAIL" }));
            ok
        }
        Err(e) => {
            console::print(&format!(" FAILED: {}\n", e));
            console::print("  Result: FAIL\n");
            false
        }
    }
}

/// Test: Spawned thread actually executes
fn test_spawn_and_run() -> bool {
    console::print("\n[TEST] Thread execution\n");

    // Reset flag
    set_test_flag(false);

    // Thread that sets the flag and terminates
    extern "C" fn test_thread_sets_flag() -> ! {
        set_test_flag(true);
        threading::mark_current_terminated();
        loop {
            threading::yield_now();
            unsafe { core::arch::asm!("wfi") };
        }
    }

    console::print("  Spawning thread that sets flag...");
    match threading::spawn(test_thread_sets_flag) {
        Ok(tid) => {
            console::print(&format!(" OK (tid={})\n", tid));

            // Yield a few times to let the thread run
            console::print("  Yielding to let thread run...");
            for _ in 0..10 {
                threading::yield_now();
            }
            console::print(" done\n");

            // Check if flag was set
            let ran = get_test_flag();
            console::print(&format!("  Thread ran: {}\n", ran));

            // Cleanup
            let cleaned = threading::cleanup_terminated();
            console::print(&format!("  Cleaned up: {} threads\n", cleaned));

            console::print(&format!(
                "  Result: {}\n",
                if ran { "PASS" } else { "FAIL" }
            ));
            ran
        }
        Err(e) => {
            console::print(&format!(" FAILED: {}\n", e));
            console::print("  Result: FAIL\n");
            false
        }
    }
}

/// Test: Spawn, terminate, cleanup, verify count returns to original
fn test_spawn_and_cleanup() -> bool {
    console::print("\n[TEST] Spawn and cleanup\n");

    let count_before = threading::thread_count();
    console::print(&format!("  Threads before: {}\n", count_before));

    extern "C" fn test_thread_terminates() -> ! {
        threading::mark_current_terminated();
        loop {
            threading::yield_now();
            unsafe { core::arch::asm!("wfi") };
        }
    }

    // Spawn thread
    console::print("  Spawning...");
    let tid = match threading::spawn(test_thread_terminates) {
        Ok(t) => {
            console::print(&format!(" tid={}\n", t));
            t
        }
        Err(e) => {
            console::print(&format!(" FAILED: {}\n", e));
            return false;
        }
    };

    // Yield to let it run and terminate
    console::print("  Yielding...");
    for _ in 0..5 {
        threading::yield_now();
    }
    console::print(" done\n");

    // Check it's terminated
    let (_, _, terminated) = threading::thread_stats();
    console::print(&format!("  Terminated count: {}\n", terminated));

    // Cleanup
    let cleaned = threading::cleanup_terminated();
    console::print(&format!("  Cleaned: {}\n", cleaned));

    let count_after = threading::thread_count();
    console::print(&format!("  Threads after: {}\n", count_after));

    let ok = count_after == count_before && cleaned >= 1;
    console::print(&format!("  Result: {}\n", if ok { "PASS" } else { "FAIL" }));
    ok
}

// Counter for multiple thread test
static mut THREAD_COUNTER: u32 = 0;

fn increment_counter() {
    unsafe {
        let ptr = core::ptr::addr_of_mut!(THREAD_COUNTER);
        let val = core::ptr::read_volatile(ptr);
        core::ptr::write_volatile(ptr, val + 1);
    }
}

fn get_counter() -> u32 {
    unsafe {
        let ptr = core::ptr::addr_of!(THREAD_COUNTER);
        core::ptr::read_volatile(ptr)
    }
}

fn reset_counter() {
    unsafe {
        let ptr = core::ptr::addr_of_mut!(THREAD_COUNTER);
        core::ptr::write_volatile(ptr, 0);
    }
}

/// Test: Spawn multiple threads
fn test_spawn_multiple() -> bool {
    console::print("\n[TEST] Spawn multiple threads\n");

    reset_counter();
    let count_before = threading::thread_count();
    console::print(&format!("  Threads before: {}\n", count_before));

    extern "C" fn counter_thread() -> ! {
        increment_counter();
        threading::mark_current_terminated();
        loop {
            threading::yield_now();
            unsafe { core::arch::asm!("wfi") };
        }
    }

    // Spawn 3 threads
    const NUM_THREADS: usize = 3;
    console::print(&format!("  Spawning {} threads...", NUM_THREADS));

    for i in 0..NUM_THREADS {
        match threading::spawn(counter_thread) {
            Ok(_) => {}
            Err(e) => {
                console::print(&format!(" FAILED at {}: {}\n", i, e));
                return false;
            }
        }
    }
    console::print(" done\n");

    let count_mid = threading::thread_count();
    console::print(&format!("  Threads after spawn: {}\n", count_mid));

    // Yield to let them all run
    console::print("  Yielding...");
    for _ in 0..20 {
        threading::yield_now();
    }
    console::print(" done\n");

    let counter_val = get_counter();
    console::print(&format!(
        "  Counter value: {} (expect {})\n",
        counter_val, NUM_THREADS
    ));

    // Cleanup
    let cleaned = threading::cleanup_terminated();
    console::print(&format!("  Cleaned: {}\n", cleaned));

    let count_after = threading::thread_count();
    console::print(&format!("  Threads after cleanup: {}\n", count_after));

    let ok = counter_val == NUM_THREADS as u32 && count_after == count_before;
    console::print(&format!("  Result: {}\n", if ok { "PASS" } else { "FAIL" }));
    ok
}

// Yield counter for yield test
static mut YIELD_COUNT: u32 = 0;

fn increment_yield_count() {
    unsafe {
        let ptr = core::ptr::addr_of_mut!(YIELD_COUNT);
        let val = core::ptr::read_volatile(ptr);
        core::ptr::write_volatile(ptr, val + 1);
    }
}

fn get_yield_count() -> u32 {
    unsafe {
        let ptr = core::ptr::addr_of!(YIELD_COUNT);
        core::ptr::read_volatile(ptr)
    }
}

fn reset_yield_count() {
    unsafe {
        let ptr = core::ptr::addr_of_mut!(YIELD_COUNT);
        core::ptr::write_volatile(ptr, 0);
    }
}

/// Test: Thread that yields multiple times
fn test_spawn_and_yield() -> bool {
    console::print("\n[TEST] Thread with multiple yields\n");

    reset_yield_count();

    extern "C" fn yielding_thread() -> ! {
        // Yield 5 times, incrementing counter each time
        for _ in 0..5 {
            increment_yield_count();
            threading::yield_now();
        }
        threading::mark_current_terminated();
        loop {
            threading::yield_now();
            unsafe { core::arch::asm!("wfi") };
        }
    }

    console::print("  Spawning yielding thread...");
    match threading::spawn(yielding_thread) {
        Ok(tid) => console::print(&format!(" tid={}\n", tid)),
        Err(e) => {
            console::print(&format!(" FAILED: {}\n", e));
            return false;
        }
    }

    // Yield many times to let thread complete
    console::print("  Running scheduler...");
    for _ in 0..20 {
        threading::yield_now();
    }
    console::print(" done\n");

    let count = get_yield_count();
    console::print(&format!("  Yield count: {} (expect 5)\n", count));

    // Cleanup
    threading::cleanup_terminated();

    let ok = count == 5;
    console::print(&format!("  Result: {}\n", if ok { "PASS" } else { "FAIL" }));
    ok
}

/// Test: Cooperative thread spawning
fn test_spawn_cooperative() -> bool {
    console::print("\n[TEST] Cooperative thread spawn\n");

    set_test_flag(false);

    extern "C" fn coop_thread() -> ! {
        set_test_flag(true);
        threading::mark_current_terminated();
        loop {
            threading::yield_now();
            unsafe { core::arch::asm!("wfi") };
        }
    }

    console::print("  Spawning cooperative thread...");
    match threading::spawn_cooperative(coop_thread) {
        Ok(tid) => console::print(&format!(" tid={}\n", tid)),
        Err(e) => {
            console::print(&format!(" FAILED: {}\n", e));
            return false;
        }
    }

    // Yield to let it run
    console::print("  Yielding...");
    for _ in 0..10 {
        threading::yield_now();
    }
    console::print(" done\n");

    let ran = get_test_flag();
    console::print(&format!("  Thread ran: {}\n", ran));

    // Cleanup
    threading::cleanup_terminated();

    console::print(&format!(
        "  Result: {}\n",
        if ran { "PASS" } else { "FAIL" }
    ));
    ran
}

// Yield cycle counter
static mut YIELD_CYCLE_COUNT: u32 = 0;

fn increment_yield_cycle() {
    unsafe {
        let ptr = core::ptr::addr_of_mut!(YIELD_CYCLE_COUNT);
        let val = core::ptr::read_volatile(ptr);
        core::ptr::write_volatile(ptr, val + 1);
    }
}

fn get_yield_cycle() -> u32 {
    unsafe {
        let ptr = core::ptr::addr_of!(YIELD_CYCLE_COUNT);
        core::ptr::read_volatile(ptr)
    }
}

fn reset_yield_cycle() {
    unsafe {
        let ptr = core::ptr::addr_of_mut!(YIELD_CYCLE_COUNT);
        core::ptr::write_volatile(ptr, 0);
    }
}

/// Test: Thread can yield and resume multiple times in sequence
fn test_yield_cycle() -> bool {
    console::print("\n[TEST] Yield-resume cycle\n");

    reset_yield_cycle();

    const CYCLES: u32 = 10;

    extern "C" fn cycle_thread() -> ! {
        // Perform multiple yield-resume cycles
        for _ in 0..CYCLES {
            increment_yield_cycle();
            threading::yield_now();
        }
        threading::mark_current_terminated();
        loop {
            threading::yield_now();
            unsafe { core::arch::asm!("wfi") };
        }
    }

    console::print(&format!("  Spawning thread for {} yield cycles...", CYCLES));
    match threading::spawn(cycle_thread) {
        Ok(tid) => console::print(&format!(" tid={}\n", tid)),
        Err(e) => {
            console::print(&format!(" FAILED: {}\n", e));
            return false;
        }
    }

    // Each cycle requires 2 yields (one from worker, one from main)
    // Plus extra to ensure completion
    console::print("  Running yield cycles...");
    for i in 0..(CYCLES * 2 + 10) {
        threading::yield_now();
        if i % 5 == 0 {
            console::print(".");
        }
    }
    console::print(" done\n");

    let cycles = get_yield_cycle();
    console::print(&format!(
        "  Completed cycles: {} (expect {})\n",
        cycles, CYCLES
    ));

    // Cleanup
    let cleaned = threading::cleanup_terminated();
    console::print(&format!("  Cleaned: {} threads\n", cleaned));

    let ok = cycles == CYCLES;
    console::print(&format!("  Result: {}\n", if ok { "PASS" } else { "FAIL" }));
    ok
}

// Flags for mixed thread test
static mut COOP_THREAD_DONE: bool = false;
static mut PREEMPT_THREAD_DONE: bool = false;

fn set_coop_done(val: bool) {
    unsafe {
        let ptr = core::ptr::addr_of_mut!(COOP_THREAD_DONE);
        core::ptr::write_volatile(ptr, val);
    }
}

fn get_coop_done() -> bool {
    unsafe {
        let ptr = core::ptr::addr_of!(COOP_THREAD_DONE);
        core::ptr::read_volatile(ptr)
    }
}

fn set_preempt_done(val: bool) {
    unsafe {
        let ptr = core::ptr::addr_of_mut!(PREEMPT_THREAD_DONE);
        core::ptr::write_volatile(ptr, val);
    }
}

fn get_preempt_done() -> bool {
    unsafe {
        let ptr = core::ptr::addr_of!(PREEMPT_THREAD_DONE);
        core::ptr::read_volatile(ptr)
    }
}

/// Test: Mixed cooperative and preemptible threads
/// - 1 cooperative thread: yields for 5ms then exits
/// - 1 preemptible thread: loops for 15ms then exits  
/// - Verify both complete and only idle thread remains after cleanup
fn test_mixed_cooperative_preemptible() -> bool {
    console::print("\n[TEST] Mixed cooperative & preemptible threads\n");

    set_coop_done(false);
    set_preempt_done(false);

    let count_before = threading::thread_count();
    console::print(&format!("  Threads before: {}\n", count_before));

    // Cooperative thread: yields for ~5ms total
    extern "C" fn cooperative_5ms_thread() -> ! {
        let start = crate::timer::uptime_us();
        let target = 5_000; // 5ms

        while crate::timer::uptime_us() - start < target {
            threading::yield_now();
        }

        set_coop_done(true);
        threading::mark_current_terminated();
        loop {
            threading::yield_now();
            unsafe { core::arch::asm!("wfi") };
        }
    }

    // Preemptible thread: busy-loops for ~15ms
    extern "C" fn preemptible_15ms_thread() -> ! {
        let start = crate::timer::uptime_us();
        let target = 15_000; // 15ms

        // Busy loop - will be preempted by timer
        while crate::timer::uptime_us() - start < target {
            // Just spin
            unsafe { core::arch::asm!("nop") };
        }

        set_preempt_done(true);
        threading::mark_current_terminated();
        loop {
            threading::yield_now();
            unsafe { core::arch::asm!("wfi") };
        }
    }

    // Spawn cooperative thread
    console::print("  Spawning cooperative thread (5ms)...");
    match threading::spawn_cooperative(cooperative_5ms_thread) {
        Ok(tid) => console::print(&format!(" tid={}\n", tid)),
        Err(e) => {
            console::print(&format!(" FAILED: {}\n", e));
            return false;
        }
    }

    // Spawn preemptible thread
    console::print("  Spawning preemptible thread (15ms)...");
    match threading::spawn(preemptible_15ms_thread) {
        Ok(tid) => console::print(&format!(" tid={}\n", tid)),
        Err(e) => {
            console::print(&format!(" FAILED: {}\n", e));
            return false;
        }
    }

    let count_mid = threading::thread_count();
    console::print(&format!("  Threads after spawn: {}\n", count_mid));

    // Wait for both to complete (max 30ms with some margin)
    console::print("  Waiting for threads to complete...");
    let wait_start = crate::timer::uptime_us();
    let max_wait = 50_000; // 50ms max

    while (!get_coop_done() || !get_preempt_done())
        && (crate::timer::uptime_us() - wait_start < max_wait)
    {
        threading::yield_now();
    }

    let elapsed = (crate::timer::uptime_us() - wait_start) / 1000;
    console::print(&format!(" {}ms\n", elapsed));

    // Check completion
    let coop_done = get_coop_done();
    let preempt_done = get_preempt_done();
    console::print(&format!("  Cooperative done: {}\n", coop_done));
    console::print(&format!("  Preemptible done: {}\n", preempt_done));

    // Cleanup
    let cleaned = threading::cleanup_terminated();
    console::print(&format!("  Cleaned: {} threads\n", cleaned));

    let count_after = threading::thread_count();
    console::print(&format!("  Threads after cleanup: {}\n", count_after));

    // Verify: both threads completed and only idle remains
    let ok = coop_done && preempt_done && count_after == 1;
    console::print(&format!("  Result: {}\n", if ok { "PASS" } else { "FAIL" }));
    ok
}
