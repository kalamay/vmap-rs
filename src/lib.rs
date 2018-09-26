//! A cross-platform library for fast and safe memory-mapped IO
//!
//! This library defines a convenient API for reading and writing to files
//! using the hosts virtual memory system. The design of the API strives to
//! both minimize the frequency of mapping system calls while still retaining
//! safe access.
//!
//! Additionally, a variety of buffer implementations are provided in the
//! [`vmap::buf`](buf/index.html) module.

//#![deny(missing_docs)]

use std::fs::File;
use std::io::{Result, Error, ErrorKind};
use std::sync::{Once, ONCE_INIT};

/// Low-level cross-platform virtual memory functions
pub mod os {
    #[cfg(unix)]
    mod unix;
    #[cfg(unix)]
    pub use self::unix::*;

    #[cfg(windows)]
    mod windows;
    #[cfg(windows)]
    pub use self::windows::*;
}

mod page;
pub use self::page::{Page, PageMut};

pub mod buf;

/// Type to represent whole page offsets and counts.
pub type Pgno = u32;

/// Protection level for a page.
pub enum Protect {
    /// The page(s) may only be read from.
    ReadOnly,
    /// The page(s) may be read from and written to.
    ReadWrite
}

/// Desired behavior when flushing write changes.
pub enum Flush {
    /// Request dirty pages to be written immediately and block until completed.
    ///
    /// This is not supported on Windows. The flush is always performed asynchronously.
    Sync,
    /// Request dirty pages to be written but do not wait for completion.
    Async,
}

static mut SIZE:usize = 0;
static INIT: Once = ONCE_INIT;

/// Gets a cached version of the system page size.
///
/// ```
/// # extern crate vmap;
/// let size = vmap::page_size();
/// println!("the system page size is {} bytes", size);
/// ```
pub fn page_size() -> usize {
    unsafe {
        INIT.call_once(|| {
            SIZE = self::os::page_size();
        });
        SIZE
    }
}

/// Type for allocating anonymous and file-backed virtual memory.
///
/// The construction of this object is very cheap, as it does not track
/// any of the allocations. That is handled through the Drop implementation.
/// This serves as an entry point for safe allocation sizes. Virtual memory
/// is restricted to allocations at page boundaries, so this type handles
/// adjustements when impropoer boundaries are used.
///
/// This type can also be used for convenient page size calculations.
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
/// let pages = alloc.page_count(200);
/// assert_eq!(pages, 1);
///
/// let f = OpenOptions::new().read(true).open("src/lib.rs")?;
/// let page = alloc.file_page(&f, 0, 1)?;
/// assert_eq!(b"fast and safe memory-mapped IO", &page[33..63]);
/// # Ok(())
/// # }
/// ```
#[derive(Copy, Clone)]
pub struct Alloc {
    sizem: usize,
    shift: u32,
}

impl Alloc {
    /// Creates a type for calculating page numbers and byte offsets.
    ///
    /// The size is determined from the system's configurated page size.
    /// While the call to get this value is cached, it is preferrable to
    /// reuse the Alloc instance when possible.
    #[inline]
    pub fn new() -> Self {
        unsafe { Self::new_size(page_size()) }
    }

    /// Creates a type for calculating page numbers and byte offsets using a
    /// known page size.
    ///
    /// # Safety
    ///
    /// The size *must* be a power-of-2. To successfully map pages, the size
    /// must also be a mutliple of the actual system page size. Hypothetically
    /// this could be used to simulate larger page sizes.
    ///
    /// # Example
    ///
    /// ```
    /// # extern crate vmap;
    /// use vmap::Alloc;
    ///
    /// let size = vmap::page_size();
    /// let alloc = unsafe { Alloc::new_size(size << 2) };
    /// assert_eq!(alloc.page_round(1), size << 2);   // probably 16384
    /// ```
    #[inline]
    pub unsafe fn new_size(size: usize) -> Self {
        Self {
            sizem: size - 1,
            shift: size.trailing_zeros()
        }
    }

    /// Round a byte size up to the nearest page size.
    ///
    /// # Example
    ///
    /// ```
    /// use vmap::Alloc;
    ///
    /// let alloc = Alloc::new();
    /// let size = vmap::page_size();
    /// assert_eq!(alloc.page_round(0), 0);
    /// assert_eq!(alloc.page_round(1), size);        // probably 4096
    /// assert_eq!(alloc.page_round(size-1), size);   // probably 4096
    /// assert_eq!(alloc.page_round(size), size);     // probably 4096
    /// assert_eq!(alloc.page_round(size+1), size*2); // probably 8192
    /// ```
    #[inline]
    pub fn page_round(&self, len: usize) -> usize {
        self.page_truncate(len + self.sizem)
    }

    /// Round a byte size down to the nearest page size.
    ///
    /// # Example
    ///
    /// ```
    /// use vmap::Alloc;
    ///
    /// let alloc = Alloc::new();
    /// let size = vmap::page_size();
    /// assert_eq!(alloc.page_truncate(0), 0);
    /// assert_eq!(alloc.page_truncate(1), 0);
    /// assert_eq!(alloc.page_truncate(size-1), 0);
    /// assert_eq!(alloc.page_truncate(size), size);   // probably 4096
    /// assert_eq!(alloc.page_truncate(size+1), size); // probably 4096
    /// ```
    #[inline]
    pub fn page_truncate(&self, len: usize) -> usize {
        len & !self.sizem
    }
    
    /// Convert a page count into a byte size.
    ///
    /// # Example
    ///
    /// ```
    /// use vmap::Alloc;
    ///
    /// let alloc = Alloc::new();
    /// let size = vmap::page_size();
    /// assert_eq!(alloc.page_size(0), 0);
    /// assert_eq!(alloc.page_size(1), size);   // probably 4096
    /// assert_eq!(alloc.page_size(2), size*2); // probably 8192
    /// ```
    #[inline]
    pub fn page_size(&self, count: Pgno) -> usize {
        (count as usize) << self.shift
    }
    
