pub const VVAR_PAGES: usize = 6;
pub const PVCLOCK_MAX_CPUS: usize = 128;

#[repr(i32)]
pub enum ClockMode {
    None,
    Tsc,
    Pvclock,
}
