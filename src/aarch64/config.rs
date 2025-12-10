pub const VVAR_PAGES: usize = 4;

#[repr(i32)]
pub enum ClockMode {
    None,
    Cntvct,
}
