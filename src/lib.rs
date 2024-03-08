//! A cross-platform library for fast and safe memory-mapped IO and boundary-free
//! ring buffer.
//!
//! This library defines a convenient API for reading and writing to files
//! using the hosts virtual memory system, as well as allocating memory and
//! creating circular memory regions. The design of the API strives to
//! both minimize the frequency of mapping system calls while still retaining
//! safe access. Critically, it never attempts the own the `File` object used
//! for mapping. That is, it never clones it or in any way retains it. While
//! this has some implications for the API (i.e. [`.flush()`]), it cannot cause
//! bugs outside of this library through `File`'s leaky abstraction when cloned
//! and then closed.
//!
//! The [`Map`] and [`MapMut`] types are primary means for allocating virtual
//! memory regions, both for a file and anonymously. Generally, the
//! [`Map::with_options()`] and [`MapMut::with_options()`] are used to specify
//! the mapping requirements. See [`Options`] for more information.
//!
//! The [`MapMut`] type maintains interior mutability for the mapped memory,
//! while the [`Map`] is read-only. However, it is possible to convert between
//! these types ([`.into_map_mut()`] and [`.into_map()`]) assuming the proper
//! [`Options`] are specified.
//!
//! Additionally, a variety of buffer implementations are provided in the
//! [`vmap::io`] module. The [`Ring`] and [`InfiniteRing`] use cross-platform
//! optimzed circular memory mapping to remove the typical boundary problem
//! with most circular buffers. This ensures all ranges of the underlying byte
//! buffer can be viewed as a single byte slice, event when the value wraps
//! back around to the beginning of the buffer. The [`BufReader`] and [`BufWriter`]
//! implement buffered I/O using a [`Ring`] as a backing layer.
//!
//! # Examples
//!
//! ```
//! use vmap::Map;
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
//! // Map the first 4 bytes
//! let (map, file) = Map::with_options().len(4).open(&path)?;
//! assert_eq!(Ok("this"), from_utf8(&map[..]));
//!
//! // Reuse the file to map a different region
//! let map = Map::with_options().offset(10).len(4).map(&file)?;
//! assert_eq!(Ok("test"), from_utf8(&map[..]));
//! # Ok(())
//! # }
//! ```
//!
//! If opened properly, the `Map` can be moved into a `MapMut` and modifications
//! to the underlying file can be performed:
//!
//! ```
//! use vmap::{Map, Flush};
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
//! let (map, file) = Map::with_options().write().len(14).open(&path)?;
//! assert_eq!(Ok("this is a test"), from_utf8(&map[..]));
//!
//! // Move the Map into a MapMut
//! // ... we could have started with MapMut::with_options()
//! let mut map = map.into_map_mut()?;
//! map[..4].clone_from_slice(b"that");
//!
//! // Flush the changes to disk synchronously
//! map.flush(&file, Flush::Sync)?;
//!
//! // Move the MapMut back into a Map
//! let map = map.into_map()?;
//! assert_eq!(Ok("that is a test"), from_utf8(&map[..]));
//! # Ok(())
//! # }
//! ```
//!
//! This library contains a [`Ring`] that constructs a circular memory
//! allocation where values can wrap from around from the end of the buffer back
//! to the beginning with sequential memory addresses. The [`InfiniteRing`] is
//! similar, however it allows writes to overwrite reads.
//!
//! ```
//! use vmap::io::{Ring, SeqWrite};
//! use std::io::{BufRead, Read, Write};
//!
//! # fn main() -> std::io::Result<()> {
//! let mut buf = Ring::new(4000).unwrap();
//! let mut i = 1;
//!
//! // Fill up the buffer with lines.
//! while buf.write_len() > 20 {
//!     write!(&mut buf, "this is test line {}\n", i)?;
//!     i += 1;
//! }
//!
//! // No more space is available.
//! assert!(write!(&mut buf, "this is test line {}\n", i).is_err());
//!
//! let mut line = String::new();
//!
//! // Read the first line written.
//! let len = buf.read_line(&mut line)?;
//! assert_eq!(line, "this is test line 1\n");
//!
//! line.clear();
//!
//! // Read the second line written.
//! let len = buf.read_line(&mut line)?;
//! assert_eq!(line, "this is test line 2\n");
//!
//! // Now there is enough space to write more.
//! write!(&mut buf, "this is test line {}\n", i)?;
//! # Ok(())
//! # }
//! ```
//!
//! [`.flush()`]: struct.MapMut.html#method.flush
//! [`.into_map()`]: struct.MapMut.html#method.into_map
//! [`.into_map_mut()`]: struct.Map.html#method.into_map_mut
//! [`BufReader`]: io/struct.BufReader.html
//! [`BufWriter`]: io/struct.BufWriter.html
//! [`InfiniteRing`]: io/struct.InfiniteRing.html
//! [`Map::with_options()`]: struct.Map.html#method.with_options
//! [`MapMut::with_options()`]: struct.MapMut.html#method.with_options
//! [`MapMut`]: struct.MapMut.html
//! [`Map`]: struct.Map.html
//! [`Options`]: struct.Options.html
//! [`Ring`]: io/struct.Ring.html
//! [`vmap::io`]: io/index.html

