use crate::config::ClockMode;
use crate::vdso_time_data::VdsoTimeData;
use crate::x86_64::pvclock_data::PvClockTimeInfo;
use crate::x86_64::config::PVCLOCK_MAX_CPUS;

#[repr(C)]
pub struct VdsoData {
    pub time_data: VdsoTimeData,
    pub _pad: [u8; 3 * 4096],
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
            _pad: [0; 3 * 4096],
            pvclock: [PvClockTimeInfo::new(); PVCLOCK_MAX_CPUS],
        }
    }

    pub fn time_update(&mut self) {
        self.time_data.update();
    }

    /// Enable pvclock support.
    pub fn enable_pvclock(&mut self) {
        register_pvclock(0);
        self.time_data.set_pvclock_mode();
        log::info!("vDSO pvclock support enabled");
    }
}

impl VdsoTimeData {
    pub fn set_pvclock_mode(&mut self) {
        for clk in self.clock_data.iter_mut() {
            clk.clock_mode = ClockMode::Pvclock as i32;
        }
    }
}

fn register_pvclock(cpu_id: usize) {
    let base = crate::vdso::vdso_data_paddr() as u64 + 4 * 4096;
    let offset = cpu_id * core::mem::size_of::<crate::x86_64::pvclock_data::PvClockTimeInfo>();
    let paddr = base + offset as u64;
    crate::x86_64::pvclock_data::register_kvm_clock(paddr);
    log::info!("PVCLOCK registered for cpu {} at {:#x}", cpu_id, paddr);
}

