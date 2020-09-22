//! A cross-platform library for fast and safe memory-mapped IO
//!
//! This library defines a convenient API for reading and writing to files
//! using the hosts virtual memory system. The design of the API strives to
//! both minimize the frequency of mapping system calls while still retaining
//! safe access.
//!
//! Additionally, a variety of buffer implementations are provided in the
//! [`vmap::io`](io/index.html) module.
//!
//! # Examples
//!
//! ```
//! # extern crate vmap;
//! # extern crate tempdir;
//! #
//! use vmap::Map;
//! use std::io::Write;
//! use std::fs::OpenOptions;
//! use std::path::PathBuf;
//! # use std::fs;
//!
//! # fn main() -> vmap::Result<()> {
//! # let tmp = tempdir::TempDir::new("vmap")?;
//! let path: PathBuf = /* path to file */
//! # tmp.path().join("example");
//! # fs::write(&path, b"this is a test")?;
//! // Open with write permissions so the Map can be converted into a MapMut
//! let file = OpenOptions::new().read(true).write(true).open(&path)?;
//!
//! // Map the beginning of the file
//! let map = Map::file(&file, 0, 14)?;
//! assert_eq!(b"this is a test", &map[..]);
//!
//! // Move the Map into a MapMut
//! // ... we could have started with MapMut::file(...)
//! let mut map = map.into_map_mut()?;
//! {
//!     let mut data = &mut map[..];
//!     data.write_all(b"that")?;
//! }
//!
//! // Move the MapMut back into a Map
//! let map = map.into_map()?;
//! assert_eq!(b"that is a test", &map[..]);
//! # Ok(())
//! # }
//! ```

#![deny(missing_docs)]

use std::sync::atomic::{AtomicUsize, Ordering};

pub mod os;

mod error;
pub use self::error::{ConvertResult, Error, Input, KernelResult, Operation, Result};

mod span;
pub use self::span::{Span, SpanMut};

mod map;
pub use self::map::{Map, MapMut};

pub mod io;

/// Type to represent whole page offsets and counts.
pub type Pgno = u32;

/// Protection level for a page.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Protect {
    /// The page(s) may only be read from.
    ReadOnly,
    /// The page(s) may be read from and written to.
    ReadWrite,
    /// Like `ReadWrite`, but changes are not shared.
    ReadCopy,
}

/// Desired behavior when flushing write changes.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Flush {
    /// Request dirty pages to be written immediately and block until completed.
    ///
    /// This is not supported on Windows. The flush is always performed asynchronously.
    Sync,
    /// Request dirty pages to be written but do not wait for completion.
    Async,
}

/// Hint for the access pattern of the underlying mapping.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AdviseAccess {
    /// Use the system default behavior.
    Normal,
    /// The map will be accessed in a sequential manner.
    Sequential,
    /// The map will be accessed in a random manner.
    Random,
}

/// Hint for the immediacy of accessing the underlying mapping.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AdviseUsage {
    /// Use the system default behavior.
    Normal,
    /// The map is expected to be accessed soon.
    WillNeed,
    /// The map is not expected to be accessed soon.
    WillNotNeed,
}

/// Gets a cached version of the system page size.
///
/// # Examples
///
/// ```
/// # extern crate vmap;
/// let page_size = vmap::page_size();
/// println!("the system page size is {} bytes", page_size);
/// assert!(page_size >= 4096);
/// ```
pub fn page_size() -> usize {
    let size = PAGE_SIZE.load(Ordering::Relaxed);
    if size == 0 {
        load_system_info().0 as usize
    } else {
        size
    }
}

/// Gets a cached version of the system allocation granularity size.
///
/// On Windows this value is typically 64k. Otherwise it is the same as the
/// page size.
///
/// # Examples
///
/// ```
/// # extern crate vmap;
/// let alloc_size = vmap::allocation_size();
/// println!("the system allocation granularity is {} bytes", alloc_size);
/// if cfg!(windows) {
///     assert!(alloc_size >= 65536);
/// } else {
///     assert!(alloc_size >= 4096);
/// }
/// ```
pub fn allocation_size() -> usize {
    let size = ALLOC_SIZE.load(Ordering::Relaxed);
    if size == 0 {
        load_system_info().1 as usize
    } else {
        size
    }
}

