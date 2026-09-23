use core::time::Duration;
use x86_64::instructions::port::Port;
use spin::Once;

/// The calibrated TSC frequency in MHz (cycles per microsecond).
static TSC_MHZ: Once<u64> = Once::new();

/// Calibrates the TSC frequency using the PIT.
///
/// This should be called once during kernel initialization.
pub fn init() {
    TSC_MHZ.call_once(|| unsafe { calibrate_tsc() });
}

/// Measures the TSC frequency using multiple methods.
///
/// Prioritizes CPUID leaf 0x15 (instant and accurate on modern CPUs) over PIT.
unsafe fn calibrate_tsc() -> u64 {
    crate::serial::write_str("TSC: calibrating...\n");

    // Check maximum CPUID leaf.
    let max_leaf = core::arch::x86_64::__cpuid(0x00).eax;

    // Method 1: CPUID Leaf 0x15 (Intel)
    // EAX = Denominator, EBX = Numerator, ECX = Crystal Hz.
    if max_leaf >= 0x15 {
        let res = core::arch::x86_64::__cpuid(0x15);

        if res.eax > 0 && res.ebx > 0 && res.ecx > 0 {
            let mhz =
                (res.ecx as u64 * res.ebx as u64 / res.eax as u64) / 1_000_000;

            if mhz > 0 {
                crate::serial::write_str(
                    "TSC: calibrated via CPUID 0x15: ",
                );
                crate::serial::write_hex(mhz);
                crate::serial::write_str(" MHz\n");
                return mhz;
            }
        }
    }

    crate::serial::write_str(
        "TSC: WARNING: calibration failed! Using 3GHz default.\n",
    );
    3000
}

unsafe fn calibrate_tsc_pit() -> u64 {
    let mut pit_command = Port::<u8>::new(0x43);
    let mut pit_data = Port::<u8>::new(0x42);
    let mut system_control = Port::<u8>::new(0x61);

    let gate_bits = system_control.read();

    if gate_bits == 0xFF {
        return 0;
    }

    let pit_ticks = 11932u16;

    pit_command.write(0xB0);
    pit_data.write((pit_ticks & 0xFF) as u8);
    pit_data.write((pit_ticks >> 8) as u8);

    system_control.write((gate_bits & 0xFD) | 0x01);

    let start_tsc = core::arch::x86_64::_rdtsc();

    let mut timeout = 100_000_000u32;

    while (system_control.read() & 0x20) == 0 && timeout > 0 {
        timeout -= 1;
        core::hint::spin_loop();
    }

    let end_tsc = core::arch::x86_64::_rdtsc();

    system_control.write(gate_bits);

    let elapsed_tsc = end_tsc.wrapping_sub(start_tsc);

    if elapsed_tsc < 100_000 || timeout == 0 {
        return 0;
    }

    elapsed_tsc / 10_000
}

#[inline(always)]
fn get_tsc_mhz() -> u64 {
    *TSC_MHZ.get().unwrap_or(&3000)
}

/// Returns the calibrated TSC-derived timestamp in microseconds.
///
/// The value is intended for monotonic deadline comparisons inside the
/// kernel.  `init()` should be called during early kernel initialization.
#[inline(always)]
pub fn now_us() -> u64 {
    let mhz = get_tsc_mhz().max(1);
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };

    tsc / mhz
}

/// Busy-waits for the requested duration using the calibrated TSC.
pub fn delay(duration: Duration) {
    let mhz = get_tsc_mhz();

    let secs_part = duration
        .as_secs()
        .saturating_mul(mhz)
        .saturating_mul(1_000_000);

    let nanos_part =
        (duration.subsec_nanos() as u64).saturating_mul(mhz) / 1_000;

    let cycles = secs_part.saturating_add(nanos_part);

    if cycles == 0 {
        return;
    }

    let start = unsafe { core::arch::x86_64::_rdtsc() };

    while unsafe {
        core::arch::x86_64::_rdtsc()
    }
    .wrapping_sub(start)
        < cycles
    {
        core::hint::spin_loop();
    }
}

#[inline(always)]
pub fn delay_seconds(seconds: u64) {
    delay(Duration::from_secs(seconds));
}

#[inline(always)]
pub fn delay_ms(ms: u64) {
    delay(Duration::from_millis(ms));
}

#[inline(always)]
pub fn delay_us(us: u64) {
    delay(Duration::from_micros(us));
}
