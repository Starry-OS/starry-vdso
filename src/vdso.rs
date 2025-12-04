//! vDSO data management.
extern crate alloc;
extern crate log;
use alloc::alloc::alloc_zeroed;
use core::{
    alloc::Layout,
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
};

use axerrno::{AxError, AxResult};
use axplat::{mem::virt_to_phys, time::monotonic_time_nanos};

const PAGE_SIZE_4K: usize = 4096;

/// Number of clock bases
const VDSO_BASES: usize = 16;

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

/// Global vDSO data instance
#[unsafe(link_section = ".data")]
pub static mut VDSO_DATA: crate::vdso_data::VdsoData = crate::vdso_data::VdsoData::new();

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

/// Information about loaded vDSO pages for userspace mapping and auxv update.
pub type VdsoPageInfo = (
    axplat::mem::PhysAddr,
    &'static [u8],
    usize,
    usize,
    Option<(usize, usize)>,
);

/// Load vDSO into the given user address space and update auxv accordingly.
pub fn prepare_vdso_pages(vdso_kstart: usize, vdso_kend: usize) -> AxResult<VdsoPageInfo> {
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
