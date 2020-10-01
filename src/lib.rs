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
//! use vmap::Map;
//! use std::io::Write;
//! use std::fs::OpenOptions;
//! use std::path::PathBuf;
//! use std::str::from_utf8;
//! # use std::fs;
//!
//! # fn main() -> vmap::Result<()> {
//! # let tmp = tempdir::TempDir::new("vmap")?;
//! let path: PathBuf = /* path to file */
//! # tmp.path().join("example");
//! # fs::write(&path, b"this is a test")?;
//!
//! // Open with write permissions so the Map can be converted into a MapMut
//! let map = Map::with_options().write().len(14).open(&path)?;
//! assert_eq!(Ok("this is a test"), from_utf8(&map[..]));
//!
//! // Move the Map into a MapMut
//! // ... we could have started with MapMut::with_options()
//! let mut map = map.into_map_mut()?;
//! {
//!     let mut data = &mut map[..];
//!     data.write_all(b"that")?;
//! }
//!
//! // Move the MapMut back into a Map
//! let map = map.into_map()?;
//! assert_eq!(Ok("that is a test"), from_utf8(&map[..]));
//! # Ok(())
//! # }
//! ```

#![deny(missing_docs)]

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicUsize, Ordering};

pub mod os;

mod error;
pub use self::error::{ConvertResult, Error, Input, Operation, Result};

mod map;
pub use self::map::{Map, MapMut, Options};

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

/// Byte extent type used for length and resize options.
///
/// For usage information, see the [`.len()`] or [`.resize()`] methods of the
/// [`Options`] builder type.
///
/// [`.len()`]: struct.Options.html#method.len
/// [`.resize()`]: struct.Options.html#method.resize
/// [`Options`]: struct.Options.html
pub enum Extent {
    /// A dynamic extent that implies the end byte position of an underlying
    /// file resource or anonymous allocation.
    End,
    /// A static extent that referers to an exact byte position.
    Exact(usize),
    /// A dynamic extent that referes a byte position of at least a particular
    /// offset.
    Min(usize),
    /// A dynamic extent that referes a byte position of no greater than a
    /// particular offset.
    Max(usize),
}

impl From<usize> for Extent {
    fn from(v: usize) -> Self {
        Self::Exact(v)
    }
}

/// Gets a cached version of the system page size.
///
/// # Examples
///
/// ```
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

/// General trait for working with any memory-safe representation of a
/// contiguous region of arbitrary memory.
pub trait Span: Deref<Target = [u8]> + Sized + sealed::Span {
    /// Get the length of the allocated region.
    fn len(&self) -> usize;

    /// Get the pointer to the start of the allocated region.
    fn as_ptr(&self) -> *const u8;

    /// Tests if the span covers zero bytes.
    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// General trait for working with any memory-safe representation of a
/// contiguous region of arbitrary mutable memory.
pub trait SpanMut: Span + DerefMut {
    /// Get a mutable pointer to the start of the allocated region.
    fn as_mut_ptr(&mut self) -> *mut u8;
}

impl<'a> Span for &'a [u8] {
    #[inline]
    fn len(&self) -> usize {
        <[u8]>::len(self)
    }

    #[inline]
    fn as_ptr(&self) -> *const u8 {
        <[u8]>::as_ptr(self)
    }
}

impl<'a> Span for &'a mut [u8] {
    #[inline]
    fn len(&self) -> usize {
        <[u8]>::len(self)
    }

    #[inline]
    fn as_ptr(&self) -> *const u8 {
        <[u8]>::as_ptr(self)
    }
}

impl<'a> SpanMut for &'a mut [u8] {
    #[inline]
    fn as_mut_ptr(&mut self) -> *mut u8 {
        <[u8]>::as_mut_ptr(self)
    }
}

mod sealed {
    pub trait Span {}

    impl Span for super::Map {}
    impl Span for super::MapMut {}
    impl<'a> Span for &'a [u8] {}
    impl<'a> Span for &'a mut [u8] {}

    pub trait FromPtr {
        unsafe fn from_ptr(ptr: *mut u8, len: usize) -> Self;
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::str::from_utf8;

    use super::*;

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
        assert_eq!(sz.offset(0), 0);
        assert_eq!(sz.offset(1), 1);
        assert_eq!(sz.offset(4095), 4095);
        assert_eq!(sz.offset(4096), 0);
        assert_eq!(sz.offset(4097), 1);
    }

    #[test]
    fn alloc_min() -> Result<()> {
        let sz = Size::allocation();
        let mut map = MapMut::with_options().len(Extent::Min(100)).alloc()?;
        assert_eq!(map.len(), sz.round(100));
        assert_eq!(Ok("\0\0\0\0\0"), from_utf8(&map[..5]));
        {
            let mut data = &mut map[..];
            data.write_all(b"hello")?;
        }
        assert_eq!(Ok("hello"), from_utf8(&map[..5]));
        Ok(())
    }

    #[test]
    fn alloc_exact() -> Result<()> {
        let mut map = MapMut::with_options().len(5).alloc()?;
        assert_eq!(map.len(), 5);
        assert_eq!(Ok("\0\0\0\0\0"), from_utf8(&map[..]));
        {
            let mut data = &mut map[..];
            data.write_all(b"hello")?;
        }
        assert_eq!(Ok("hello"), from_utf8(&map[..]));
        Ok(())
    }

