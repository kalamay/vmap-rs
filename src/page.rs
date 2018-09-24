use std::slice;
use std::io::Result;
use std::ops::{Deref, DerefMut};
use std::fmt;

use super::{Protect, Flush};
use super::{unmap, protect, flush};



pub struct Page {
    base: PageMut,
}

impl Page {
    pub unsafe fn new(ptr: *mut u8, len: usize) -> Self {
        Self { base: PageMut::new(ptr, len) }
    }

    pub fn make_mut(self) -> Result<PageMut> {
        protect(self.base.ptr, self.base.len, Protect::ReadWrite)?;
        Ok(self.base)
    }
}

impl Deref for Page {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        self.base.deref()
    }
}

impl AsRef<[u8]> for Page {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

impl fmt::Debug for Page {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Page")
            .field("ptr", &self.base.ptr)
            .field("len", &self.base.len)
            .finish()
    }
}


#[derive(Debug)]
pub struct PageMut {
    ptr: *mut u8,
    len: usize,
}

impl PageMut {
    pub unsafe fn new(ptr: *mut u8, len: usize) -> Self {
        Self { ptr: ptr, len: len }
    }

    pub fn make_const(self) -> Result<Page> {
        protect(self.ptr, self.len, Protect::ReadOnly)?;
        Ok(Page { base: self })
    }

    pub fn flush(&self, mode: Flush) -> Result<()> {
        flush(self.ptr, self.len, mode)
    }
}

impl Drop for PageMut {
    fn drop(&mut self) {
        unmap(self.ptr, self.len).unwrap_or_default();
    }
}

impl Deref for PageMut {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.ptr as *const u8, self.len) }
    }
}

impl DerefMut for PageMut {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl AsRef<[u8]> for PageMut {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

impl AsMut<[u8]> for PageMut {
    #[inline]
    fn as_mut(&mut self) -> &mut [u8] {
        self.deref_mut()
    }
}

