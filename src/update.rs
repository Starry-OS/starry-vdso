use core::sync::atomic::Ordering;

use axplat::time::NANOS_PER_SEC;

use crate::{config::ClockMode, vdso_time_data::VdsoClock};

/// vDSO timestamp structure
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VdsoTimestamp {
    /// Seconds
    pub sec: u64,
    /// Nanoseconds
    pub nsec: u64,
}

impl VdsoTimestamp {
    /// Create a new zero timestamp
    pub const fn new() -> Self {
        Self { sec: 0, nsec: 0 }
    }
}

/// Update vDSO clock.
pub fn update_vdso_clock(
    clk: &mut VdsoClock,
    cycle_now: u64,
    wall_ns: u64,
    mono_ns: u64,
    mult_shift: (u32, u32),
) {
    let prev_cycle = clk.cycle_last.load(Ordering::Relaxed);
    let prev_basetime_ns = clk.time_data[1]
        .sec
        .wrapping_mul(NANOS_PER_SEC)
        .wrapping_add(clk.time_data[1].nsec);

    // Check if this is a counter-based clock mode (non-None)
    let is_counter_mode = clk.clock_mode != (ClockMode::None as i32);

    if is_counter_mode {
        // Counter-based modes: Tsc (x86_64), Csr (riscv64/loongarch64), Cntvct
        // (aarch64)
        if prev_cycle == 0 {
            let (mult, shift) = mult_shift;
            clk.mult = mult;
            clk.shift = shift;
            clk.time_data[1].sec = mono_ns / NANOS_PER_SEC;
            clk.time_data[1].nsec = (mono_ns % NANOS_PER_SEC) << shift;
            clk.cycle_last.store(cycle_now, Ordering::Relaxed);
        } else {
            let (mult, shift) = mult_shift;
            if !(mult == u32::MAX && shift == 0) {
                clk.mult = mult;
                clk.shift = shift;
                clk.time_data[1].sec = mono_ns / NANOS_PER_SEC;
                clk.time_data[1].nsec = (mono_ns % NANOS_PER_SEC) << shift;
                clk.cycle_last.store(cycle_now, Ordering::Relaxed);
            } else {
                let delta_cycles = (cycle_now.wrapping_sub(prev_cycle)) & clk.mask;
                let delta_ns = mono_ns.saturating_sub(prev_basetime_ns);
                if delta_cycles != 0 && delta_ns > 0 {
                    let (mult, shift) = clocks_calc_mult_shift(delta_cycles, delta_ns, 1);
                    clk.mult = mult;
                    clk.shift = shift;
                    clk.time_data[1].sec = mono_ns / NANOS_PER_SEC;
                    clk.time_data[1].nsec = (mono_ns % NANOS_PER_SEC) << shift;
                    clk.cycle_last.store(cycle_now, Ordering::Relaxed);
                }
            }
        }
    } else {
        // ClockMode::None - No cycle->ns conversion; store direct monotonic ns.
        clk.mult = 0;
        clk.time_data[1].sec = mono_ns / NANOS_PER_SEC;
        clk.time_data[1].nsec = mono_ns % NANOS_PER_SEC;
        clk.cycle_last.store(0, Ordering::Relaxed);
    }

    // Update realtime and boottime entries.
    let shift = clk.shift;
    clk.time_data[0].sec = wall_ns / NANOS_PER_SEC;
    clk.time_data[0].nsec = (wall_ns % NANOS_PER_SEC) << shift;
    clk.time_data[7].sec = clk.time_data[1].sec;
    clk.time_data[7].nsec = clk.time_data[1].nsec;

    if clk.seq.load(Ordering::Relaxed) < 10 {
        let cycle_val = clk.cycle_last.load(Ordering::Relaxed);
        log::trace!(
            "vDSO update: seq={}, cycle_last={}, mono_ns={}, mult={}, shift={}",
            clk.seq.load(Ordering::Relaxed),
            cycle_val,
            mono_ns,
            clk.mult,
            clk.shift
        );
    }
}

/// Compute multiplier and shift to convert from timer_frequency to
/// nanos_per_sec.
pub fn clocks_calc_mult_shift(from: u64, to: u64, maxsec: u32) -> (u32, u32) {
    // sftacc starts at 32 and is reduced based on the maximum conversion range
    let mut tmp = ((maxsec as u64).wrapping_mul(from)) >> 32;
    let mut sftacc: i32 = 32;
    while tmp != 0 {
        tmp >>= 1;
        sftacc -= 1;
    }

    // Try shifts from 32 down to 1 and pick the first that fits the range
    for sft in (1..=32).rev() {
        // compute tmp = (to << sft) / from with rounding
        let mut numer = (to as u128) << sft;
        numer += (from as u128) / 2u128;
        let tmp128 = numer / (from as u128);

        // If tmp128 can be represented within the allowed shift range, select it
        if sftacc <= 0 || (tmp128 >> (sftacc as u128)) == 0u128 {
            let mult = if tmp128 > (u32::MAX as u128) {
                u32::MAX
            } else {
                tmp128 as u32
            };
            return (mult, sft as u32);
        }
    }
    // Fallback: return maximum multiplier with shift 0
    (u32::MAX, 0)
}