    #[test]
    fn alloc_offset() -> Result<()> {
        // map to the offset of the last 5 bytes of a page, but map 6 bytes
        let off = Size::allocation().size(1) - 5;
        let mut map = MapMut::with_options().offset(off).len(6).alloc()?;

        // force the page after the 5 bytes to be read-only
        unsafe { os::protect(map.as_mut_ptr().add(5), 1, Protect::ReadOnly)? };

        assert_eq!(map.len(), 6);
        assert_eq!(Ok("\0\0\0\0\0\0"), from_utf8(&map[..]));
        {
            let mut data = &mut map[..];
            // writing one more byte will segfault
            data.write_all(b"hello")?;
        }
        assert_eq!(Ok("hello\0"), from_utf8(&map[..]));
        Ok(())
    }

    #[test]
    fn read_end() -> Result<()> {
        let (_tmp, path, len) = write_default("read_end")?;
        let map = Map::with_options().offset(29).open(&path)?;
        assert!(map.len() >= 30);
        assert_eq!(len - 29, map.len());
        assert_eq!(Ok("fast and safe memory-mapped IO"), from_utf8(&map[..30]));
        Ok(())
    }

    #[test]
    fn read_min() -> Result<()> {
        let (_tmp, path, len) = write_default("read_min")?;
        let map = Map::with_options()
            .offset(29)
            .len(Extent::Min(30))
            .open(&path)?;
        println!("path = {:?}, len = {}, map = {}", path, len, map.len());
        assert!(map.len() >= 30);
        assert_eq!(len - 29, map.len());
        assert_eq!(Ok("fast and safe memory-mapped IO"), from_utf8(&map[..30]));
        Ok(())
    }

    #[test]
    fn read_max() -> Result<()> {
        let (_tmp, path, _len) = write_default("read_max")?;
        let map = Map::with_options()
            .offset(29)
            .len(Extent::Max(30))
            .open(&path)?;
        assert!(map.len() == 30);
        assert_eq!(Ok("fast and safe memory-mapped IO"), from_utf8(&map[..]));
        Ok(())
    }

    #[test]
    fn read_exact() -> Result<()> {
        let (_tmp, path, _len) = write_default("read_exact")?;
        let map = Map::with_options().offset(29).len(30).open(&path)?;
        assert!(map.len() == 30);
        assert_eq!(Ok("fast and safe memory-mapped IO"), from_utf8(&map[..]));
        Ok(())
    }

    #[test]
    fn copy() -> Result<()> {
        let (_tmp, path, _len) = write_default("copy")?;
        let mut map = MapMut::with_options()
            .offset(29)
            .len(30)
            .copy()
            .open(&path)?;
        assert_eq!(map.len(), 30);
        assert_eq!(Ok("fast and safe memory-mapped IO"), from_utf8(&map[..]));
        {
            let mut data = &mut map[..];
            data.write_all(b"nice")?;
        }
        assert_eq!(Ok("nice and safe memory-mapped IO"), from_utf8(&map[..]));
        Ok(())
    }

    #[test]
    fn write_into_mut() -> Result<()> {
        let tmp = tempdir::TempDir::new("vmap")?;
        let path: PathBuf = tmp.path().join("write_into_mut");
        fs::write(&path, "this is a test").expect("failed to write file");

        let map = Map::with_options().write().resize(16).open(&path)?;
        assert_eq!(16, map.len());
        assert_eq!(Ok("this is a test"), from_utf8(&map[..14]));
        assert_eq!(Ok("this is a test\0\0"), from_utf8(&map[..]));

        let mut map = map.into_map_mut()?;
        {
            let mut data = &mut map[..];
            data.write_all(b"that")?;
            assert_eq!(Ok("that is a test"), from_utf8(&map[..14]));
            assert_eq!(Ok("that is a test\0\0"), from_utf8(&map[..]));
        }

        let map = map.into_map()?;
        assert_eq!(Ok("that is a test"), from_utf8(&map[..14]));
        assert_eq!(Ok("that is a test\0\0"), from_utf8(&map[..]));

        let map = Map::with_options().open(&path)?;
        assert_eq!(16, map.len());
        assert_eq!(Ok("that is a test"), from_utf8(&map[..14]));
        assert_eq!(Ok("that is a test\0\0"), from_utf8(&map[..]));

        Ok(())
    }

    #[test]
    fn truncate() -> Result<()> {
        let tmp = tempdir::TempDir::new("vmap")?;
        let path: PathBuf = tmp.path().join("truncate");
        fs::write(&path, "this is a test").expect("failed to write file");

        let map = Map::with_options()
            .write()
            .truncate(true)
            .resize(16)
            .open(&path)?;
        assert_eq!(16, map.len());
        assert_eq!(Ok("\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0"), from_utf8(&map[..]));
        Ok(())
    }

    type WriteResult = Result<(tempdir::TempDir, PathBuf, usize)>;

    fn write_tmp(name: &'static str, msg: &'static str) -> WriteResult {
        let tmp = tempdir::TempDir::new("vmap")?;
        let path: PathBuf = tmp.path().join(name);
        fs::write(&path, msg)?;
        Ok((tmp, path, msg.len()))
    }

    fn write_default(name: &'static str) -> WriteResult {
        write_tmp(
            name,
            "A cross-platform library for fast and safe memory-mapped IO in Rust",
        )
    }
}
