use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use axplat::time::{
    NANOS_PER_SEC, current_ticks, monotonic_time_nanos, nanos_to_ticks, wall_time_nanos,
};

use crate::update::{VdsoTimestamp, clocks_calc_mult_shift, update_vdso_clock};

const VDSO_BASES: usize = 12;

#[repr(C)]
pub struct VdsoClock {
    pub seq: AtomicU32,
    pub clock_mode: i32,
    pub cycle_last: AtomicU64,
    #[cfg(target_arch = "x86_64")]
    pub max_cycles: u64,
    pub mask: u64,
    pub mult: u32,
    pub shift: u32,
    pub time_data: [VdsoTimestamp; VDSO_BASES],
    pub _unused: u32,
}

impl VdsoClock {
    /// Create a new VdsoClock with default values.
    pub const fn new() -> Self {
        Self {
            seq: AtomicU32::new(0),
            clock_mode: 1,
            cycle_last: AtomicU64::new(0),
            // only for x86 because CONFIG_GENERIC_VDSO_OVERFLOW_PROTECT
            #[cfg(target_arch = "x86_64")]
            max_cycles: 0,

            mask: u64::MAX,
            mult: 0,
            shift: 32,
            time_data: [VdsoTimestamp::new(); VDSO_BASES],
            _unused: 0,
        }
    }

    pub fn write_seqcount_begin(&self) {
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq.wrapping_add(1), Ordering::Release);
        core::sync::atomic::fence(Ordering::SeqCst);
    }

    pub fn write_seqcount_end(&self) {
        core::sync::atomic::fence(Ordering::SeqCst);
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq.wrapping_add(1), Ordering::Release);
    }
}

#[repr(C)]
#[repr(align(4096))]
pub struct VdsoTimeData {
    pub clock_data: [VdsoClock; 2],
    pub tz_minuteswest: i32,
    pub tz_dsttime: i32,
    pub hrtimer_res: u32,
    pub __unused: u32,
}

impl Default for VdsoTimeData {
    fn default() -> Self {
        Self::new()
    }
}

impl VdsoTimeData {
    pub const fn new() -> Self {
        Self {
            clock_data: [VdsoClock::new(), VdsoClock::new()],
            tz_minuteswest: 0,
            tz_dsttime: 0,
            hrtimer_res: 1,
            __unused: 0,
        }
    }

    pub fn update(&mut self) {
        let cycle_now = current_ticks();
        let wall_ns = wall_time_nanos();
        let mono_ns = monotonic_time_nanos();
        let ticks_per_sec = nanos_to_ticks(NANOS_PER_SEC);
        let mult_shift = clocks_calc_mult_shift(ticks_per_sec, NANOS_PER_SEC, 10);

        for clk in self.clock_data.iter_mut() {
            clk.write_seqcount_begin();
            update_vdso_clock(clk, cycle_now, wall_ns, mono_ns, mult_shift);
            clk.write_seqcount_end();
        }
    }
}
