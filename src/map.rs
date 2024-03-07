use std::convert::TryFrom;
use std::fs::{File, OpenOptions};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::slice;
use std::{cmp, fmt, io, marker};

use crate::os::{advise, flush, lock, map_anon, map_file, protect, unlock, unmap};
use crate::sealed::FromPtr;
use crate::{
    Advise, ConvertResult, Error, Extent, Flush, Input, Operation, Protect, Result, Size, Span,
    SpanMut,
};

/// Allocation of one or more read-only sequential pages.
///
/// # Examples
///
/// ```
/// use vmap::{Map, Advise};
/// use std::path::PathBuf;
/// use std::str::from_utf8;
///
/// # fn main() -> vmap::Result<()> {
/// # let tmp = tempdir::TempDir::new("vmap")?;
/// let path: PathBuf = /* path to file */
/// # tmp.path().join("example");
/// # std::fs::write(&path, "A cross-platform library for fast and safe memory-mapped IO in Rust")?;
/// let (map, file) = Map::with_options().offset(29).len(30).open(&path)?;
/// map.advise(Advise::Sequential)?;
/// assert_eq!(Ok("fast and safe memory-mapped IO"), from_utf8(&map[..]));
/// assert_eq!(Ok("safe"), from_utf8(&map[9..13]));
/// # Ok(())
/// # }
/// ```
pub struct Map(MapMut);

impl Map {
    /// Returns a new [`Options`] object to create a read-only `Map`.
    ///
    /// When used to [`.open()`] a path or [`.map()`] a file, the default
    /// [`Options`] object is assumed to cover the entire file.
    ///
    /// See the [`Options`] type for details on options for modifying the file
    /// size, specifying offset positions, and selecting specific lengths.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Map;
    /// use std::path::PathBuf;
    /// use std::str::from_utf8;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    /// # std::fs::write(&path, "A cross-platform library for fast and safe memory-mapped IO in Rust")?;
    /// let (map, file) = Map::with_options()
    ///     .offset(29)
    ///     .len(30)
    ///     .open(&path)?;
    /// assert_eq!(Ok("fast and safe memory-mapped IO"), from_utf8(&map));
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_options() -> Options<Self> {
        Options::new()
    }

    /// Transfer ownership of the map into a mutable map.
    ///
    /// This will change the protection of the mapping. If the original file
    /// was not opened with write permissions, this will error.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Map;
    /// use std::fs::OpenOptions;
    /// use std::path::PathBuf;
    /// use std::str::from_utf8;
    /// # use std::fs;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    /// # fs::write(&path, b"this is a test")?;
    ///
    /// // Map the beginning of the file
    /// let (map, file) = Map::with_options().write().len(14).open(path)?;
    /// assert_eq!(Ok("this is a test"), from_utf8(&map[..]));
    ///
    /// let mut map = map.into_map_mut()?;
    /// map[..4].clone_from_slice(b"that");
    /// assert_eq!(Ok("that is a test"), from_utf8(&map[..]));
    /// # Ok(())
    /// # }
    /// ```
    pub fn into_map_mut(self) -> ConvertResult<MapMut, Self> {
            let (ptr, len) = unsafe { Size::page().bounds(self.0.ptr, self.0.len) };
            match unsafe { protect(ptr, len, Protect::ReadWrite) }{
                Ok(()) => Ok(self.0),
                Err(err) => Err((err, self)),
            }
    }

    /// Updates the advise for the entire mapped region..
    pub fn advise(&self, adv: Advise) -> Result<()> {
        self.0.advise(adv)
    }

    /// Updates the advise for a specific range of the mapped region.
    pub fn advise_range(&self, off: usize, len: usize, adv: Advise) -> Result<()> {
        self.0.advise_range(off, len, adv)
    }

    /// Lock all mapped physical pages into memory.
    pub fn lock(&self) -> Result<()> {
        self.0.lock()
    }

    /// Lock a range of physical pages into memory.
    pub fn lock_range(&self, off: usize, len: usize) -> Result<()> {
        self.0.lock_range(off, len)
    }

    /// Unlock all mapped physical pages into memory.
    pub fn unlock(&self) -> Result<()> {
        self.0.unlock()
    }

    /// Unlock a range of physical pages into memory.
    pub fn unlock_range(&self, off: usize, len: usize) -> Result<()> {
        self.0.unlock_range(off, len)
    }
}

impl FromPtr for Map {
    unsafe fn from_ptr(ptr: *mut u8, len: usize) -> Self {
        Self(MapMut::from_ptr(ptr, len))
    }
}