static PAGE_SIZE: AtomicUsize = AtomicUsize::new(0);
static ALLOC_SIZE: AtomicUsize = AtomicUsize::new(0);

#[inline]
fn load_system_info() -> (u32, u32) {
    let (page, alloc) = self::os::system_info();
    PAGE_SIZE.store(page as usize, Ordering::Relaxed);
    ALLOC_SIZE.store(alloc as usize, Ordering::Relaxed);
    (page, alloc)
}

/// Type for calculation system page or allocation size information.
///
/// # Examples
///
/// ```
/// # extern crate vmap;
/// let size = vmap::AllocSize::new();
/// let pages = size.count(200);
/// assert_eq!(pages, 1);
///
/// let round = size.round(200);
/// println!("200 bytes requires a {} byte mapping", round);
///
/// let count = size.count(10000);
/// println!("10000 bytes requires {} pages", count);
///
/// let size = size.size(3);
/// println!("3 pages are {} bytes", size);
/// ```
#[deprecated(since = "0.4.0", note = "use Size instead")]
pub type AllocSize = Size;

/// Type for calculation system page or allocation size information.
///
/// # Examples
///
/// ```
/// # extern crate vmap;
/// let size = vmap::Size::allocation();
/// let pages = size.count(200);
/// assert_eq!(pages, 1);
///
/// let round = size.round(200);
/// println!("200 bytes requires a {} byte mapping", round);
///
/// let count = size.count(10000);
/// println!("10000 bytes requires {} pages", count);
///
/// let size = size.size(3);
/// println!("3 pages are {} bytes", size);
/// ```
#[derive(Copy, Clone)]
pub struct Size(usize);

impl Size {
    /// Creates a type for calculating allocation numbers and byte offsets.
    ///
    /// The size is determined from the system's configurated allocation
    /// granularity. This value is cached making it very cheap to construct.
    #[inline]
    #[deprecated(since = "0.4.0", note = "use Size::allocation instead")]
    pub fn new() -> Self {
        Self::allocation()
    }

    /// Creates a type for calculating page numbers and byte offsets.
    ///
    /// The size is determined from the system's configurated page size.
    /// This value is cached making it very cheap to construct.
    #[inline]
    pub fn page() -> Self {
        unsafe { Self::with_size(page_size()) }
    }

    /// Creates a type for calculating allocation numbers and byte offsets.
    ///
    /// The size is determined from the system's configurated allocation
    /// granularity. This value is cached making it very cheap to construct.
    #[inline]
    pub fn allocation() -> Self {
        unsafe { Self::with_size(allocation_size()) }
    }

    /// Creates a type for calculating page numbers and byte offsets using a
    /// known page size.
    ///
    /// # Safety
    ///
    /// The size *must* be a power-of-2. To successfully map pages, the size
    /// must also be a mutliple of the actual system allocation granularity.
    /// Hypothetically this could be used to simulate larger page sizes, but
    /// this has no bearing on the TLB cache.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate vmap;
    /// use vmap::Size;
    ///
    /// let sys = vmap::allocation_size();
    /// let size = unsafe { Size::with_size(sys << 2) };
    /// assert_eq!(size.round(1), sys << 2);   // probably 16384
    /// ```
    #[inline]
    pub unsafe fn with_size(size: usize) -> Self {
        Size(size)
    }

    /// Round a byte size up to the nearest page size.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Size;
    ///
    /// let sys = vmap::allocation_size();
    /// let size = Size::allocation();
    /// assert_eq!(size.round(0), 0);
    /// assert_eq!(size.round(1), sys);       // probably 4096
    /// assert_eq!(size.round(sys-1), sys);   // probably 4096
    /// assert_eq!(size.round(sys), sys);     // probably 4096
    /// assert_eq!(size.round(sys+1), sys*2); // probably 8192
    /// ```
    #[inline]
    pub fn round(&self, len: usize) -> usize {
        self.truncate(len + self.0 - 1)
    }

