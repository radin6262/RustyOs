use core::time::Duration;
use x86_64::instructions::port::Port;
use spin::Once;

/// The calibrated TSC frequency in MHz (cycles per microsecond).
static TSC_MHZ: Once<u64> = Once::new();

/// Calibrates the TSC frequency using the PIT.
/// This should be called once during kernel initialization.
pub fn init() {
    TSC_MHZ.call_once(|| {
        unsafe { calibrate_tsc() }
    });
}

/// Measures the TSC frequency using multiple methods.
/// Prioritizes CPUID (instant and accurate on modern CPUs) over PIT.
unsafe fn calibrate_tsc() -> u64 {
    crate::serial::write_str("TSC: calibrating...\n");

    // Check maximum CPUID leaf
    let max_leaf = core::arch::x86_64::__cpuid(0x00).eax;

    // Method 1: CPUID Leaf 0x15 (Intel)
    // EAX = Denominator, EBX = Numerator, ECX = Crystal Hz
    if max_leaf >= 0x15 {
        let res = core::arch::x86_64::__cpuid(0x15);
        if res.eax > 0 && res.ebx > 0 && res.ecx > 0 {
            let mhz = (res.ecx as u64 * res.ebx as u64 / res.eax as u64) / 1_000_000;
            crate::serial::write_str("TSC: calibrated via CPUID 0x15: ");
            crate::serial::write_hex(mhz);
            crate::serial::write_str(" MHz\n");
            return mhz;
        }
    }
    // If all calibration fails, we must provide a non-zero value to avoid division by zero.
    // However, we log a warning.
    crate::serial::write_str("TSC: WARNING: calibration failed! Using 3GHz default.\n");
    3000
}

/// Measures the TSC frequency using PIT Channel 2.
/// Returns 0 if PIT is missing or unresponsive (e.g. port 0x61 returns 0xFF).
unsafe fn calibrate_tsc_pit() -> u64 {
    let mut pit_command = Port::<u8>::new(0x43);
    let mut pit_data = Port::<u8>::new(0x42);
    let mut system_control = Port::<u8>::new(0x61);

    // Get the current System Control value and check if it's 0xFF (no PIT)
    let gate_bits = system_control.read();
    if gate_bits == 0xFF {
        return 0;
    }

    // We want to wait for 10ms (11932 PIT ticks at 1.193182 MHz)
    let pit_ticks = 11932u16;

    // Set PIT Channel 2 to Mode 0 (Interrupt on Terminal Count), LSB/MSB
    pit_command.write(0xB0);
    pit_data.write((pit_ticks & 0xFF) as u8);
    pit_data.write((pit_ticks >> 8) as u8);

    // Ensure PIT Channel 2 gate is high (Bit 0) and speaker is disabled (Bit 1)
    system_control.write((gate_bits & 0xFD) | 0x01);

    // Read TSC at the start
    let start_tsc = core::arch::x86_64::_rdtsc();

    // Poll PIT Status (Bit 5 of 0x61) until it goes high (Terminal Count reached)
    // We add a timeout loop to prevent infinite hangs if the bit never changes.
    let mut timeout = 100_000_000;
    while (system_control.read() & 0x20) == 0 && timeout > 0 {
        timeout -= 1;
        core::hint::spin_loop();
    }

    // Read TSC at the end
    let end_tsc = core::arch::x86_64::_rdtsc();

    // Restore original System Control state
    system_control.write(gate_bits);

    let elapsed_tsc = end_tsc.wrapping_sub(start_tsc);

    // If calibration finished too fast (e.g. PIT bit was already high), return 0 to trigger fallback
    // 10ms at 100MHz is 1,000,000 cycles. If it took < 100,000, it's a failure.
    if elapsed_tsc < 100_000 || timeout == 0 {
        return 0;
    }

    // We waited 10ms. Frequency in MHz = cycles / 10,000 microseconds.
    elapsed_tsc / 10_000
}

/// Returns the calibrated TSC frequency in MHz.
fn get_tsc_mhz() -> u64 {
    *TSC_MHZ.get().unwrap_or(&3000)
}

/// Delays execution for the given Duration using the calibrated TSC.
pub fn delay(duration: Duration) {
    let mhz = get_tsc_mhz();
    
    // Calculate total cycles needed
    // cycles = (seconds * MHz * 10^6) + (nanos * MHz / 10^3)
    let secs_part = duration.as_secs().saturating_mul(mhz).saturating_mul(1_000_000);
    let nanos_part = (duration.subsec_nanos() as u64).saturating_mul(mhz) / 1_000;
    
    let cycles = secs_part.saturating_add(nanos_part);
    
    if cycles == 0 { return; }

    let start = unsafe { core::arch::x86_64::_rdtsc() };
    while unsafe { core::arch::x86_64::_rdtsc() }.wrapping_sub(start) < cycles {
        core::hint::spin_loop();
    }
}

/// Delays execution for the given number of seconds.
pub fn delay_seconds(seconds: u64) {
    delay(Duration::from_secs(seconds));
}

/// Delays execution for the given number of milliseconds.
pub fn delay_ms(ms: u64) {
    delay(Duration::from_millis(ms));
}

/// Delays execution for the given number of microseconds.
pub fn delay_us(us: u64) {
    delay(Duration::from_micros(us));
}