impl Span for Map {
    #[inline]
    fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    fn as_ptr(&self) -> *const u8 {
        self.0.as_ptr()
    }
}

impl Deref for Map {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.0.ptr, self.0.len) }
    }
}

impl AsRef<[u8]> for Map {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

impl TryFrom<MapMut> for Map {
    type Error = (Error, MapMut);

    fn try_from(map: MapMut) -> ConvertResult<Self, MapMut> {
        map.into_map()
    }
}

impl fmt::Debug for Map {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Map")
            .field("ptr", &self.0.ptr)
            .field("len", &self.0.len)
            .finish()
    }
}

/// Allocation of one or more read-write sequential pages.
#[derive(Debug)]
pub struct MapMut {
    ptr: *mut u8,
    len: usize,
}

impl MapMut {
    /// Returns a new `Options` object to create a writable `MapMut`.
    ///
    /// When used to [`.open()`] a path or [`.map()`] a file, the default
    /// [`Options`] object is assumed to cover the entire file.
    ///
    /// See the [`Options`] type for details on options for modifying the file
    /// size, specifying offset positions, and selecting specific lengths.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::{MapMut, Flush};
    /// use std::path::PathBuf;
    /// use std::str::from_utf8;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    /// # std::fs::write(&path, "A cross-platform library for fast and safe memory-mapped IO in Rust")?;
    /// let (mut map, file) = MapMut::with_options()
    ///     .offset(29)
    ///     .len(30)
    ///     .open(&path)?;
    /// assert_eq!(Ok("fast and safe memory-mapped IO"), from_utf8(&map));
    /// map[..4].clone_from_slice(b"nice");
    ///
    /// map.flush_range(&file, 0, 4, Flush::Sync);
    ///
    /// assert_eq!(Ok("nice and safe memory-mapped IO"), from_utf8(&map));
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_options() -> Options<Self> {
        let mut opts = Options::new();
        opts.write();
        opts
    }

    /// Create a new anonymous mapping at least as large as the hint.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::{MapMut, Protect};
    /// use std::str::from_utf8;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// let mut map = MapMut::new(200)?;
    /// map[..4].clone_from_slice(b"test");
    /// assert_eq!(Ok("test"), from_utf8(&map[..4]));
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(hint: usize) -> Result<Self> {
        Self::with_options().len(Extent::Min(hint)).alloc()
    }

    /// Transfer ownership of the map into a mutable map.
    ///
    /// This will change the protection of the mapping. If the original file
    /// was not opened with write permissions, this will error.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::MapMut;
    /// use std::fs::OpenOptions;
    /// use std::path::PathBuf;
    /// use std::str::from_utf8;
    /// # use std::fs;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    /// # fs::write(&path, b"this is a test")?;
    /// let (mut map, file) = MapMut::with_options().len(14).open(&path)?;
    /// assert_eq!(Ok("this is a test"), from_utf8(&map[..]));
    ///
    /// map[..4].clone_from_slice(b"that");
    ///
    /// let map = map.into_map()?;
    /// assert_eq!(Ok("that is a test"), from_utf8(&map[..]));
    /// # Ok(())
    /// # }
    /// ```
    pub fn into_map(self) -> ConvertResult<Map, Self> {
            let (ptr, len) = unsafe { Size::page().bounds(self.ptr, self.len) };
            match unsafe { protect(ptr, len, Protect::ReadWrite) }{
                Ok(()) => Ok(Map(self)),
                Err(err) => Err((err, self)),
            }
    }

    /// Writes modifications back to the filesystem.
    ///
    /// Flushes will happen automatically, but this will invoke a flush and
    /// return any errors with doing so.
    pub fn flush(&self, file: &File, mode: Flush) -> Result<()> {
        unsafe {
            let (ptr, len) = Size::page().bounds(self.ptr, self.len);
            flush(ptr, file, len, mode)
        }
    }

    /// Writes modifications back to the filesystem for a sub-range of the map.
    ///
    /// Flushes will happen automatically, but this will invoke a flush and
    /// return any errors with doing so.
    pub fn flush_range(&self, file: &File, off: usize, len: usize, mode: Flush) -> Result<()> {
        if off + len > self.len {
            Err(Error::input(Operation::Flush, Input::InvalidRange))
        } else {
            unsafe {
                let (ptr, len) = Size::page().bounds(self.ptr.add(off), len);
                flush(ptr, file, len, mode)
            }
        }
    }