#![deny(missing_docs)]

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{mem, ptr};

#[cfg(feature = "os")]
pub mod os;

#[cfg(not(feature = "os"))]
mod os;

mod error;
pub use self::error::{ConvertResult, Error, Input, Operation, Result};

mod map;
pub use self::map::{Map, MapMut, Options};

#[cfg(feature = "io")]
pub mod io;

/// Protection level for a page.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Protect {
    /// The page(s) may only be read from.
    ReadOnly,
    /// The page(s) may be read from and written to.
    ReadWrite,
    /// Like `ReadWrite`, but changes are not shared.
    ReadCopy,
    /// The page(s) may be read from and executed.
    ReadExec,
}

/// Desired behavior when flushing write changes.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Flush {
    /// Request dirty pages to be written immediately and block until completed.
    Sync,
    /// Request dirty pages to be written but do not wait for completion.
    Async,
}

/// Hint for the access pattern of the underlying mapping.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Advise {
    /// Use the system default behavior.
    Normal,
    /// The map will be accessed in a sequential manner.
    Sequential,
    /// The map will be accessed in a random manner.
    Random,
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
/// let size = vmap::Size::alloc();
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
    pub fn alloc() -> Self {
        unsafe { Self::with_size(allocation_size()) }
    }

    /// Creates a type for calculating allocations numbers and byte offsets
    /// using a known size.
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

    /// Round a byte size up to the nearest unit size.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Size;
    ///
    /// let sys = vmap::page_size();
    /// let size = Size::page();
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

    /// Round a byte size down to the nearest unit size.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Size;
    ///
    /// let sys = vmap::page_size();
    /// let size = Size::page();
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

    /// Calculate the byte offset from size unit containing the position.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Size;
    ///
    /// let sys = vmap::page_size();
    /// let size = Size::page();
    /// assert_eq!(size.offset(1), 1);
    /// assert_eq!(size.offset(sys-1), sys-1);
    /// assert_eq!(size.offset(sys*2 + 123), 123);
    /// ```
    #[inline]
    pub fn offset(&self, len: usize) -> usize {
        len & (self.0 - 1)
    }

    /// Convert a unit count into a byte size.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Size;
    ///
    /// let sys = vmap::page_size();
    /// let size = Size::page();
    /// assert_eq!(size.size(0), 0);
    /// assert_eq!(size.size(1), sys);   // probably 4096
    /// assert_eq!(size.size(2), sys*2); // probably 8192
    /// ```
    #[inline]
    pub fn size(&self, count: u32) -> usize {
        (count as usize) << self.0.trailing_zeros()
    }

    /// Covert a byte size into the number of units necessary to contain it.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Size;
    ///
    /// let sys = vmap::page_size();
    /// let size = Size::page();
    /// assert_eq!(size.count(0), 0);
    /// assert_eq!(size.count(1), 1);
    /// assert_eq!(size.count(sys-1), 1);
    /// assert_eq!(size.count(sys), 1);
    /// assert_eq!(size.count(sys+1), 2);
    /// assert_eq!(size.count(sys*2), 2);
    /// ```
    #[inline]
    pub fn count(&self, len: usize) -> u32 {
        (self.round(len) >> self.0.trailing_zeros()) as u32
    }

    /// Calculates the unit bounds for a pointer and length.
    ///
    /// # Safety
    ///
    /// There is no verification that the pointer is a mapped page nor that
    /// the calculated offset may be dereferenced.
    #[inline]
    pub unsafe fn bounds(&self, ptr: *mut u8, len: usize) -> (*mut u8, usize) {
        let off = self.offset(ptr as usize);
        (ptr.offset(-(off as isize)), self.round(len + off))
    }
}

