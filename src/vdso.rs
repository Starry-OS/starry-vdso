//! vDSO data management.
extern crate alloc;
extern crate log;
use alloc::alloc::{alloc_zeroed, dealloc};
use core::{
    alloc::Layout,
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
};

use axerrno::{AxError, AxResult};
use axplat::{
    mem::virt_to_phys,
    time::{NANOS_PER_SEC, current_ticks, monotonic_time_nanos, nanos_to_ticks, wall_time_nanos},
};

use crate::config::ClockMode;

const PAGE_SIZE_4K: usize = 4096;

/// Number of clock bases
const VDSO_BASES: usize = 16;

/// Compute multiplier and shift to convert from timer_frequency to nanos_per_sec.
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

#[repr(C)]
#[derive(Default)]
pub struct VdsoClock {
    pub seq: AtomicU32,
    pub clock_mode: i32,
    pub cycle_last: AtomicU64,
    pub mask: u64,
    pub mult: u32,
    pub shift: u32,
    pub basetime: [VdsoTimestamp; VDSO_BASES],
    pub _unused: u32,
}

impl VdsoClock {
    /// Create a new VdsoClock with default values.
    pub const fn new() -> Self {
        Self {
            seq: AtomicU32::new(0),
            clock_mode: 1,
            cycle_last: AtomicU64::new(0),
            mask: u64::MAX,
            mult: 0,
            shift: 32,
            basetime: [VdsoTimestamp::new(); VDSO_BASES],
            _unused: 0,
        }
    }

    pub(crate) fn write_seqcount_begin(&self) {
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq.wrapping_add(1), Ordering::Release);
        core::sync::atomic::fence(Ordering::SeqCst);
    }

    pub(crate) fn write_seqcount_end(&self) {
        core::sync::atomic::fence(Ordering::SeqCst);
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq.wrapping_add(1), Ordering::Release);
    }
}

// Architecture-specific VdsoData definitions

#[cfg(target_arch = "x86_64")]
#[repr(C)]
#[repr(align(4096))]
pub struct VdsoData {
    pub _pad: [u8; 128],
    pub clocks: [VdsoClock; 2],
    pub tz_minuteswest: i32,
    pub tz_dsttime: i32,
    pub hrtimer_res: u32,
}

#[cfg(target_arch = "aarch64")]
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

#[cfg(any(target_arch = "riscv64", target_arch = "loongarch64"))]
#[repr(C)]
#[repr(align(4096))]
#[derive(Default)]
pub struct VdsoData {
    pub clocks: [VdsoClock; 2],
    pub tz_minuteswest: i32,
    pub tz_dsttime: i32,
    pub hrtimer_res: u32,
}

impl VdsoData {
    /// Create a new VdsoData with default values.
    #[cfg(any(target_arch = "riscv64", target_arch = "loongarch64"))]
    pub const fn new() -> Self {
        Self {
            clocks: [VdsoClock::new(), VdsoClock::new()],
            tz_minuteswest: 0,
            tz_dsttime: 0,
            hrtimer_res: 1,
        }
    }

    #[cfg(target_arch = "x86_64")]
    pub const fn new() -> Self {
        Self {
            _pad: [0; 128],
            clocks: [VdsoClock::new(), VdsoClock::new()],
            tz_minuteswest: 0,
            tz_dsttime: 0,
            hrtimer_res: 1,
        }
    }

    #[cfg(target_arch = "aarch64")]
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

    /// Update vDSO clocks and basetimes.
    pub fn update(&mut self) {
        #[cfg(any(target_arch = "loongarch64", target_arch = "riscv64"))]
        {
            update_vdso(self);
            return;
        }
        #[cfg(target_arch = "x86_64")]
        {
            update_vdso_x86(self);
            return;
        }
        #[cfg(target_arch = "aarch64")]
        {
            update_vdso_aarch(self);
            return;
        }
    }
}