    /// Updates the advise for the entire mapped region..
    pub fn advise(&self, adv: Advise) -> Result<()> {
        unsafe {
            let (ptr, len) = Size::page().bounds(self.ptr, self.len);
            advise(ptr, len, adv)
        }
    }

    /// Updates the advise for a specific range of the mapped region.
    pub fn advise_range(&self, off: usize, len: usize, adv: Advise) -> Result<()> {
        if off + len > self.len {
            Err(Error::input(Operation::Advise, Input::InvalidRange))
        } else {
            unsafe {
                let (ptr, len) = Size::page().bounds(self.ptr.add(off), len);
                advise(ptr, len, adv)
            }
        }
    }

    /// Lock all mapped physical pages into memory.
    pub fn lock(&self) -> Result<()> {
        unsafe {
            let (ptr, len) = Size::page().bounds(self.ptr, self.len);
            lock(ptr, len)
        }
    }

    /// Lock a range of physical pages into memory.
    pub fn lock_range(&self, off: usize, len: usize) -> Result<()> {
        if off + len > self.len {
            Err(Error::input(Operation::Lock, Input::InvalidRange))
        } else {
            unsafe {
                let (ptr, len) = Size::page().bounds(self.ptr.add(off), len);
                lock(ptr, len)
            }
        }
    }

    /// Unlock all mapped physical pages into memory.
    pub fn unlock(&self) -> Result<()> {
        unsafe {
            let (ptr, len) = Size::page().bounds(self.ptr, self.len);
            unlock(ptr, len)
        }
    }

    /// Unlock a range of physical pages into memory.
    pub fn unlock_range(&self, off: usize, len: usize) -> Result<()> {
        if off + len > self.len {
            Err(Error::input(Operation::Unlock, Input::InvalidRange))
        } else {
            unsafe {
                let (ptr, len) = Size::page().bounds(self.ptr.add(off), len);
                unlock(ptr, len)
            }
        }
    }
}

impl FromPtr for MapMut {
    unsafe fn from_ptr(ptr: *mut u8, len: usize) -> Self {
        Self { ptr, len }
    }
}

impl Span for MapMut {
    #[inline]
    fn len(&self) -> usize {
        self.len
    }

    #[inline]
    fn as_ptr(&self) -> *const u8 {
        self.ptr
    }
}

impl SpanMut for MapMut {
    #[inline]
    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for MapMut {
    fn drop(&mut self) {
        unsafe {
            if self.len > 0 {
                let (ptr, len) = Size::alloc().bounds(self.ptr, self.len);
                unmap(ptr, len).unwrap_or_default();
            }
        }
    }
}

impl Deref for MapMut {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl DerefMut for MapMut {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl AsRef<[u8]> for MapMut {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

impl AsMut<[u8]> for MapMut {
    #[inline]
    fn as_mut(&mut self) -> &mut [u8] {
        self.deref_mut()
    }
}

impl TryFrom<Map> for MapMut {
    type Error = (Error, Map);

    fn try_from(map: Map) -> ConvertResult<Self, Map> {
        map.into_map_mut()
    }
}

/// Options and flags which can be used to configure how a map is allocated.
///
/// This builder exposes the ability to configure how a [`Map`] or a [`MapMut`]
/// is allocated. These options can be used to either map a file or allocate
/// an anonymous memory region. For file-based operations, a `std::fs::OpenOptions`
/// value is maintained to match the desired abilities between the mapping and
/// the underlying resource. This allows the creation, truncation, and resizing
/// of a file to be coordinated when allocating a named map. For both mapping
/// and anonymous allocations the option can also specify an offset and a
/// mapping length.
///
/// The `T` must either be a [`Map`] or a [`MapMut`]. Generally, this will be
/// created by [`Map::with_options()`] or [`MapMut::with_options()`], then
/// chain calls to methods to set each option, then call either [`.open()`],
/// [`.map()`], or [`.alloc()`]. This will return a [`Result`] with the correct
/// [`Map`] or [`MapMut`] inside. Additionally, there are [`.open_if()`] and
/// [`.map_if()`] variations which instead return a [`Result`] containing an
/// `Option<T>`. These return `Ok(None)` if the attempted range lies outside
/// of the file rather than an `Err`.
///
/// Without specifying a size, the options defaults to either the full size of
/// the file when using [`.open()`] or [`.map()`]. When using [`.alloc()`], the default
/// size will be a single unit of allocation granularity.
///
/// [`Map`]: struct.Map.html
/// [`MapMut`]: struct.MapMut.html
/// [`Map::with_options()`]: struct.Map.html#method.with_options
/// [`MapMut::with_options()`]: struct.MapMut.html#method.with_options
/// [`.open()`]: #method.open
/// [`.open_if()`]: #method.open_if
/// [`.map()`]: #method.map
/// [`.map_if()`]: #method.map_if
/// [`.alloc()`]: #method.alloc
/// [`Result`]: type.Result.html
pub struct Options<T: FromPtr> {
    open_options: OpenOptions,
    resize: Extent,
    len: Extent,
    offset: usize,
    protect: Protect,
    truncate: bool,
    _marker: marker::PhantomData<fn() -> T>,
}

impl<T: FromPtr> Options<T> {
    /// Creates a new [`Options`] value with a default state.
    ///
    /// Generally, [`Map::with_options()`] or [`MapMut::with_options()`] is the
    /// preferred way to create options.
    ///
    /// [`Options`]: struct.Options.html
    /// [`Map::with_options()`]: struct.Map.html#method.with_options
    /// [`MapMut::with_options()`]: struct.MapMut.html#method.with_options
    pub fn new() -> Self {
        let mut open_options = OpenOptions::new();
        open_options.read(true);
        Self {
            open_options,
            resize: Extent::End,
            len: Extent::End,
            offset: 0,
            protect: Protect::ReadOnly,
            truncate: false,
            _marker: marker::PhantomData,
        }
    }

