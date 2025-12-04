extern crate alloc;

use alloc::alloc::dealloc;
use core::alloc::Layout;

const PAGE_SIZE_4K: usize = 4096;

/// RAII guard that will free allocated vdso pages on Drop unless disarmed.
pub struct VdsoAllocGuard {
    alloc: Option<(usize, usize)>,
}

impl VdsoAllocGuard {
    pub fn new(alloc: Option<(usize, usize)>) -> Self {
        Self { alloc }
    }

    pub fn disarm(&mut self) {
        self.alloc = None;
    }
}

impl Drop for VdsoAllocGuard {
    fn drop(&mut self) {
        if let Some((vaddr, pages)) = self.alloc {
            // free memory allocated with `alloc_zeroed` above
            let size = pages * PAGE_SIZE_4K;
            if let Ok(layout) = Layout::from_size_align(size, PAGE_SIZE_4K) {
                unsafe { dealloc(vaddr as *mut u8, layout) };
            }
        }
    }
}
