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