    /// Sets the option for write access.
    ///
    /// This is applied automatically when using [`MapMut::with_options()`].
    /// This can be useful with [`Map`] when there is a future intent to call
    /// [`Map::into_map_mut()`].
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Map;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// let (map, file) = Map::with_options().open("README.md")?;
    /// assert!(map.into_map_mut().is_err());
    ///
    /// let (map, file) = Map::with_options().write().open("README.md")?;
    /// assert!(map.into_map_mut().is_ok());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`MapMut::with_options()`]: struct.MapMut.html#method.with_options
    /// [`Map`]: struct.Map.html
    /// [`Map::into_map_mut()`]: struct.Map.html#method.into_map_mut
    pub fn write(&mut self) -> &mut Self {
        self.open_options.write(true);
        self.protect = Protect::ReadWrite;
        self
    }

    /// Sets the option for copy-on-write access.
    ///
    /// This efficiently implements a copy to an underlying modifiable
    /// resource. The allocated memory can be shared between multiple
    /// unmodified instances, and the copy operation is deferred until the
    /// first write. When used for an anonymous allocation, the deffered copy
    /// can be used in a child process.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::MapMut;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// let (mut map1, file) = MapMut::with_options().copy().open("README.md")?;
    /// let (mut map2, _) = MapMut::with_options().copy().open("README.md")?;
    /// let first = map1[0];
    ///
    /// map1[0] = b'X';
    ///
    /// assert_eq!(first, map2[0]);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`MapMut::with_options()`]: struct.MapMut.html#method.with_options
    /// [`Map`]: struct.Map.html
    /// [`Map::into_map_mut()`]: struct.Map.html#method.into_map_mut
    pub fn copy(&mut self) -> &mut Self {
        self.open_options.write(false);
        self.protect = Protect::ReadCopy;
        self
    }

    /// Sets the option to create a new file, or open it if it already exists.
    ///
    /// This only applies when using [`.open()`] or [`.open_if()`]. In order for the
    /// file to be created, [`.write()`] access must be used.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::{Map, MapMut};
    /// use std::path::PathBuf;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    /// let (mut map, file) = MapMut::with_options().create(true).resize(100).open(&path)?;
    /// assert_eq!(100, map.len());
    /// assert_eq!(b"\0\0\0\0", &map[..4]);
    ///
    /// map[..4].clone_from_slice(b"test");
    ///
    /// let (map, file) = Map::with_options().open(&path)?;
    /// assert_eq!(100, map.len());
    /// assert_eq!(b"test", &map[..4]);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`.open()`]: #method.open
    /// [`.open_if()`]: #method.open_if
    /// [`.write()`]: #method.write
    pub fn create(&mut self, create: bool) -> &mut Self {
        self.open_options.create(create);
        self
    }

