use core::arch::global_asm;

macro_rules! include_vdso {
    ($arch:expr) => {
        concat!(
            ".global vdso_start, vdso_end\n",
            ".section .rodata\n",
            ".balign 4096\n",
            "vdso_start:\n",
            ".incbin \"",
            env!("CARGO_MANIFEST_DIR"),
            "/vdso/vdso_",
            $arch,
            ".so\"\n",
            ".balign 4096\n",
            "vdso_end:\n",
            ".previous"
        )
    };
}

#[cfg(target_arch = "riscv64")]
global_asm!(include_vdso!("rv"));

#[cfg(target_arch = "loongarch64")]
global_asm!(include_vdso!("la"));

#[cfg(target_arch = "aarch64")]
global_asm!(include_vdso!("aarch"));

#[cfg(target_arch = "x86_64")]
global_asm!(include_vdso!("x86"));

#[used]
#[unsafe(no_mangle)]
pub static mut VDSO_START: usize = 0;

#[used]
#[unsafe(no_mangle)]
pub static mut VDSO_END: usize = 0;

/// Initialize vDSO start/end address symbols at runtime; call this very early.
/// Returns (start_address, end_address) in kernel virtual memory.
pub unsafe fn init_vdso_symbols() -> (usize, usize) {
    unsafe extern "C" {
        static vdso_start: usize;
        static vdso_end: usize;
    }
    unsafe {
        VDSO_START = &vdso_start as *const usize as usize;
        VDSO_END = &vdso_end as *const usize as usize;
        (VDSO_START, VDSO_END)
    }
}
