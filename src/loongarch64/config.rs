pub const VVAR_PAGES: usize = 20;
pub const SIGRETURN_SYM_OFFSET: usize = 0xee8;

#[repr(i32)]
pub enum ClockMode {
    None,
    Csr,
}
