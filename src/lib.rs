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
//! # Example
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
//! # fn main() -> std::io::Result<()> {
//! # let tmp = tempdir::TempDir::new("vmap")?;
//! let path: PathBuf = /* path to file */
//! # tmp.path().join("example");
//! # fs::write(&path, b"this is a test")?;
//! let file = OpenOptions::new().read(true).write(true).open(&path)?;
//!
//! // Map the beginning of the file
//! let map = Map::file(&file, 0, 14)?;
//! assert_eq!(b"this is a test", &map[..]);
//!
//! // Move the Map into a MapMut
//! // ... we could have started with MapMut::file(...)
//! let mut map = map.make_mut()?;
//! {
//!     let mut data = &mut map[..];
//!     data.write_all(b"that")?;
//! }
//!
//! // Move the MapMut back into a Map
//! let map = map.make_read_only()?;
//! assert_eq!(b"that is a test", &map[..]);
//! # Ok(())
//! # }
//! ```

#![deny(missing_docs)]

use std::sync::atomic::{AtomicUsize, Ordering};

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

mod map;
pub use self::map::{Map, MapMut, Mapped, MappedMut};

mod slice;
pub use self::slice::{ArcSlice, ByteRange};

pub mod io;

/// Type to represent whole page offsets and counts.
pub type Pgno = u32;

/// Protection level for a page.
pub enum Protect {
    /// The page(s) may only be read from.
    ReadOnly,
    /// The page(s) may be read from and written to.
    ReadWrite,
    /// Like `ReadWrite`, but changes are not shared.
    ReadCopy,
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

/// Hint for the access pattern of the underlying mapping.
pub enum AdviseAccess {
    /// Use the system default behavior.
    Normal,
    /// The map will be accessed in a sequential manner.
    Sequential,
    /// The map will be accessed in a random manner.
    Random,
}

/// Hint for the immediacy of accessing the underlying mapping.
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
/// ```
/// # extern crate vmap;
/// println!("the system page size is {} bytes", vmap::page_size());
/// ```
pub fn page_size() -> usize {
    static SIZE: AtomicUsize = AtomicUsize::new(0);
    let mut size: usize = SIZE.load(Ordering::Relaxed);
    if size == 0 {
        size = crate::os::page_size();
        SIZE.store(size, Ordering::Relaxed);
    }
    size
}

/// Gets a cached version of the system allocation granularity size.
///
/// On Windows this value is typically 64k. Otherwise it is the same as the
/// page size.
///
/// ```
/// # extern crate vmap;
/// println!("the system allocation granularity is {} bytes", vmap::allocation_size());
/// ```
pub fn allocation_size() -> usize {
    static SIZE: AtomicUsize = AtomicUsize::new(0);
    let mut size: usize = SIZE.load(Ordering::Relaxed);
    if size == 0 {
        size = crate::os::allocation_size();
        SIZE.store(size, Ordering::Relaxed);
    }
    size
}

/// Type for calculation page size information.
///
/// # Example
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
#[derive(Copy, Clone)]
pub struct AllocSize(usize);

impl AllocSize {
    /// Creates a type for calculating page numbers and byte offsets.
    ///
    /// The size is determined from the system's configurated page size.
    /// This value is cached making it very cheap to construct.
    #[inline]
    pub fn new() -> Self {
        unsafe { Self::with_size(allocation_size()) }
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
    /// use vmap::AllocSize;
    ///
    /// let sys = vmap::allocation_size();
    /// let size = unsafe { AllocSize::with_size(sys << 2) };
    /// assert_eq!(size.round(1), sys << 2);   // probably 16384
    /// ```
    #[inline]
    pub unsafe fn with_size(size: usize) -> Self {
        AllocSize(size)
    }

    /// Round a byte size up to the nearest page size.
    ///
    /// # Example
    ///
    /// ```
    /// use vmap::AllocSize;
    ///
    /// let sys = vmap::allocation_size();
    /// let size = AllocSize::new();
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
    /// # Example
    ///
    /// ```
    /// use vmap::AllocSize;
    ///
    /// let sys = vmap::allocation_size();
    /// let size = AllocSize::new();
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
    /// # Example
    ///
    /// ```
    /// use vmap::AllocSize;
    ///
    /// let sys = vmap::allocation_size();
    /// let size = AllocSize::new();
    /// assert_eq!(size.offset(1), 1);
    /// assert_eq!(size.offset(sys-1), sys-1);
    /// assert_eq!(size.offset(sys*2 + 123), 123);
    /// ```
    pub fn offset(&self, len: usize) -> usize {
        len & (self.0 - 1)
    }

    /// Convert a page count into a byte size.
    ///
    /// # Example
    ///
    /// ```
    /// use vmap::AllocSize;
    ///
    /// let sys = vmap::allocation_size();
    /// let size = AllocSize::new();
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
    /// # Example
    ///
    /// ```
    /// use vmap::AllocSize;
    ///
    /// let sys = vmap::allocation_size();
    /// let size = AllocSize::new();
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

#[cfg(test)]
mod tests {
    use super::AllocSize;

    #[test]
    fn allocation_size() {
        let sz = unsafe { AllocSize::with_size(4096) };
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