#[cfg(any(target_arch = "loongarch64", target_arch = "riscv64"))]
/// Update vDSO data
pub fn update_vdso(data: &mut VdsoData) {
    let cycle_now = current_ticks();
    let wall_ns = wall_time_nanos();
    let mono_ns = monotonic_time_nanos();

    let ticks_per_sec = nanos_to_ticks(NANOS_PER_SEC);
    let mult_shift = clocks_calc_mult_shift(ticks_per_sec, NANOS_PER_SEC, 10);

    for clk in &mut data.clocks {
        clk.write_seqcount_begin();
        update_vdso_clock(clk, cycle_now, wall_ns, mono_ns, mult_shift);
        clk.write_seqcount_end();
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
    let prev_basetime_ns = clk.basetime[1]
        .sec
        .wrapping_mul(NANOS_PER_SEC)
        .wrapping_add(clk.basetime[1].nsec);

    // Check if this is a counter-based clock mode (non-None)
    let is_counter_mode = clk.clock_mode != (ClockMode::None as i32);

    if is_counter_mode {
        // Counter-based modes: Tsc (x86_64), Csr (riscv64/loongarch64), Cntvct (aarch64)
        if prev_cycle == 0 {
            let (mult, shift) = mult_shift;
            clk.mult = mult;
            clk.shift = shift;
            clk.basetime[1].sec = mono_ns / NANOS_PER_SEC;
            clk.basetime[1].nsec = (mono_ns % NANOS_PER_SEC) << shift;
            clk.cycle_last.store(cycle_now, Ordering::Relaxed);
        } else {
            let (mult, shift) = mult_shift;
            if !(mult == u32::MAX && shift == 0) {
                clk.mult = mult;
                clk.shift = shift;
                clk.basetime[1].sec = mono_ns / NANOS_PER_SEC;
                clk.basetime[1].nsec = (mono_ns % NANOS_PER_SEC) << shift;
                clk.cycle_last.store(cycle_now, Ordering::Relaxed);
            } else {
                let delta_cycles = (cycle_now.wrapping_sub(prev_cycle)) & clk.mask;
                let delta_ns = mono_ns.saturating_sub(prev_basetime_ns);
                if delta_cycles != 0 && delta_ns > 0 {
                    let (mult, shift) = clocks_calc_mult_shift(delta_cycles, delta_ns, 1);
                    clk.mult = mult;
                    clk.shift = shift;
                    clk.basetime[1].sec = mono_ns / NANOS_PER_SEC;
                    clk.basetime[1].nsec = (mono_ns % NANOS_PER_SEC) << shift;
                    clk.cycle_last.store(cycle_now, Ordering::Relaxed);
                }
            }
        }
    } else {
        // ClockMode::None - No cycle->ns conversion; store direct monotonic ns.
        clk.mult = 0;
        clk.basetime[1].sec = mono_ns / NANOS_PER_SEC;
        clk.basetime[1].nsec = mono_ns % NANOS_PER_SEC;
        clk.cycle_last.store(0, Ordering::Relaxed);
    }

    // Update realtime and boottime entries.
    let shift = clk.shift;
    clk.basetime[0].sec = wall_ns / NANOS_PER_SEC;
    clk.basetime[0].nsec = (wall_ns % NANOS_PER_SEC) << shift;
    clk.basetime[7].sec = clk.basetime[1].sec;
    clk.basetime[7].nsec = clk.basetime[1].nsec;

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

/// Global vDSO data instance
#[unsafe(link_section = ".data")]
pub static mut VDSO_DATA: VdsoData = VdsoData::new();

/// Initialize vDSO data
pub fn init_vdso_data() {
    unsafe {
        let data_ptr = core::ptr::addr_of_mut!(VDSO_DATA);
        (*data_ptr).update();
        log::info!("vDSO data initialized at {:#x}", data_ptr as usize);
    }
}

/// Update vDSO data
pub fn update_vdso_data() {
    unsafe {
        let data_ptr = core::ptr::addr_of_mut!(VDSO_DATA);
        (*data_ptr).update();
    }
}

/// Get the physical address of vDSO data for mapping to userspace
pub fn vdso_data_paddr() -> usize {
    let data_ptr = core::ptr::addr_of!(VDSO_DATA) as usize;
    virt_to_phys(data_ptr.into()).into()
}

/// Load vDSO into the given user address space and update auxv accordingly.
pub fn prepare_vdso_pages(
    vdso_kstart: usize,
    vdso_kend: usize,
) -> AxResult<(
    axplat::mem::PhysAddr,
    &'static [u8],
    usize,
    usize,
    Option<(usize, usize)>,
)> {
    let orig_vdso_len = vdso_kend - vdso_kstart;
    let orig_page_off = vdso_kstart & (PAGE_SIZE_4K - 1);

    if orig_page_off == 0 {
        // Already page aligned: use original memory region directly.
        let vdso_paddr_page = virt_to_phys(vdso_kstart.into());
        let vdso_size = (vdso_kend - vdso_kstart + PAGE_SIZE_4K - 1) & !(PAGE_SIZE_4K - 1);
        let vdso_bytes =
            unsafe { core::slice::from_raw_parts(vdso_kstart as *const u8, orig_vdso_len) };
        Ok((vdso_paddr_page, vdso_bytes, vdso_size, 0usize, None))
    } else {
        let total_size = orig_vdso_len + orig_page_off;
        let num_pages = total_size.div_ceil(PAGE_SIZE_4K);
        let vdso_size = num_pages * PAGE_SIZE_4K;

        let layout = match Layout::from_size_align(vdso_size, PAGE_SIZE_4K) {
            Ok(l) => l,
            Err(_) => return Err(AxError::InvalidExecutable),
        };
        let alloc_ptr = unsafe { alloc_zeroed(layout) };
        if alloc_ptr.is_null() {
            return Err(AxError::InvalidExecutable);
        }
        // destination start where vdso_start should reside
        let dest = unsafe { alloc_ptr.add(orig_page_off) };
        let src = vdso_kstart as *const u8;
        unsafe { core::ptr::copy_nonoverlapping(src, dest, orig_vdso_len) };
        let alloc_vaddr = alloc_ptr as usize;
        let vdso_paddr_page = virt_to_phys(alloc_vaddr.into());
        let vdso_bytes = unsafe { core::slice::from_raw_parts(dest as *const u8, orig_vdso_len) };
        Ok((
            vdso_paddr_page,
            vdso_bytes,
            vdso_size,
            orig_page_off,
            Some((alloc_vaddr, num_pages)),
        ))
    }
}

/// RAII guard that will free allocated vdso pages on Drop unless disarmed.
pub struct VdsoAllocGuard {
    alloc: Option<(usize, usize)>,
}

impl VdsoAllocGuard {
    pub fn new(alloc: Option<(usize, usize)>) -> Self {
        Self { alloc }
    }

    pub fn disarm(&mut self) {
        self.alloc = None;
    }
}

impl Drop for VdsoAllocGuard {
    fn drop(&mut self) {
        if let Some((vaddr, pages)) = self.alloc {
            // free memory allocated with `alloc_zeroed` above
            let size = pages * PAGE_SIZE_4K;
            if let Ok(layout) = Layout::from_size_align(size, PAGE_SIZE_4K) {
                unsafe { dealloc(vaddr as *mut u8, layout) };
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
/// Update vDSO data for x86_64
pub fn update_vdso_x86(data: &mut VdsoData) {
    let cycle_now = current_ticks();

    let wall_ns = wall_time_nanos();
    let mono_ns = monotonic_time_nanos();

    let ticks_per_sec = nanos_to_ticks(NANOS_PER_SEC);
    let mult_shift = clocks_calc_mult_shift(ticks_per_sec, NANOS_PER_SEC, 10);

    for (_i, clk) in data.clocks.iter_mut().enumerate() {
        clk.write_seqcount_begin();

        clk.clock_mode = ClockMode::Tsc as i32;
        clk.mask = u64::MAX;
        update_vdso_clock(clk, cycle_now, wall_ns, mono_ns, mult_shift);

        clk.write_seqcount_end();
    }
}

#[cfg(target_arch = "aarch64")]
/// Update vDSO data for aarch64
pub fn update_vdso_aarch(data: &mut VdsoData) {
    let wall_ns = wall_time_nanos();
    let mono_ns = monotonic_time_nanos();

    let ticks_per_sec = nanos_to_ticks(NANOS_PER_SEC);
    let mult_shift = clocks_calc_mult_shift(ticks_per_sec, NANOS_PER_SEC, 10);
    let cycle_now = current_ticks();

    data.clock_page0.write_seqcount_begin();
    data.clock_page0.clock_mode = ClockMode::Cntvct as i32;
    data.clock_page0.mask = u64::MAX;
    update_vdso_clock(
        &mut data.clock_page0,
        cycle_now,
        wall_ns,
        mono_ns,
        mult_shift,
    );
    data.clock_page0.write_seqcount_end();

    data.clock_page1.write_seqcount_begin();
    data.clock_page1.clock_mode = ClockMode::Cntvct as i32;
    data.clock_page1.mask = u64::MAX;
    update_vdso_clock(
        &mut data.clock_page1,
        cycle_now,
        wall_ns,
        mono_ns,
        mult_shift,
    );
    data.clock_page1.write_seqcount_end();
}

#[cfg(target_arch = "aarch64")]
pub fn enable_cntvct_access() {
    log::info!("Enabling user-space access to timer counter registers...");
    unsafe {
        let mut cntkctl_el1: u64;
        core::arch::asm!("mrs {}, CNTKCTL_EL1", out(reg) cntkctl_el1);

        cntkctl_el1 |= 0x3;

        core::arch::asm!("msr CNTKCTL_EL1, {}", in(reg) cntkctl_el1);
        core::arch::asm!("isb");

        log::info!("CNTKCTL_EL1 configured: {:#x}", cntkctl_el1);
    }
}

/// Calculate ASLR-randomized vDSO user address
pub fn calculate_vdso_aslr_addr(
    vdso_kstart: usize,
    vdso_kend: usize,
    vdso_page_offset: usize,
) -> (usize, usize) {
    use rand_core::RngCore;
    use rand_pcg::Pcg64Mcg;

    const VDSO_USER_ADDR_BASE: usize = 0x7f00_0000;
    const VDSO_ASLR_PAGES: usize = 256;

    let seed: u128 = (monotonic_time_nanos() as u128)
        ^ ((vdso_kstart as u128).rotate_left(13))
        ^ ((vdso_kend as u128).rotate_left(37));
    let mut rng = Pcg64Mcg::new(seed);
    let page_off: usize = (rng.next_u64() as usize) % VDSO_ASLR_PAGES;
    let base_addr = VDSO_USER_ADDR_BASE + page_off * PAGE_SIZE_4K;
    let vdso_addr = if vdso_page_offset != 0 {
        base_addr.wrapping_add(vdso_page_offset)
    } else {
        base_addr
    };

    (base_addr, vdso_addr)
}
