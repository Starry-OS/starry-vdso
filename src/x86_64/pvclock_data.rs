#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PvClockVcpuTimeInfo {
    pub version: u32,
    pub pad0: u32,
    pub tsc_timestamp: u64,
    pub system_time: u64,
    pub tsc_to_system_mul: u32,
    pub tsc_shift: i8,
    pub flags: u8,
    pub pad: [u8; 2],
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PvClockTimeInfo {
    pub pvti: PvClockVcpuTimeInfo,
}

pub const PVCLOCK_TSC_STABLE_BIT: u8 = 0x1;
pub const PVCLOCK_GUEST_STOPPED: u8 = 0x2;

impl PvClockTimeInfo {
    pub const fn new() -> Self {
        PvClockTimeInfo {
            pvti: PvClockVcpuTimeInfo::new(),
        }
    }
}

impl PvClockVcpuTimeInfo {
    pub const fn new() -> Self {
        PvClockVcpuTimeInfo {
            version: 0,
            pad0: 0,
            tsc_timestamp: 0,
            system_time: 0,
            tsc_to_system_mul: 0,
            tsc_shift: 0,
            flags: 0,
            pad: [0; 2],
        }
    }
}

pub const MSR_KVM_SYSTEM_TIME_NEW: u32 = 0x4b564d01;
pub const MSR_KVM_SYSTEM_TIME: u32 = 0x12;

/// Register the KVM clock for the current vCPU.
pub fn register_kvm_clock(paddr: u64) {
    let val = paddr | 1;
    let msr = MSR_KVM_SYSTEM_TIME_NEW;
    let low = val as u32;
    let high = (val >> 32) as u32;
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
            options(nostack, preserves_flags)
        );
    }
}
