use core::arch::asm;

#[repr(C, packed)]
struct GdtDesc {
    limit: u16,
    base: u64,
}

#[repr(align(8))]
struct GdtBuffer(#[allow(dead_code)] [u64; 32]);
static mut EXTENDED_GDT: GdtBuffer = GdtBuffer([0; 32]);

/// Initialize the GDT entry for vDSO getcpu.
pub fn init_vdso_getcpu(cpu_id: u32, node_id: u32) {
    let mut gdtr = GdtDesc { limit: 0, base: 0 };
    unsafe {
        asm!("sgdt [{}]", in(reg) &mut gdtr, options(nostack, preserves_flags));
    }

    let mut gdt_base = gdtr.base as *mut u64;
    let limit = gdtr.limit;
    let entry_idx = 15;
    let needed_limit = (entry_idx * 8 + 7) as u16;

    // Check if GDT is large enough
    if needed_limit > limit {
        log::warn!("GDT is too small for vDSO getcpu: limit={limit}, extending...");

        unsafe {
            let old_entries = (limit as usize + 1) / 8;
            let new_gdt_ptr = core::ptr::addr_of_mut!(EXTENDED_GDT) as *mut u64;

            for i in 0..old_entries {
                *new_gdt_ptr.add(i) = *gdt_base.add(i);
            }

            gdtr.base = new_gdt_ptr as u64;
            gdtr.limit = (32 * 8 - 1) as u16;

            asm!("lgdt [{}]", in(reg) &mut gdtr, options(nostack, preserves_flags));

            gdt_base = gdtr.base as *mut u64;

            let new_base = gdtr.base;
            let new_limit = gdtr.limit;
            log::info!("Extended GDT loaded. Base={new_base:#x}, Limit={new_limit}");
        }
    }

    let val = (node_id << 12) | (cpu_id & 0xfff);
    let limit_low = (val & 0xffff) as u64;
    let limit_high = ((val >> 16) & 0xf) as u64;

    unsafe {
        let entry_ptr = gdt_base.add(entry_idx);

        let access_byte = 0xF2u64;
        let flags = 0x0u64;

        let entry = limit_low | (access_byte << 40) | (limit_high << 48) | (flags << 52);

        *entry_ptr = entry;

        log::info!(
            "Initialized vDSO getcpu GDT entry at {:#x} with value {:#x} (limit={:#x})",
            entry_ptr as usize,
            entry,
            val
        );

        let mut check_val: u32;
        let selector: u16 = 0x7b;
        asm!("lsl {:e}, {:x}", out(reg) check_val, in(reg) selector);
        if check_val != val {
            log::warn!("LSL check failed! Expected {val:#x}, got {check_val:#x}");
        } else {
            log::info!("LSL check passed.");
        }
    }
}
