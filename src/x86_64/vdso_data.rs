use crate::{
    config::ClockMode,
    vdso_time_data::VdsoTimeData,
    x86_64::{config::PVCLOCK_MAX_CPUS, pvclock_data::PvClockTimeInfo},
};

#[repr(C)]
#[repr(align(4096))]
pub struct VdsoData {
    pub time_data: VdsoTimeData,
    pub pvclock: [PvClockTimeInfo; PVCLOCK_MAX_CPUS],
}

impl Default for VdsoData {
    fn default() -> Self {
        Self::new()
    }
}

impl VdsoData {
    pub const fn new() -> Self {
        Self {
            time_data: VdsoTimeData::new(),
            pvclock: [PvClockTimeInfo::new(); PVCLOCK_MAX_CPUS],
        }
    }

    pub fn time_update(&mut self) {
        self.time_data.update();
    }

    /// Enable pvclock support.
    pub fn enable_pvclock(&mut self) {
        if !detect_kvm_clock() {
            log::warn!("KVM clock not supported by Hypervisor, skipping pvclock registration");
            return;
        }
        register_pvclock(0);
        self.time_data.set_pvclock_mode();
        log::info!("vDSO pvclock support enabled");
    }
}

fn detect_kvm_clock() -> bool {
    let (max_leaf, ebx, ecx, edx) = cpuid(0x40000000);
    let sig = [ebx.to_le_bytes(), ecx.to_le_bytes(), edx.to_le_bytes()];
    // Safe because we constructed it from bytes
    let sig_str = unsafe {
        core::str::from_utf8_unchecked(core::slice::from_raw_parts(sig.as_ptr() as *const u8, 12))
    };
    log::info!("Hypervisor Signature: {}", sig_str);

    if max_leaf < 0x40000001 {
        log::warn!("CPUID 0x40000001 not supported");
        return false;
    }

    let (features, ..) = cpuid(0x40000001);
    log::info!("KVM Features (EAX): {:#x}", features);

    // KVM_FEATURE_CLOCKSOURCE2 is bit 3
    let has_clocksource2 = (features & (1 << 3)) != 0;
    // KVM_FEATURE_CLOCKSOURCE is bit 0 (older)
    let has_clocksource = (features & (1 << 0)) != 0;

    log::info!(
        "KVM Clock Source: old={}, new={}",
        has_clocksource,
        has_clocksource2
    );

    has_clocksource2 || has_clocksource
}

fn cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    let eax: u32;
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov {0:e}, ebx",
            "pop rbx",
            out(reg) ebx,
            inout("eax") leaf => eax,
            lateout("ecx") ecx,
            lateout("edx") edx,
            options(nostack, preserves_flags),
        );
    }
    (eax, ebx, ecx, edx)
}

impl VdsoTimeData {
    pub fn set_pvclock_mode(&mut self) {
        for clk in self.clock_data.iter_mut() {
            clk.clock_mode = ClockMode::Pvclock as i32;
        }
    }
}

fn register_pvclock(cpu_id: usize) {
    let base = crate::vdso::vdso_data_paddr() as u64 + 4096;
    let offset = cpu_id * core::mem::size_of::<crate::x86_64::pvclock_data::PvClockTimeInfo>();
    let paddr = base + offset as u64;
    crate::x86_64::pvclock_data::register_kvm_clock(paddr);
    log::info!("PVCLOCK registered for cpu {} at {:#x}", cpu_id, paddr);
}
