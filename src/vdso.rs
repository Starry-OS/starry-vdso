//! vDSO data management.
extern crate alloc;
extern crate log;
use alloc::{alloc::alloc_zeroed, vec::Vec};
use core::alloc::Layout;

use axerrno::{AxError, AxResult};
use axplat::{mem::virt_to_phys, time::monotonic_time_nanos};
use kernel_elf_parser::{AuxEntry, AuxType};
use log::{info, warn};
use memory_addr::{MemoryAddr, PAGE_SIZE_4K};

/// Global vDSO data instance
#[unsafe(link_section = ".data")]
pub static mut VDSO_DATA: crate::vdso_data::VdsoData = crate::vdso_data::VdsoData::new();

/// Initialize vDSO data
pub fn init_vdso_data() {
    unsafe {
        let data_ptr = core::ptr::addr_of_mut!(VDSO_DATA);
        (*data_ptr).time_update();
        info!("vDSO data initialized at {:#x}", data_ptr as usize);
        #[cfg(target_arch = "aarch64")]
        {
            crate::vdso_data::enable_cntvct_access();
            info!("vDSO CNTVCT access enabled");
        }
        #[cfg(target_arch = "x86_64")]
        {
            (*data_ptr).enable_pvclock();
        }
    }
}

/// Update vDSO data
pub fn update_vdso_data() {
    unsafe {
        let data_ptr = core::ptr::addr_of_mut!(VDSO_DATA);
        (*data_ptr).time_update();
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

/// Load vDSO into the given user address space and update auxv accordingly.
pub fn load_vdso_data<F1, F2, F3>(auxv: &mut Vec<AuxEntry>, f1: F1, f2: F2, f3: F3) -> AxResult<()>
where
    F1: FnOnce(usize, axplat::mem::PhysAddr, usize) -> AxResult<()>,
    F2: FnOnce(usize, usize) -> AxResult<()>,
    F3: FnMut(
        usize,
        axplat::mem::PhysAddr,
        usize,
        &xmas_elf::program::ProgramHeader64,
    ) -> AxResult<()>,
{
    unsafe extern "C" {
        static vdso_start: u8;
        static vdso_end: u8;
    }
    let (vdso_kstart, vdso_kend) = unsafe {
        (
            &vdso_start as *const u8 as usize,
            &vdso_end as *const u8 as usize,
        )
    };
    info!("vdso_kstart: {vdso_kstart:#x}, vdso_kend: {vdso_kend:#x}");

    if vdso_kend <= vdso_kstart {
        warn!(
            "vDSO binary is missing or invalid: vdso_kstart={vdso_kstart:#x}, \
             vdso_kend={vdso_kend:#x}. vDSO will not be loaded and AT_SYSINFO_EHDR will not be \
             set."
        );
        return Err(AxError::InvalidExecutable);
    }

    let (vdso_paddr_page, vdso_bytes, vdso_size, vdso_page_offset, alloc_info) =
        prepare_vdso_pages(vdso_kstart, vdso_kend).map_err(|_| AxError::InvalidExecutable)?;

    let mut alloc_guard = crate::guard::VdsoAllocGuard::new(alloc_info);

    let (_base_addr, vdso_user_addr) =
        calculate_vdso_aslr_addr(vdso_kstart, vdso_kend, vdso_page_offset);

    match kernel_elf_parser::ELFHeadersBuilder::new(vdso_bytes).and_then(|b| {
        let range = b.ph_range();
        b.build(&vdso_bytes[range.start as usize..range.end as usize])
    }) {
        Ok(headers) => {
            map_vdso_segments(
                headers,
                vdso_user_addr,
                vdso_paddr_page,
                vdso_page_offset,
                f3,
            )?;
            alloc_guard.disarm();
        }
        Err(_) => {
            info!("vDSO ELF parsing failed, using fallback mapping");
            let map_user_start = if vdso_page_offset == 0 {
                vdso_user_addr
            } else {
                vdso_user_addr - vdso_page_offset
            };
            f1(map_user_start, vdso_paddr_page, vdso_size)?;
            alloc_guard.disarm();
        }
    }

    map_vvar_and_push_aux(auxv, vdso_user_addr, f2)?;

    Ok(())
}

fn map_vvar_and_push_aux<F>(auxv: &mut Vec<AuxEntry>, vdso_user_addr: usize, f: F) -> AxResult<()>
where
    F: FnOnce(usize, usize) -> AxResult<()>,
{
    use crate::config::VVAR_PAGES;
    let vvar_user_addr = vdso_user_addr - VVAR_PAGES * PAGE_SIZE_4K;
    let vvar_paddr = vdso_data_paddr();

    f(vvar_user_addr, vvar_paddr)?;

    info!(
        "Mapped vvar pages at user {:#x}..{:#x} -> paddr {:#x}",
        vvar_user_addr,
        vvar_user_addr + VVAR_PAGES * PAGE_SIZE_4K,
        vvar_paddr,
    );

    let aux_entry = AuxEntry::new(AuxType::SYSINFO_EHDR, vdso_user_addr);
    auxv.push(aux_entry);

    Ok(())
}

fn map_vdso_segments<F>(
    headers: kernel_elf_parser::ELFHeaders,
    vdso_user_addr: usize,
    vdso_paddr_page: axplat::mem::PhysAddr,
    vdso_page_offset: usize,
    mut f: F,
) -> AxResult<()>
where
    F: FnMut(
        usize,
        axplat::mem::PhysAddr,
        usize,
        &xmas_elf::program::ProgramHeader64,
    ) -> AxResult<()>,
{
    info!("vDSO ELF parsed successfully, mapping segments");
    for ph in headers
        .ph
        .iter()
        .filter(|ph| ph.get_type() == Ok(xmas_elf::program::Type::Load))
    {
        let vaddr = ph.virtual_addr as usize;
        let seg_pad = vaddr.align_offset_4k() + vdso_page_offset;
        let seg_align_size =
            (ph.mem_size as usize + seg_pad + PAGE_SIZE_4K - 1) & !(PAGE_SIZE_4K - 1);

        let map_base_user = vdso_user_addr & !(PAGE_SIZE_4K - 1);
        let seg_user_start = map_base_user + vaddr.align_down_4k();
        let seg_paddr = vdso_paddr_page + vaddr.align_down_4k();

        f(seg_user_start, seg_paddr, seg_align_size, ph)?;
    }
    Ok(())
}
