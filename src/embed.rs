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

cfg_if::cfg_if! {
    if #[cfg(target_arch = "x86_64")] {
        global_asm!(include_vdso!("x86_64"));
    } else if #[cfg(target_arch = "riscv64")] {
        global_asm!(include_vdso!("riscv"));
    } else if #[cfg(target_arch = "aarch64")]{
        global_asm!(include_vdso!("aarch64"));
    } else if #[cfg(any(target_arch = "loongarch64"))] {
        global_asm!(include_vdso!("loongarch64"));
    }
}
