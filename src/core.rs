#![allow(unused)]

use core::{
    alloc::{AllocError, Layout},
    num::NonZero,
    ptr::{self, NonNull},
};

pub(crate) struct Tag {
    ptr: NonNull<u8>,
    layout: Layout,
}

impl Tag {
    /// # SAFETY
    ///
    /// TODO@safety
    /// `ptr` must be aligned to `layout.align()` and valid for `layout.size()`.
    pub(crate) unsafe fn new(ptr: NonNull<u8>, layout: Layout) -> Self {
        Self { ptr, layout }
    }

    #[inline]
    pub(crate) fn ptr(&self) -> NonNull<u8> {
        self.ptr
    }

    #[inline]
    pub(crate) fn layout(&self) -> Layout {
        self.layout
    }
}

pub(crate) trait Alloc {
    fn alloc(&self, layout: Layout) -> Result<Tag, AllocError>;
    unsafe fn free(&self, tag: Tag);
}

pub(crate) trait FreeAll {
    unsafe fn free_all(&self);
}

pub(crate) trait Grind {
    fn grind(&self);
}

pub struct ZeroHeap<T>(T);

impl<T: Alloc> Alloc for ZeroHeap<T> {
    fn alloc(&self, layout: Layout) -> Result<Tag, AllocError> {
        if layout.size() == 0 {
            Ok(unsafe { Tag::new(layout.dangling(), layout) })
        } else {
            self.0.alloc(layout)
        }
    }

    unsafe fn free(&self, tag: Tag) {
        if layout.size() != 0 {
            unsafe { self.0.free(tag) }
        }
    }
}
