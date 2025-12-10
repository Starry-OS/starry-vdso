pub const VVAR_PAGES: usize = 6;

#[repr(i32)]
pub enum ClockMode {
    None,
    Tsc,
    Pvclock,
}