    /// Sets the option to create a new file, failing if it already exists.
    ///
    /// This option is useful because it is atomic. Otherwise between checking
    /// whether a file exists and creating a new one, the file may have been
    /// created by another process (a TOCTOU race condition / attack).
    ///
    /// If `.create_new(true)` is set, [`.create()`] and [`.truncate()`] are
    /// ignored.
    ///
    /// This only applies when using [`.open()`] or [`.open_if()`]. In order for the
    /// file to be created, [`.write()`] access must be used.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::MapMut;
    /// use std::path::PathBuf;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    ///
    /// let (map, file) = MapMut::with_options().create_new(true).resize(10).open(&path)?;
    /// assert_eq!(10, map.len());
    /// assert!(MapMut::with_options().create_new(true).open(&path).is_err());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`.create()`]: #method.create
    /// [`.truncate()`]: #method.truncate
    /// [`.open()`]: #method.open
    /// [`.open_if()`]: #method.open_if
    /// [`.write()`]: #method.write
    pub fn create_new(&mut self, create_new: bool) -> &mut Self {
        self.open_options.create_new(create_new);
        self
    }

    /// Sets the option for truncating a previous file.
    ///
    /// If a file is successfully opened with this option set it will truncate
    /// the file to 0 length if it already exists. Given that the file will now
    /// be empty, a [`.resize()`] should be used.
    ///
    /// In order for the file to be truncated, [`.write()`] access must be used.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::MapMut;
    /// use std::path::PathBuf;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    ///
    /// {
    ///     let (mut map, file) = MapMut::with_options()
    ///         .create(true)
    ///         .truncate(true)
    ///         .resize(4)
    ///         .open(&path)?;
    ///     assert_eq!(b"\0\0\0\0", &map[..]);
    ///     map[..4].clone_from_slice(b"test");
    ///     assert_eq!(b"test", &map[..]);
    /// }
    ///
    /// let (mut map, file) = MapMut::with_options().truncate(true).resize(4).open(&path)?;
    /// assert_eq!(b"\0\0\0\0", &map[..]);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`.resize()`]: #method.resize
    /// [`.write()`]: #method.write
    pub fn truncate(&mut self, truncate: bool) -> &mut Self {
        self.open_options.truncate(truncate);
        self.truncate = truncate;
        self
    }

    /// Sets the byte offset into the mapping.
    ///
    /// For file-based mappings, the offset defines the starting byte range
    /// from the beginning of the resource. This must be within the range of
    /// the file.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Map;
    /// use std::path::PathBuf;
    /// use std::str::from_utf8;
    /// use std::fs;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    /// fs::write(&path, b"this is a test")?;
    ///
    /// let (map, file) = Map::with_options().offset(10).open(path)?;
    /// assert_eq!(Ok("test"), from_utf8(&map[..]));
    /// # Ok(())
    /// # }
    /// ```
    pub fn offset(&mut self, offset: usize) -> &mut Self {
        self.offset = offset;
        self
    }

