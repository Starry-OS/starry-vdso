use crate::vdso_time_data::VdsoTimeData;

pub struct VdsoData {
    pub time_data: VdsoTimeData,
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
