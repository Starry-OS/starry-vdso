pub const VVAR_PAGES: usize = 44;
#[cfg(target_arch = "loongarch64")]
#[repr(i32)]
pub enum ClockMode {
    None,
    Csr,
}