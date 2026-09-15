pub fn delay_seconds(seconds: u64) {
    // Approximate delay using the CPU's TSC.
    // 3 GHz is used as a rough estimate.
    let start = unsafe { core::arch::x86_64::_rdtsc() };
    let cycles = seconds.saturating_mul(3_000_000_000);

    while unsafe { core::arch::x86_64::_rdtsc() }.wrapping_sub(start) < cycles {
        core::hint::spin_loop();
    }
}