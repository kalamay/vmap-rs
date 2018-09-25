use std::slice;
use std::io::Result;
use std::ops::{Deref, DerefMut};
use std::fmt;

use ::{Protect, Flush};
use ::os::{unmap, protect, flush};



/// Allocation of one or more read-only sequential pages.
///
/// Typically you will not want to construct this directly. Instead, try
/// constructing these from an [`Alloc`](struct.Alloc.html) instance.
/// Use [`file_page`](struct.Alloc.html#method.file_page) to get full page
/// ranges from a file.
///
/// # Example
///
/// ```
/// # extern crate vmap;
/// use vmap::Alloc;
/// use std::fs::OpenOptions;
///
/// # fn main() -> std::io::Result<()> {
/// let alloc = Alloc::new();
/// let f = OpenOptions::new().read(true).open("README.md")?;
/// let page = alloc.file_page(&f, 0, 1)?;
/// assert_eq!(b"# vmap-rs", &page[..9]);
///
/// # Ok(())
/// # }
pub struct Page {
    base: PageMut,
}

impl Page {
    /// Constructs a new page sequence from an existing mapping.
    ///
    /// # Safety
    ///
    /// This does not know or care if `ptr` or `len` are valid. That is,
    /// it may be null, not at a proper page boundary, point to a size
    /// different from `len`, or worse yet, point to properly mapped pointer
    /// from some other allocation system.
    ///
    /// Generally don't use this unless you are entirely sure you are
    /// doing so correctly.
    ///
    /// # Example
    ///
    /// ```
    /// # extern crate vmap;
    /// use vmap::{Page, Protect};
    /// use std::fs::OpenOptions;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let f = OpenOptions::new().read(true).open("README.md")?;
    /// let page = unsafe {
    ///     let len = vmap::page_size();
    ///     let ptr = vmap::os::map_file(&f, 0, len, Protect::ReadOnly)?;
    ///     Page::new(ptr, len)
    /// };
    /// assert_eq!(b"# vmap-rs", &page[..9]);
    ///
    /// # Ok(())
    /// # }
    /// ```
    pub unsafe fn new(ptr: *mut u8, len: usize) -> Self {
        Self { base: PageMut::new(ptr, len) }
    }

    pub fn make_mut(self) -> Result<PageMut> {
        unsafe { protect(self.base.ptr, self.base.len, Protect::ReadWrite) }?;
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



/// Allocation of one or more read-write sequential pages.
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
        unsafe { protect(self.ptr, self.len, Protect::ReadOnly) }?;
        Ok(Page { base: self })
    }

    pub fn flush(&self, mode: Flush) -> Result<()> {
        unsafe { flush(self.ptr, self.len, mode) }
    }
}

impl Drop for PageMut {
    fn drop(&mut self) {
        unsafe { unmap(self.ptr, self.len) }.unwrap_or_default();
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

