use axplat::time::{
    NANOS_PER_SEC, current_ticks, monotonic_time_nanos, nanos_to_ticks, wall_time_nanos,
};

use super::config::ClockMode;
use crate::update::{VdsoClock, clocks_calc_mult_shift, update_vdso_clock};

#[repr(C)]
#[repr(align(4096))]
pub struct VdsoData {
    pub _pad: [u8; 128],
    pub clocks: [VdsoClock; 2],
    pub tz_minuteswest: i32,
    pub tz_dsttime: i32,
    pub hrtimer_res: u32,
}

#[cfg(target_arch = "x86_64")]
impl Default for VdsoData {
    fn default() -> Self {
        Self::new()
    }
}

impl VdsoData {
    pub const fn new() -> Self {
        Self {
            _pad: [0; 128],
            clocks: [VdsoClock::new(), VdsoClock::new()],
            tz_minuteswest: 0,
            tz_dsttime: 0,
            hrtimer_res: 1,
        }
    }

    pub fn update(&mut self) {
        let cycle_now = current_ticks();

        let wall_ns = wall_time_nanos();
        let mono_ns = monotonic_time_nanos();

        let ticks_per_sec = nanos_to_ticks(NANOS_PER_SEC);
        let mult_shift = clocks_calc_mult_shift(ticks_per_sec, NANOS_PER_SEC, 10);

        for clk in self.clocks.iter_mut() {
            clk.write_seqcount_begin();

            clk.clock_mode = self::ClockMode::Tsc as i32;
            clk.mask = u64::MAX;
            update_vdso_clock(clk, cycle_now, wall_ns, mono_ns, mult_shift);

            clk.write_seqcount_end();
        }
    }
}
