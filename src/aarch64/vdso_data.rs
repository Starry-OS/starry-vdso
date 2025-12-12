use crate::vdso_time_data::VdsoTimeData;
#[repr(C)]
pub struct VdsoData {
    pub time_data: VdsoTimeData,
    pub timen_data: [u8; 4096],
    pub rng_data: [u8; 4096],
    pub arch_data: [u8; 4096],
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
            timen_data: [0u8; 4096],
            rng_data: [0u8; 4096],
            arch_data: [0u8; 4096],
        }
    }

    pub fn time_update(&mut self) {
        self.time_data.update();
    }
}

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
