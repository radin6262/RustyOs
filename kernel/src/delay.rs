use core::time::Duration;


pub fn delay_seconds(seconds: u64) {
    // Approximate delay using the CPU's TSC.
    // 3 GHz is used as a rough estimate.
    let start = unsafe { core::arch::x86_64::_rdtsc() };
    let cycles = seconds.saturating_mul(3_000_000_000);

    while unsafe { core::arch::x86_64::_rdtsc() }.wrapping_sub(start) < cycles {
        core::hint::spin_loop();
    }
}
pub fn delay(duration: Duration) {
    let nanos = duration.as_nanos();

    // Rusty's current TSC calibration is approximately 3 GHz.
    let cycles = nanos
        .saturating_mul(3_000_000_000u128)
        / 1_000_000_000u128;

    let cycles = cycles.min(u64::MAX as u128) as u64;

    let start =
        unsafe {
            core::arch::x86_64::_rdtsc()
        };

    while unsafe {
        core::arch::x86_64::_rdtsc()
    }
        .wrapping_sub(start) < cycles {
        core::hint::spin_loop();
    }
}