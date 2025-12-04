pub const VVAR_PAGES: usize = 2;

#[repr(i32)]
pub enum ClockMode {
    None,
    Csr,
}