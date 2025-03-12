#![allow(unused)]

use core::{
    alloc::{AllocError, Layout, LayoutError},
    num::NonZero,
    ptr::{self, NonNull},
};

use rustix::{
    io::Errno,
    mm::{MapFlags, ProtFlags, mmap_anonymous},
};
use thiserror::Error;

struct Mem {
    ptr: NonNull<u8>,
    layout: Layout,
}

pub struct Mmap {
    pagesize: usize,
}

#[derive(Debug, Error)]
enum MmapErr {
    #[error("mmap failed with {0}")]
    Os(#[from] rustix::io::Errno),
    #[error("overflow")]
    Overflow,
    #[error("failed to align")]
    NoAlign,
    #[error("mmap failed with {0}")]
    Layout(#[from] LayoutError),
}

fn map(len: usize) -> Result<NonNull<u8>, Errno> {
    let nil = ptr::null_mut();
    let rw = ProtFlags::READ | ProtFlags::WRITE;
    // SAFETY: passsing `ptr::null_mut()` means the kernel will choose a
    // page-aligned address at which to create the mapping. See mmap(2).
    let ptr = unsafe { mmap_anonymous(nil, len, rw, MapFlags::PRIVATE) }?;
    Ok(NonNull::new(ptr.cast()).unwrap())
}

impl Mmap {
    fn new() -> Self {
        Self {
            pagesize: rustix::param::page_size(),
        }
    }

    fn pagesize(&self) -> usize {
        self.pagesize
    }

    // SAFETY: `ptr` must be aligned to `self.pagesize`.
    unsafe fn unmap(&self, ptr: NonNull<u8>, len: usize) -> Result<(), Errno> {
        assert!(ptr.is_aligned_to(self.pagesize));
        assert!(len % self.pagesize == 0);
        //assert!(round_up(len, self.pagesize) == len);
        unsafe { rustix::mm::munmap(ptr.as_ptr().cast(), len) }
    }

    /// Cuts a cookie of shape `layout` from an allocation of size `alloc_size`
    /// bytes starting at `alloc`. The trimmed regions of memory are unmapped.
    /// The returned pointer is aligned to `layout.align()` and has provenance
    /// inherited from `alloc`.
    ///
    /// # SAFETY
    ///
    /// If `alloc` is not already aligned to `layout.align()`, then the pointer
    /// must be valid for the entire range of length `alloc_size` bytes
    /// beginning at `alloc`.
    unsafe fn trim(
        &self,
        alloc: NonNull<u8>,
        alloc_size: usize,
        layout: Layout,
    ) -> Result<NonNull<u8>, MmapErr> {
        let align_ost = alloc.align_offset(layout.align());
        let trim_end = alloc_size
            .checked_sub(align_ost)
            .and_then(|x| x.checked_sub(layout.size()))
            .ok_or(MmapErr::NoAlign)?;
        debug_assert!(align_ost + layout.size() <= alloc_size);

        // SAFETY:
        //
        // * The checked arithmetic above implies
        //   `align_ost + layout.size() <= alloc_size` The existence of an
        //   allocation of size `alloc_size` implies that `alloc_size` and
        //   therefore `align_ost` can never be larger than `isize::MAX`.
        // * If `align_ost > 0`, then the caller has ensured that `alloc` is
        //   valid for the entire range of length `alloc_size` beginning at
        //   `alloc`. From above, `align_ost + layout.size() <= alloc_size`.
        //   Therefore, `alloc` is valid for the range of length `layout.size()`
        //   beginning at `alloc + align_ost`.
        let aligned = unsafe { alloc.add(align_ost) };
        if align_ost > 0 {
            unsafe { self.unmap(alloc, align_ost) }?;
        }
        if trim_end > 0 {
            // SAFETY: As above, the checked arithmetic implies
            // `align_ost + layout.size() <= alloc_size`, and the caller ensures
            // that `alloc` is valid for the range of `alloc_size` bytes
            // beginning at `alloc`. The provenance of `aligned` is inherited
            // from `alloc`. Therefore `aligned` is valid for the entire range
            // of length `layout.size()` beginning at `alloc + align_ost`.
            let end = unsafe { aligned.add(layout.size()) };
            unsafe { self.unmap(end, trim_end) }?;
        }
        Ok(aligned)
    }

    // https://github.com/jemalloc/jemalloc/blob/22440a0207cd7d7c624c78723ca1eeb8a4353e79/src/pages.c#L312-L336
    fn alloc(&self, layout: Layout) -> Result<Mem, MmapErr> {
        let layout = layout.align_to(self.pagesize)?.pad_to_align();
        let ptr = map(layout.size())?;
        if ptr.is_aligned_to(layout.align()) {
            Ok(Mem { ptr, layout })
        } else {
            unsafe { self.unmap(ptr, layout.size()) }?;
            self.alloc_slow(layout)
        }
    }

    fn alloc_slow(&self, layout: Layout) -> Result<Mem, MmapErr> {
        // Any pointer returned by `mmap` is guaranteed to be page-aligned, so
        // we should be at most `align - pagesize` bytes away from an address
        // aligned to `align`. Reserving `align - pagesize` extra bytes ensures
        // that we can fit an aligned chunk of memory of length `layout.size()`
        // inside the allocation of `alloc_size` bytes beginning at `alloc`.
        let pad = layout.align().checked_sub(self.pagesize).unwrap();
        let alloc_size = layout.size().checked_add(pad).ok_or(MmapErr::Overflow)?;
        let alloc = map(alloc_size)?;
        // SAFETY: `alloc` points to the beginning of the freshly mmap'd region
        // of `alloc_size` bytes.
        let ptr = unsafe { self.trim(alloc, alloc_size, layout) }?;
        Ok(Mem { ptr, layout })
    }

    unsafe fn free(&self, m: Mem) -> Result<(), MmapErr> {
        unsafe { self.unmap(m.ptr, m.layout.size()) }.map_err(Into::into)
    }
}
