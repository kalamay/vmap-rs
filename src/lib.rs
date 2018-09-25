use std::fs::File;
use std::io::Result;
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

mod ring;
pub use self::ring::{Ring,UnboundRing};

mod buffer;
use self::buffer::Seq;

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
    /// This is not supported on Windows, and the flush is always performed asynchronously.
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
    /// let f = OpenOptions::new().read(true).open("README.md")?;
    /// let page = alloc.file_page(&f, 0, 1)?;
    /// assert_eq!(b"# vmap-rs", &page[..9]);
    ///
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn new() -> Self {
        unsafe { Self::new_size(page_size()) }
    }

    /// Creates a type for calculating page numbers and byte offsets using a
    /// known page size.
    ///
    /// The size *must* be a power-of-2.
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
    /// let alloc = unsafe { Alloc::new_size(4096) };
    /// assert_eq!(alloc.page_round(0), 0);
    /// assert_eq!(alloc.page_round(1), 4096);
    /// assert_eq!(alloc.page_round(4095), 4096);
    /// assert_eq!(alloc.page_round(4096), 4096);
    /// assert_eq!(alloc.page_round(4097), 8192);
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
    /// let alloc = unsafe { Alloc::new_size(4096) };
    /// assert_eq!(alloc.page_truncate(0), 0);
    /// assert_eq!(alloc.page_truncate(1), 0);
    /// assert_eq!(alloc.page_truncate(4095), 0);
    /// assert_eq!(alloc.page_truncate(4096), 4096);
    /// assert_eq!(alloc.page_truncate(4097), 4096);
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
    /// let alloc = unsafe { Alloc::new_size(4096) };
    /// assert_eq!(alloc.page_size(0), 0);
    /// assert_eq!(alloc.page_size(1), 4096);
    /// assert_eq!(alloc.page_size(2), 8192);
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
    /// let alloc = unsafe { Alloc::new_size(4096) };
    /// assert_eq!(alloc.page_count(0), 0);
    /// assert_eq!(alloc.page_count(1), 1);
    /// assert_eq!(alloc.page_count(4095), 1);
    /// assert_eq!(alloc.page_count(4096), 1);
    /// assert_eq!(alloc.page_count(4097), 2);
    /// assert_eq!(alloc.page_count(8192), 2);
    /// ```
    #[inline]
    pub fn page_count(&self, len: usize) -> Pgno {
        (self.page_round(len) >> self.shift) as Pgno
    }

    /// Create a new page object mapped from a range of a file.
    pub fn file_page(&self, file: &File, no: Pgno, count: Pgno) -> Result<Page> {
    	let len = self.page_size(count);
        unsafe {
            let ptr = self::os::map_file(file, self.page_size(no), len, Protect::ReadOnly)?;
            Ok(Page::new(ptr, len))
        }
    }

    /// Create a new mutable page object mapped from a range of a file.
    pub fn file_page_mut(&self, file: &File, no: Pgno, count: Pgno) -> Result<PageMut> {
    	let len = self.page_size(count);
        unsafe {
            let ptr = self::os::map_file(file, self.page_size(no), len, Protect::ReadWrite)?;
            Ok(PageMut::new(ptr, len))
        }
    }

    /// Create a circular buffer with minumum size.
    pub fn ring(&self, len: usize) -> Result<Ring> {
    	let len = self.page_round(len);
        unsafe {
            let ptr = self::os::map_ring(len)?;
            Ok(Ring::new(ptr, len))
        }
    }

    /// Create an unbound circular buffer with minumum size.
    pub fn unbound_ring(&self, len: usize) -> Result<UnboundRing> {
    	let len = self.page_round(len);
        unsafe {
            let ptr = self::os::map_ring(len)?;
            Ok(UnboundRing::new(ptr, len))
        }
    }
}

