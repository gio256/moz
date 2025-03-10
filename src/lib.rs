#![no_std]
#![feature(allocator_api)]
#![feature(pointer_is_aligned_to)]
#![allow(unused)]

use core::{
    alloc::{AllocError, Layout},
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

impl Mmap {
    fn new() -> Self {
        Self {
            pagesize: rustix::param::page_size(),
        }
    }
}

#[derive(Debug, Error)]
enum MmapErr {
    #[error("mmap failed with {0}")]
    Os(#[from] rustix::io::Errno),
    #[error("overflow")]
    Overflow,
}

fn mmap(len: usize) -> Result<NonNull<u8>, Errno> {
    let nil = ptr::null_mut();
    let rw = ProtFlags::READ | ProtFlags::WRITE;
    // SAFETY: passsing `ptr::null_mut()` means the kernel will choose a
    // page-aligned address at which to create the mapping. See mmap(2).
    let ptr = unsafe { mmap_anonymous(nil, len, rw, MapFlags::PRIVATE) }?;
    Ok(NonNull::new(ptr.cast()).unwrap())
}

const fn round_to_align(addr: usize, layout: Layout) -> usize {
    // SAFETY: alignment is guaranteed to be a power of two and therefore > 0.
    let align_minus_one = unsafe { usize::unchecked_sub(layout.align(), 1) };
    addr.wrapping_add(align_minus_one) & !align_minus_one
}

impl Mmap {
    //fn trim(&self)

    // SAFETY: `ptr` must be aligned to `self.pagesize`.
    unsafe fn munmap(&self, ptr: NonNull<u8>, len: usize) -> Result<(), Errno> {
        assert!(ptr.is_aligned_to(self.pagesize));
        //assert!(round_up(len, self.pagesize) == len);
        unsafe { rustix::mm::munmap(ptr.as_ptr().cast(), len) }
    }

    // https://github.com/jemalloc/jemalloc/blob/22440a0207cd7d7c624c78723ca1eeb8a4353e79/src/pages.c#L312-L336
    fn alloc(&self, layout: Layout) -> Result<Mem, MmapErr> {
        //let layout = layout.align_to(self.pagesize).unwrap();
        assert!(self.pagesize <= layout.align());
        let ptr = mmap(layout.size())?;
        if ptr.is_aligned_to(layout.align()) {
            return Ok(Mem { ptr, layout });
        }
        unsafe { self.munmap(ptr, layout.size()) }?;

        let padded_size = layout
            .size()
            .wrapping_add(layout.align())
            .wrapping_sub(self.pagesize);
        if padded_size < layout.size() {
            return Err(MmapErr::Overflow);
        }
        // NB: it's important that `start` is of type `NonNull<u8>` rather than `NonNull<()>`.
        let start: NonNull<u8> = mmap(padded_size)?;
        let align_ost = start.align_offset(layout.align());
        assert!(layout.size().checked_add(align_ost).unwrap() <= padded_size);
        let ptr = unsafe { start.add(align_ost) };
        if align_ost > 0 {
            unsafe { self.munmap(start, align_ost) }?;
        }
        let trim_end = padded_size - align_ost - layout.size();
        if trim_end > 0 {
            let end = unsafe { ptr.add(layout.size()) };
            unsafe { self.munmap(end, trim_end) }?;
        }
        //let ptr = start.map_addr(|a| round_to_align(a.into(), layout));
        Ok(Mem { ptr, layout })
    }

    unsafe fn free(&self, m: Mem) -> Result<(), MmapErr> {
        unsafe { self.munmap(m.ptr, m.layout.size()) }.map_err(Into::into)
    }
}
