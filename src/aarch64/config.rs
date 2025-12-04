pub const VVAR_PAGES: usize = 5;

#[repr(i32)]
pub enum ClockMode {
    None,
    Cntvct,
}