    /// Covert a byte size into the number of pages necessary to contain it.
    ///
    /// # Example
    ///
    /// ```
    /// use vmap::Alloc;
    ///
    /// let alloc = Alloc::new();
    /// let size = vmap::page_size();
    /// assert_eq!(alloc.page_count(0), 0);
    /// assert_eq!(alloc.page_count(1), 1);
    /// assert_eq!(alloc.page_count(size-1), 1);
    /// assert_eq!(alloc.page_count(size), 1);
    /// assert_eq!(alloc.page_count(size+1), 2);
    /// assert_eq!(alloc.page_count(size*2), 2);
    /// ```
    #[inline]
    pub fn page_count(&self, len: usize) -> Pgno {
        (self.page_round(len) >> self.shift) as Pgno
    }

    /// Create a new page object mapped from a range of a file.
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
    /// let f = OpenOptions::new().read(true).open("src/lib.rs")?;
    /// let page = alloc.file_page(&f, 0, 1)?;
    /// assert_eq!(page.is_empty(), false);
    /// assert_eq!(b"fast and safe memory-mapped IO", &page[33..63]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn file_page(&self, file: &File, no: Pgno, count: Pgno) -> Result<Page> {
        let off = self.page_size(no);
        let len = self.page_size(count);
        if file.metadata()?.len() < (off+len) as u64 {
            Err(Error::new(ErrorKind::InvalidInput, "page range not in file"))
        }
        else {
            unsafe {
                let ptr = self::os::map_file(file, off, len, Protect::ReadOnly)?;
                Ok(Page::new(ptr, len))
            }
        }
    }

    /// Create a new page object mapped from a range of a file without bounds
    /// checking.
    ///
    /// # Safety
    ///
    /// This does not verify that the requsted range is valid for the file.
    /// This can be useful in a few scenarios:
    /// 1. When the range is already known to be valid.
    /// 2. When a valid sub-range is known and not exceeded.
    /// 3. When the range will become valid and is not used until then.
    pub unsafe fn file_page_unchecked(&self, file: &File, no: Pgno, count: Pgno) -> Result<Page> {
        let off = self.page_size(no);
        let len = self.page_size(count);
        let ptr = self::os::map_file(file, off, len, Protect::ReadOnly)?;
        Ok(Page::new(ptr, len))
    }

    /// Create a new mutable page object mapped from a range of a file.
    pub fn file_page_mut(&self, file: &File, no: Pgno, count: Pgno) -> Result<PageMut> {
        let off = self.page_size(no);
        let len = self.page_size(count);
        if file.metadata()?.len() < (off+len) as u64 {
            Err(Error::new(ErrorKind::InvalidInput, "page range not in file"))
        }
        else {
            unsafe {
                let ptr = self::os::map_file(file, off, len, Protect::ReadWrite)?;
                Ok(PageMut::new(ptr, len))
            }
        }
    }

    /// Create a new mutable page object mapped from a range of a file.
    /// Create a new mutable page object mapped from a range of a file
    /// without bounds checking.
    ///
    /// # Safety
    ///
    /// This does not verify that the requsted range is valid for the file.
    /// This can be useful in a few scenarios:
    /// 1. When the range is already known to be valid.
    /// 2. When a valid sub-range is known and not exceeded.
    /// 3. When the range will become valid and is not used until then.
    pub unsafe fn file_page_mut_unchecked(&self, file: &File, no: Pgno, count: Pgno) -> Result<PageMut> {
        let off = self.page_size(no);
        let len = self.page_size(count);
        let ptr = self::os::map_file(file, off, len, Protect::ReadWrite)?;
        Ok(PageMut::new(ptr, len))
    }

    /// Create a fixed size buffer.
    pub fn buffer(&self, len: usize) -> Result<buf::Buffer> {
        let len = self.page_round(len);
        unsafe {
            let ptr = self::os::map_ring(len)?;
            Ok(buf::Buffer::new(ptr, len))
        }
    }

    /// Create a fixed size unbound circular.
    pub fn ring_buffer(&self, len: usize) -> Result<buf::RingBuffer> {
        let len = self.page_round(len);
        unsafe {
            let ptr = self::os::map_ring(len)?;
            Ok(buf::RingBuffer::new(ptr, len))
        }
    }
}

#[cfg(test)]
mod test {
    use super::Alloc;

    #[test]
    fn page_size() {
        let info = unsafe { Alloc::new_size(4096) };
        assert_eq!(info.page_round(0), 0);
        assert_eq!(info.page_round(1), 4096);
        assert_eq!(info.page_round(4095), 4096);
        assert_eq!(info.page_round(4096), 4096);
        assert_eq!(info.page_round(4097), 8192);
        assert_eq!(info.page_truncate(0), 0);
        assert_eq!(info.page_truncate(1), 0);
        assert_eq!(info.page_truncate(4095), 0);
        assert_eq!(info.page_truncate(4096), 4096);
        assert_eq!(info.page_truncate(4097), 4096);
        assert_eq!(info.page_size(0), 0);
        assert_eq!(info.page_size(1), 4096);
        assert_eq!(info.page_size(2), 8192);
        assert_eq!(info.page_count(0), 0);
        assert_eq!(info.page_count(1), 1);
        assert_eq!(info.page_count(4095), 1);
        assert_eq!(info.page_count(4096), 1);
        assert_eq!(info.page_count(4097), 2);
        assert_eq!(info.page_count(8192), 2);
    }
}