    /// Sets the byte length extent of the mapping.
    ///
    /// For file-based mappings, this length must be available in the
    /// underlying resource, including any [`.offset()`]. When not specified,
    /// the default length is implied to be [`Extent::End`].
    ///
    /// # Length with `Extent::End`
    ///
    /// With this value, the length extent is set to the end of the underlying
    /// resource. This is the default if no `.len()` is applied, but this can
    /// be set to override a prior setting if desired.
    ///
    /// For anonymous mappings, it is generally preferred to use a different
    /// extent strategy. Without setting any other extent, the default length
    /// is a single allocation unit of granularity.
    ///
    /// # Length with `Extent::Exact`
    ///
    /// Using an exact extent option will instruct the map to cover an exact
    /// byte length. That is, it will not consider the length of the underlying
    /// resource, if any. For file-based mappings, this length must be
    /// available in the file. For anonymous mappings, this is the minimum size
    /// that will be allocated, however, the resulting map will be sized
    /// exactly to this size.
    ///
    /// A `usize` may be used as an [`Extent::Exact`] through the `usize`
    /// implementation of [`Into<Extent>`].
    ///
    /// ```
    /// use vmap::{Map, MapMut};
    /// use std::path::PathBuf;
    /// use std::str::from_utf8;
    /// use std::fs;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    /// fs::write(&path, b"this is a test")?;
    ///
    /// let (map, file) = Map::with_options()
    ///     .len(4) // or .len(Extent::Exaxt(4))
    ///     .open(&path)?;
    /// assert_eq!(Ok("this"), from_utf8(&map[..]));
    ///
    /// let mut anon = MapMut::with_options()
    ///     .len(4)
    ///     .alloc()?;
    /// assert_eq!(4, anon.len());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Length with `Extent::Min`
    ///
    /// The minimum extent strategy creates a mapping that is at least the
    /// desired byte length, but may be larger. When applied to a file-based
    /// mapping, this ensures that the resulting memory region covers a minimum
    /// extent, but otherwise covers to the end of the file. For an anonymous
    /// map, this ensures the allocated region meets the minimum size required,
    /// but allows accessing the remaining allocated space that would otherwise
    /// be unusable.
    ///
    /// ```
    /// use vmap::{Extent, Map, MapMut, Size};
    /// use std::path::PathBuf;
    /// use std::str::from_utf8;
    /// use std::fs;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    /// fs::write(&path, b"this is a test")?;
    ///
    /// let (map, file) = Map::with_options()
    ///     .offset(5)
    ///     .len(Extent::Min(4))
    ///     .open(&path)?;
    /// assert_eq!(9, map.len());
    /// assert_eq!(Ok("is a test"), from_utf8(&map[..]));
    ///
    /// assert!(
    ///     Map::with_options()
    ///         .len(Extent::Min(100))
    ///         .open_if(&path)?
    ///         .0
    ///         .is_none()
    /// );
    ///
    /// let mut anon = MapMut::with_options()
    ///     .len(Extent::Min(2000))
    ///     .alloc()?;
    /// assert_eq!(Size::alloc().size(1), anon.len());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Length with `Extent::Max`
    ///
    /// The maximum extent strategy creates a mapping that is no larger than
    /// the desired byte length, but may be smaller. When applied to a file-
    /// based mapping, this will ensure that the resulting
    ///
    /// ```
    /// use vmap::{Extent, Map, MapMut};
    /// use std::path::PathBuf;
    /// use std::str::from_utf8;
    /// use std::fs;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    /// fs::write(&path, b"this is a test")?;
    ///
    /// let (map, file) = Map::with_options()
    ///    .offset(5)
    ///    .len(Extent::Max(100))
    ///    .open(&path)?;
    /// assert_eq!(9, map.len());
    /// assert_eq!(Ok("is a test"), from_utf8(&map[..]));
    ///
    /// let mut anon = MapMut::with_options()
    ///     .len(Extent::Max(2000))
    ///     .alloc()?;
    /// assert_eq!(2000, anon.len());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`Into<Extent>`]: enum.Extent.html#impl-From<usize>
    /// [`Extent::End`]: enum.Extent.html#variant.End
    /// [`Extent::Exact`]: enum.Extent.html#variant.Exact
    /// [`Extent::Min`]: enum.Extent.html#variant.Min
    /// [`Extent::Max`]: enum.Extent.html#variant.Max
    /// [`.offset()`]: #method.offset
    /// [`.len_min()`]: #method.len_min
    /// [`.len_max()`]: #method.len_max
    pub fn len<E: Into<Extent>>(&mut self, value: E) -> &mut Self {
        self.len = value.into();
        self
    }

