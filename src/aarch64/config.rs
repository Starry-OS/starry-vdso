pub const VVAR_PAGES: usize = 4;
pub const SIGRETURN_SYM_OFFSET: usize = 0x810;

#[repr(i32)]
pub enum ClockMode {
    None,
    Cntvct,
}
