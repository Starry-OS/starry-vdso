use crate::vdso_time_data::VdsoTimeData;

#[repr(C)]
pub struct VdsoData {
    pub time_data: VdsoTimeData,
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
        }
    }

    pub fn time_update(&mut self) {
        self.time_data.update();
    }
}