    /// Sets the option to resize the file prior to mapping.
    ///
    /// When mapping to a file using [`.open()`], [`.open_if()`], [`.map()`],
    /// or [`.map_if()`] this options conditionally adjusts the length of the
    /// underlying resource to the desired size by calling [`.set_len()`] on
    /// the [`File`].
    ///
    /// In order for the file to be resized, [`.write()`] access must be used.
    ///
    /// This has no affect on anonymous mappings.
    ///
    /// # Resize with `Extent::End`
    ///
    /// This implies resizing to the current size of the file. In other words,
    /// no resize is performed, and this is the default strategy.
    ///
    /// # Resize with `Extent::Exact`
    ///
    /// Using an exact extent option will instruct the map to cover an exact
    /// byte length. That is, it will not consider the length of the underlying
    /// resource, if any. For file-based mappings, this length must be
    /// available in the file. For anonymous mappings, this is the minimum size
    /// that will be allocated, however, the resulting map will be sized
    /// exactly to this size.
    ///
    /// A `usize` may be used as an [`Extent::Exact`] through the `usize`
    /// implementation of [`Into<Extent>`].
    ///
    /// ```
    /// use vmap::Map;
    /// use std::path::PathBuf;
    /// use std::str::from_utf8;
    /// use std::fs;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    /// fs::write(&path, b"this is a test")?;
    ///
    /// let (map, file) = Map::with_options()
    ///     .write()
    ///     .resize(7) // or .resize(Extent::Exact(7))
    ///     .open(&path)?;
    /// assert_eq!(7, map.len());
    /// assert_eq!(Ok("this is"), from_utf8(&map[..]));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Resize with `Extent::Min`
    ///
    /// The minimum extent strategy resizes the file to be at least the
    /// desired byte length, but may be larger. If the file is already equal
    /// to or larger than the extent, no resize is performed.
    ///
    /// ```
    /// use vmap::{Extent, Map};
    /// use std::path::PathBuf;
    /// use std::str::from_utf8;
    /// use std::fs;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    ///
    /// fs::write(&path, b"this")?;
    ///
    /// {
    ///     let (map, file) = Map::with_options()
    ///         .write()
    ///         .resize(Extent::Min(7))
    ///         .open(&path)?;
    ///     assert_eq!(7, map.len());
    ///     assert_eq!(Ok("this\0\0\0"), from_utf8(&map[..]));
    /// }
    ///
    /// fs::write(&path, b"this is a test")?;
    ///
    /// let (map, file) = Map::with_options()
    ///     .write()
    ///     .resize(Extent::Min(7))
    ///     .open(&path)?;
    /// assert_eq!(14, map.len());
    /// assert_eq!(Ok("this is a test"), from_utf8(&map[..]));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Resize with `Extent::Max`
    ///
    /// The maximum extent strategy resizes the file to be no larger than the
    /// desired byte length, but may be smaller. If the file is already equal
    /// to or smaller than the extent, no resize is performed.
    ///
    /// ```
    /// use vmap::{Extent, Map};
    /// use std::path::PathBuf;
    /// use std::str::from_utf8;
    /// use std::fs;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    /// fs::write(&path, b"this")?;
    ///
    /// {
    ///     let (map, file) = Map::with_options()
    ///         .write()
    ///         .resize(Extent::Max(7))
    ///         .open(&path)?;
    ///     assert_eq!(4, map.len());
    ///     assert_eq!(Ok("this"), from_utf8(&map[..]));
    /// }
    ///
    /// fs::write(&path, b"this is a test")?;
    ///
    /// let (map, file) = Map::with_options()
    ///     .write()
    ///     .resize(Extent::Max(7))
    ///     .open(&path)?;
    /// assert_eq!(7, map.len());
    /// assert_eq!(Ok("this is"), from_utf8(&map[..]));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`.open()`]: #method.open
    /// [`.open_if()`]: #method.open_if
    /// [`.map()`]: #method.map
    /// [`.map_if()`]: #method.map_if
    /// [`.set_len()`]: https://doc.rust-lang.org/std/fs/struct.File.html#method.set_len
    /// [`File`]: https://doc.rust-lang.org/std/fs/struct.File.html
    /// [`.write()`]: #method.write
    /// [`Into<Extent>`]: enum.Extent.html#impl-From<usize>
    /// [`Extent::End`]: enum.Extent.html#variant.End
    /// [`Extent::Exact`]: enum.Extent.html#variant.Exact
    /// [`Extent::Min`]: enum.Extent.html#variant.Min
    /// [`Extent::Max`]: enum.Extent.html#variant.Max
    pub fn resize<E: Into<Extent>>(&mut self, value: E) -> &mut Self {
        self.resize = value.into();
        self
    }

    /// Opens and maps a file using the current options specified by `self`.
    ///
    /// Unlike [`.open_if()`], when the requested offset or length lies outside of
    /// the underlying file, an error is returned.
    ///
    /// The returned [`File`] can be discarded if no longer needed to [`.flush()`]
    /// or [`.map()`] other regions. This does not need to be kept open in order to
    /// use the mapped value.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Map;
    /// use std::path::PathBuf;
    /// use std::fs;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    /// fs::write(&path, b"this is a test")?;
    ///
    /// assert!(Map::with_options().len(4).open(&path).is_ok());
    /// assert!(Map::with_options().len(25).open(&path).is_err());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`.open_if()`]: #method.open_if
    /// [`.map()`]: #method.map
    /// [`.flush()`]: struct.MapMut.html#method.flush
    /// [`File`]: https://doc.rust-lang.org/std/fs/struct.File.html
    pub fn open<P: AsRef<Path>>(&self, path: P) -> Result<(T, File)> {
        let f = self.open_options.open(path).map_err(map_file_err)?;
        Ok((self.map(&f)?, f))
    }