    /// Round a byte size down to the nearest page size.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Size;
    ///
    /// let sys = vmap::allocation_size();
    /// let size = Size::allocation();
    /// assert_eq!(size.truncate(0), 0);
    /// assert_eq!(size.truncate(1), 0);
    /// assert_eq!(size.truncate(sys-1), 0);
    /// assert_eq!(size.truncate(sys), sys);   // probably 4096
    /// assert_eq!(size.truncate(sys+1), sys); // probably 4096
    /// ```
    #[inline]
    pub fn truncate(&self, len: usize) -> usize {
        len & !(self.0 - 1)
    }

    /// Calculate the byte offset from page containing the position.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Size;
    ///
    /// let sys = vmap::allocation_size();
    /// let size = Size::allocation();
    /// assert_eq!(size.offset(1), 1);
    /// assert_eq!(size.offset(sys-1), sys-1);
    /// assert_eq!(size.offset(sys*2 + 123), 123);
    /// ```
    pub fn offset(&self, len: usize) -> usize {
        len & (self.0 - 1)
    }

    /// Convert a page count into a byte size.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Size;
    ///
    /// let sys = vmap::allocation_size();
    /// let size = Size::allocation();
    /// assert_eq!(size.size(0), 0);
    /// assert_eq!(size.size(1), sys);   // probably 4096
    /// assert_eq!(size.size(2), sys*2); // probably 8192
    /// ```
    #[inline]
    pub fn size(&self, count: Pgno) -> usize {
        (count as usize) << self.0.trailing_zeros()
    }

    /// Covert a byte size into the number of pages necessary to contain it.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Size;
    ///
    /// let sys = vmap::allocation_size();
    /// let size = Size::allocation();
    /// assert_eq!(size.count(0), 0);
    /// assert_eq!(size.count(1), 1);
    /// assert_eq!(size.count(sys-1), 1);
    /// assert_eq!(size.count(sys), 1);
    /// assert_eq!(size.count(sys+1), 2);
    /// assert_eq!(size.count(sys*2), 2);
    /// ```
    #[inline]
    pub fn count(&self, len: usize) -> Pgno {
        (self.round(len) >> self.0.trailing_zeros()) as Pgno
    }

    /// Calculates the page bounds for a pointer and length.
    ///
    /// # Safety
    ///
    /// There is no verification that the pointer is a mapped page nor that
    /// the calculated offset may be dereferenced.
    pub unsafe fn bounds(&self, ptr: *mut u8, len: usize) -> (*mut u8, usize) {
        let off = self.offset(ptr as usize);
        (ptr.offset(-(off as isize)), self.round(len + off))
    }
}

impl Default for Size {
    fn default() -> Self {
        Self::allocation()
    }
}

#[cfg(test)]
mod tests {
    use super::Size;

    #[test]
    fn allocation_size() {
        let sz = unsafe { Size::with_size(4096) };
        assert_eq!(sz.round(0), 0);
        assert_eq!(sz.round(1), 4096);
        assert_eq!(sz.round(4095), 4096);
        assert_eq!(sz.round(4096), 4096);
        assert_eq!(sz.round(4097), 8192);
        assert_eq!(sz.truncate(0), 0);
        assert_eq!(sz.truncate(1), 0);
        assert_eq!(sz.truncate(4095), 0);
        assert_eq!(sz.truncate(4096), 4096);
        assert_eq!(sz.truncate(4097), 4096);
        assert_eq!(sz.size(0), 0);
        assert_eq!(sz.size(1), 4096);
        assert_eq!(sz.size(2), 8192);
        assert_eq!(sz.count(0), 0);
        assert_eq!(sz.count(1), 1);
        assert_eq!(sz.count(4095), 1);
        assert_eq!(sz.count(4096), 1);
        assert_eq!(sz.count(4097), 2);
        assert_eq!(sz.count(8192), 2);
    }
}