impl Default for Size {
    fn default() -> Self {
        Self::alloc()
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

    /// Performs a volatile read of the value at a given offset.
    ///
    /// Volatile operations are intended to act on I/O memory, and are
    /// guaranteed to not be elided or reordered by the compiler across
    /// other volatile operations.
    #[inline]
    fn read_volatile<T: sealed::Scalar>(&self, offset: usize) -> T {
        assert_capacity::<T>(offset, self.len());
        assert_alignment::<T>(offset, self.as_ptr());
        unsafe { ptr::read_volatile(self.as_ptr().add(offset) as *const T) }
    }

    /// Performs an unaligned read of the value at a given offset.
    #[inline]
    fn read_unaligned<T: sealed::Scalar>(&self, offset: usize) -> T {
        assert_capacity::<T>(offset, self.len());
        unsafe { ptr::read_unaligned(self.as_ptr().add(offset) as *const T) }
    }
}

/// General trait for working with any memory-safe representation of a
/// contiguous region of arbitrary mutable memory.
pub trait SpanMut: Span + DerefMut {
    /// Get a mutable pointer to the start of the allocated region.
    fn as_mut_ptr(&mut self) -> *mut u8;

    /// Performs a volatile write of the value at a given offset.
    ///
    /// Volatile operations are intended to act on I/O memory, and are
    /// guaranteed to not be elided or reordered by the compiler across
    /// other volatile operations.
    #[inline]
    fn write_volatile<T: sealed::Scalar>(&mut self, offset: usize, value: T) {
        assert_capacity::<T>(offset, self.len());
        assert_alignment::<T>(offset, self.as_ptr());
        unsafe { ptr::write_volatile(self.as_mut_ptr().add(offset) as *mut T, value) }
    }

    /// Performs an unaligned write of the value at a given offset.
    #[inline]
    fn write_unaligned<T: sealed::Scalar>(&mut self, offset: usize, value: T) {
        assert_capacity::<T>(offset, self.len());
        unsafe { ptr::write_unaligned(self.as_mut_ptr().add(offset) as *mut T, value) }
    }
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

    pub trait Scalar: Default {}

