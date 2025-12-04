use axplat::time::{
    NANOS_PER_SEC, current_ticks, monotonic_time_nanos, nanos_to_ticks, wall_time_nanos,
};

use super::config::ClockMode;
use crate::update::{VdsoClock, clocks_calc_mult_shift, update_vdso_clock};

#[repr(C)]
#[repr(align(4096))]
pub struct VdsoData {
    pub clock_page0: VdsoClock,
    pub _unused: [u8; 1648],
    pub tz_minuteswest: i32,
    pub tz_dsttime: i32,
    pub hrtimer_res: u32,
    pub _pad: [u8; 4096 - 1956],
    pub clock_page1: VdsoClock,
}

impl Default for VdsoData {
    fn default() -> Self {
        Self::new()
    }
}

impl VdsoData {
    pub const fn new() -> Self {
        Self {
            clock_page0: VdsoClock::new(),
            _unused: [0; 1648],
            tz_minuteswest: 0,
            tz_dsttime: 0,
            hrtimer_res: 1,
            _pad: [0; 4096 - 1956],
            clock_page1: VdsoClock::new(),
        }
    }

    pub fn update(&mut self) {
        let wall_ns = wall_time_nanos();
        let mono_ns = monotonic_time_nanos();

        let ticks_per_sec = nanos_to_ticks(NANOS_PER_SEC);
        let mult_shift = clocks_calc_mult_shift(ticks_per_sec, NANOS_PER_SEC, 10);
        let cycle_now = current_ticks();

        self.clock_page0.write_seqcount_begin();
        self.clock_page0.clock_mode = ClockMode::Cntvct as i32;
        self.clock_page0.mask = u64::MAX;
        update_vdso_clock(
            &mut self.clock_page0,
            cycle_now,
            wall_ns,
            mono_ns,
            mult_shift,
        );
        self.clock_page0.write_seqcount_end();

        self.clock_page1.write_seqcount_begin();
        self.clock_page1.clock_mode = ClockMode::Cntvct as i32;
        self.clock_page1.mask = u64::MAX;
        update_vdso_clock(
            &mut self.clock_page1,
            cycle_now,
            wall_ns,
            mono_ns,
            mult_shift,
        );
        self.clock_page1.write_seqcount_end();
    }
}