    /// Opens and maps a file with the options specified by `self` if the
    /// provided byte range is valid.
    ///
    /// Unlike [`.open()`], when the requested offset or length lies outside of
    /// the underlying file, `Ok(None)` will be returned rather than an error.
    ///
    /// The returned [`File`] can be discarded if no longer needed to [`.flush()`]
    /// or [`.map()`] other regions. This does not need to be kept open in order to
    /// use the mapped value.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Map;
    /// use std::path::PathBuf;
    /// use std::fs;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    /// fs::write(&path, b"this is a test")?;
    ///
    /// assert!(Map::with_options().len(4).open_if(&path).is_ok());
    ///
    /// let result = Map::with_options().len(25).open_if(&path);
    /// assert!(result.is_ok());
    /// assert!(result.unwrap().0.is_none());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`.open()`]: #method.open
    /// [`.map()`]: #method.map
    /// [`.flush()`]: struct.MapMut.html#method.flush
    /// [`File`]: https://doc.rust-lang.org/std/fs/struct.File.html
    pub fn open_if<P: AsRef<Path>>(&self, path: P) -> Result<(Option<T>, File)> {
        let f = self.open_options.open(path).map_err(map_file_err)?;
        Ok((self.map_if(&f)?, f))
    }

    /// Maps an open `File` using the current options specified by `self`.
    ///
    /// Unlike [`.map_if()`], when the requested offset or length lies outside of
    /// the underlying file, an error is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Map;
    /// use std::path::PathBuf;
    /// use std::fs::OpenOptions;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    /// let f = OpenOptions::new()
    ///     .read(true)
    ///     .write(true)
    ///     .create(true)
    ///     .open(path)?;
    /// f.set_len(8)?;
    ///
    /// assert!(Map::with_options().len(4).map(&f).is_ok());
    /// assert!(Map::with_options().len(25).map(&f).is_err());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`.map_if()`]: #method.map_if
    pub fn map(&self, f: &File) -> Result<T> {
        self.map_if(f)?
            .ok_or_else(|| Error::input(Operation::MapFile, Input::InvalidRange))
    }

    /// Maps an open `File` with the options specified by `self` if the provided
    /// byte range is valid.
    ///
    /// Unlike [`.map()`], when the requested offset or length lies outside of
    /// the underlying file, `Ok(None)` will be returned rather than an error.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Map;
    /// use std::path::PathBuf;
    /// use std::fs::OpenOptions;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("example");
    /// let f = OpenOptions::new()
    ///     .read(true)
    ///     .write(true)
    ///     .create(true)
    ///     .open(path)?;
    /// f.set_len(8)?;
    ///
    /// assert!(Map::with_options().len(4).map_if(&f).is_ok());
    ///
    /// let result = Map::with_options().len(25).map_if(&f);
    /// assert!(result.is_ok());
    /// assert!(result.unwrap().is_none());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`.map()`]: #method.map
    pub fn map_if(&self, f: &File) -> Result<Option<T>> {
        let off = self.offset;
        let mut flen = f.metadata().map_err(map_file_err)?.len() as usize;

        let resize = |sz: usize| f.set_len(sz as u64).map(|_| sz).map_err(map_file_err);

        if self.truncate && flen > 0 {
            flen = resize(0)?;
        }

        flen = match self.resize {
            Extent::Exact(sz) => resize(sz)?,
            Extent::Min(sz) if sz > flen => resize(sz)?,
            Extent::Max(sz) if sz < flen => resize(sz)?,
            _ => flen,
        };

        if flen < off {
            return Ok(None);
        }

        let max = flen - off;
        let len = match self.len {
            Extent::Min(l) | Extent::Exact(l) if l > max => return Ok(None),
            Extent::Min(_) | Extent::End => max,
            Extent::Max(l) => cmp::min(l, max),
            Extent::Exact(l) => l,
        };

        let mapoff = Size::alloc().truncate(off);
        let maplen = len + (off - mapoff);
        let ptr = map_file(f, mapoff, maplen, self.protect)?;
        unsafe { Ok(Some(T::from_ptr(ptr.add(off - mapoff), len))) }
    }

    /// Creates an anonymous allocation using the options specified by `self`.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::{Extent, MapMut};
    ///
    /// # fn main() -> vmap::Result<()> {
    /// let map = MapMut::with_options().len(Extent::Min(500)).alloc()?;
    /// assert!(map.len() >= 500);
    /// # Ok(())
    /// # }
    /// ```
    pub fn alloc(&self) -> Result<T> {
        let off = Size::page().offset(self.offset);
        let len = match self.len {
            Extent::End => Size::alloc().round(off + 1) - off,
            Extent::Min(l) => Size::alloc().round(off + l) - off,
            Extent::Max(l) | Extent::Exact(l) => l,
        };

        let ptr = map_anon(off + len, self.protect)?;
        unsafe { Ok(T::from_ptr(ptr.add(off), len)) }
    }
}

impl<T: FromPtr> Default for Options<T> {
    fn default() -> Self {
        Self::new()
    }
}

fn map_file_err(e: io::Error) -> Error {
    Error::io(Operation::MapFile, e)
}