    impl Scalar for u8 {}
    impl Scalar for i8 {}
    impl Scalar for u16 {}
    impl Scalar for i16 {}
    impl Scalar for u32 {}
    impl Scalar for i32 {}
    impl Scalar for u64 {}
    impl Scalar for i64 {}
    impl Scalar for u128 {}
    impl Scalar for i128 {}
    impl Scalar for usize {}
    impl Scalar for isize {}
    impl Scalar for f32 {}
    impl Scalar for f64 {}
}

#[inline]
fn assert_alignment<T>(offset: usize, ptr: *const u8) {
    if unsafe { ptr.add(offset) } as usize % mem::align_of::<T>() != 0 {
        panic!(
            "offset improperly aligned: the requirement is {} but the offset is +{}/-{}",
            mem::align_of::<T>(),
            ptr as usize % mem::align_of::<T>(),
            mem::align_of::<T>() - (ptr as usize % mem::align_of::<T>()),
        )
    }
}

#[inline]
fn assert_capacity<T>(offset: usize, len: usize) {
    if offset + mem::size_of::<T>() > len {
        panic!(
            "index out of bounds: the len is {} but the index is {}",
            len,
            offset + mem::size_of::<T>()
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
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
        let sz = Size::alloc();

        let mut map = MapMut::with_options().len(Extent::Min(100)).alloc()?;
        assert_eq!(map.len(), sz.round(100));
        assert_eq!(Ok("\0\0\0\0\0"), from_utf8(&map[..5]));

        map[..5].clone_from_slice(b"hello");
        assert_eq!(Ok("hello"), from_utf8(&map[..5]));
        Ok(())
    }

    #[test]
    fn alloc_exact() -> Result<()> {
        let mut map = MapMut::with_options().len(5).alloc()?;
        assert_eq!(map.len(), 5);
        assert_eq!(Ok("\0\0\0\0\0"), from_utf8(&map[..]));

        map[..5].clone_from_slice(b"hello");
        assert_eq!(Ok("hello"), from_utf8(&map[..]));
        Ok(())
    }

    #[test]
    fn alloc_offset() -> Result<()> {
        // map to the offset of the last 5 bytes of an allocation size, but map 6 bytes
        let off = Size::alloc().size(1) - 5;
        let mut map = MapMut::with_options().offset(off).len(6).alloc()?;

        // force the page after the 5 bytes to be read-only
        unsafe { os::protect(map.as_mut_ptr().add(5), 1, Protect::ReadOnly)? };

        assert_eq!(map.len(), 6);
        assert_eq!(Ok("\0\0\0\0\0\0"), from_utf8(&map[..]));

        // writing one more byte will segfault
        map[..5].clone_from_slice(b"hello");
        assert_eq!(Ok("hello\0"), from_utf8(&map[..]));
        Ok(())
    }

    #[test]
    fn read_end() -> Result<()> {
        let (_tmp, path, len) = write_default("read_end")?;
        let (map, _) = Map::with_options().offset(29).open(&path)?;
        assert!(map.len() >= 30);
        assert_eq!(len - 29, map.len());
        assert_eq!(Ok("fast and safe memory-mapped IO"), from_utf8(&map[..30]));
        Ok(())
    }

    #[test]
    fn read_min() -> Result<()> {
        let (_tmp, path, len) = write_default("read_min")?;
        let (map, _) = Map::with_options()
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
        let (map, _) = Map::with_options()
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
        let (map, _) = Map::with_options().offset(29).len(30).open(&path)?;
        assert!(map.len() == 30);
        assert_eq!(Ok("fast and safe memory-mapped IO"), from_utf8(&map[..]));
        Ok(())
    }

    #[test]
    fn copy() -> Result<()> {
        let (_tmp, path, _len) = write_default("copy")?;
        let (mut map, _) = MapMut::with_options()
            .offset(29)
            .len(30)
            .copy()
            .open(&path)?;
        assert_eq!(map.len(), 30);
        assert_eq!(Ok("fast and safe memory-mapped IO"), from_utf8(&map[..]));

        map[..4].clone_from_slice(b"nice");
        assert_eq!(Ok("nice and safe memory-mapped IO"), from_utf8(&map[..]));
        Ok(())
    }

    #[test]
    fn write_into_mut() -> Result<()> {
        let tmp = tempdir::TempDir::new("vmap")?;
        let path: PathBuf = tmp.path().join("write_into_mut");
        fs::write(&path, "this is a test").expect("failed to write file");

        let (map, _) = Map::with_options().write().resize(16).open(&path)?;
        assert_eq!(16, map.len());
        assert_eq!(Ok("this is a test"), from_utf8(&map[..14]));
        assert_eq!(Ok("this is a test\0\0"), from_utf8(&map[..]));

        let mut map = map.into_map_mut()?;
        map[..4].clone_from_slice(b"that");
        assert_eq!(Ok("that is a test"), from_utf8(&map[..14]));
        assert_eq!(Ok("that is a test\0\0"), from_utf8(&map[..]));

        let map = map.into_map()?;
        assert_eq!(Ok("that is a test"), from_utf8(&map[..14]));
        assert_eq!(Ok("that is a test\0\0"), from_utf8(&map[..]));

        let (map, _) = Map::with_options().open(&path)?;
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

        let (map, _) = Map::with_options()
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

    #[test]
    fn volatile() -> Result<()> {
        let tmp = tempdir::TempDir::new("vmap")?;
        let path: PathBuf = tmp.path().join("volatile");

        let (mut map, _) = MapMut::with_options()
            .write()
            .truncate(true)
            .create(true)
            .resize(16)
            .open(&path)?;
        assert_eq!(16, map.len());

        assert_eq!(0u64, map.read_volatile(0));
        assert_eq!(0u64, map.read_volatile(8));

        map.write_volatile(0, 0xc3a5c85c97cb3127u64);
        map.write_volatile(8, 0xb492b66fbe98f273u64);

        assert_eq!(0xc3a5c85c97cb3127u64, map.read_volatile(0));
        assert_eq!(0xb492b66fbe98f273u64, map.read_volatile(8));

        let (map, _) = Map::with_options().open(&path)?;
        assert_eq!(16, map.len());
        assert_eq!(0xc3a5c85c97cb3127u64, map.read_volatile(0));
        assert_eq!(0xb492b66fbe98f273u64, map.read_volatile(8));

        Ok(())
    }

    #[test]
    fn unaligned() -> Result<()> {
        let tmp = tempdir::TempDir::new("vmap")?;
        let path: PathBuf = tmp.path().join("unaligned");

        let (mut map, _) = MapMut::with_options()
            .write()
            .truncate(true)
            .create(true)
            .resize(17)
            .open(&path)?;
        assert_eq!(17, map.len());

        assert_eq!(0u64, map.read_unaligned(1));
        assert_eq!(0u64, map.read_unaligned(9));

        map.write_unaligned(1, 0xc3a5c85c97cb3127u64);
        map.write_unaligned(9, 0xb492b66fbe98f273u64);

        assert_eq!(0xc3a5c85c97cb3127u64, map.read_unaligned(1));
        assert_eq!(0xb492b66fbe98f273u64, map.read_unaligned(9));

        let (map, _) = Map::with_options().open(&path)?;
        assert_eq!(17, map.len());
        assert_eq!(0xc3a5c85c97cb3127u64, map.read_unaligned(1));
        assert_eq!(0xb492b66fbe98f273u64, map.read_unaligned(9));

        Ok(())
    }
}
