use std::convert::TryFrom;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::slice;

use crate::os::{advise, flush, lock, map_anon, map_file, protect, unlock, unmap};
use crate::{
    AdviseAccess, AdviseUsage, ConvertResult, Error, Flush, Input, Operation, Protect, Result,
    Size, Span, SpanMut,
};

/// Allocation of one or more read-only sequential pages.
///
/// # Examples
///
/// ```
/// # extern crate vmap;
/// use vmap::{Map, AdviseAccess, AdviseUsage};
/// use std::fs::OpenOptions;
///
/// # fn main() -> vmap::Result<()> {
/// let file = OpenOptions::new().read(true).open("README.md")?;
/// let page = Map::file(&file, 113, 30)?;
/// page.advise(AdviseAccess::Sequential, AdviseUsage::WillNeed)?;
/// assert_eq!(b"fast and safe memory-mapped IO", &page[..]);
/// assert_eq!(b"safe", &page[9..13]);
/// # Ok(())
/// # }
/// ```
pub struct Map(MapMut);

fn file_checked(f: &File, off: usize, len: usize, prot: Protect) -> Result<*mut u8> {
    match f.metadata() {
        Err(e) => Err(Error::io(Operation::MapFile, e)),
        Ok(ref md) => {
            if md.len() < off as u64 + len as u64 {
                Err(Error::input(Operation::MapFile, Input::InvalidRange))
            } else {
                unsafe { file_unchecked(f, off, len, prot) }
            }
        }
    }
}

fn file_max(
    f: &File,
    off: usize,
    mut maxlen: usize,
    prot: Protect,
) -> Result<Option<(*mut u8, usize)>> {
    match f.metadata() {
        Err(e) => Err(Error::io(Operation::MapFile, e)),
        Ok(ref md) if md.len() <= off as u64 => Ok(None),
        Ok(ref md) => {
            maxlen = std::cmp::min((md.len() - (off as u64)) as usize, maxlen);
            Ok(Some((
                unsafe { file_unchecked(f, off, maxlen, prot) }?,
                maxlen,
            )))
        }
    }
}

unsafe fn file_unchecked(f: &File, off: usize, len: usize, prot: Protect) -> Result<*mut u8> {
    let sz = Size::allocation();
    let roff = sz.truncate(off);
    let rlen = sz.round(len + (off - roff));
    let ptr = map_file(f, roff, rlen, prot)?;
    Ok(ptr.add(off - roff))
}

impl Map {
    /// Creates a new read-only map object using the full range of a file.
    ///
    /// The underlying file handle is open as read-only. If there is a need to
    /// convert the `Map` into a `MapMut`, use `Map::file` with a file handle
    /// open for writing. If not done, the convertion to `MapMut` will fail.
    ///
    /// # Examples
    /// ```
    /// # extern crate vmap;
    /// use std::fs::OpenOptions;
    /// use vmap::Map;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// let map = Map::open("README.md")?;
    /// assert_eq!(map.is_empty(), false);
    /// assert_eq!(b"fast and safe memory-mapped IO", &map[113..143]);
    ///
    /// // The file handle is read-only.
    /// assert!(map.into_map_mut().is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn open<P: AsRef<Path> + ?Sized>(path: &P) -> Result<Self> {
        match OpenOptions::new().read(true).open(path) {
            Err(e) => Err(Error::io(Operation::MapFile, e)),
            Ok(file) => match file.metadata() {
                Err(e) => Err(Error::io(Operation::MapFile, e)),
                Ok(data) => unsafe { Self::file_unchecked(&file, 0, data.len() as usize) },
            },
        }
    }

    /// Create a new map object from a range of a file.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate vmap;
    /// use std::fs::OpenOptions;
    /// use vmap::Map;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// let file = OpenOptions::new().read(true).open("README.md")?;
    /// let map = Map::file(&file, 0, 143)?;
    /// assert_eq!(map.is_empty(), false);
    /// assert_eq!(b"fast and safe memory-mapped IO", &map[113..143]);
    ///
    /// let map = Map::file(&file, 0, file.metadata()?.len() as usize + 1);
    /// assert!(map.is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn file(f: &File, offset: usize, length: usize) -> Result<Self> {
        let ptr = file_checked(f, offset, length, Protect::ReadOnly)?;
        Ok(unsafe { Self::from_ptr(ptr, length) })
    }

    /// Create a new map object from a maximum range of a file. Unlike `file`,
    /// the length is only a maximum size to map. If the length of the file
    /// is less than the requested range, the returned mapping will be
    /// shortened to match the file.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate vmap;
    /// use std::fs::OpenOptions;
    /// use vmap::Map;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// let file = OpenOptions::new().read(true).open("README.md")?;
    /// let map = Map::file_max(&file, 0, 5000)?.expect("should be valid range");
    /// assert_eq!(map.is_empty(), false);
    /// assert_eq!(b"fast and safe memory-mapped IO", &map[113..143]);
    ///
    /// let map = Map::file_max(&file, 0, file.metadata()?.len() as usize + 1);
    /// assert!(!map.is_err());
    ///
    /// let map = Map::file_max(&file, 5000, 100)?;
    /// assert!(map.is_none());
    /// # Ok(())
    /// # }
    /// ```
    pub fn file_max(f: &File, offset: usize, max_length: usize) -> Result<Option<Self>> {
        match file_max(f, offset, max_length, Protect::ReadOnly)? {
            Some((ptr, len)) => Ok(Some(unsafe { Self::from_ptr(ptr, len) })),
            None => Ok(None),
        }
    }

    /// Create a new map object from a range of a file without bounds checking.
    ///
    /// # Safety
    ///
    /// This does not verify that the requsted range is valid for the file.
    /// This can be useful in a few scenarios:
    /// 1. When the range is already known to be valid.
    /// 2. When a valid sub-range is known and not exceeded.
    /// 3. When the range will become valid and is not used until then.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate vmap;
    /// use std::fs::OpenOptions;
    /// use vmap::Map;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// let file = OpenOptions::new().read(true).open("README.md")?;
    /// let map = unsafe {
    ///     Map::file_unchecked(&file, 0, file.metadata()?.len() as usize + 1)?
    /// };
    /// // It is safe read the valid range of the file.
    /// assert_eq!(b"fast and safe memory-mapped IO", &map[113..143]);
    /// # Ok(())
    /// # }
    /// ```
    pub unsafe fn file_unchecked(f: &File, offset: usize, length: usize) -> Result<Self> {
        let ptr = file_unchecked(f, offset, length, Protect::ReadOnly)?;
        Ok(Self::from_ptr(ptr, length))
    }

    /// Constructs a new mutable map object from an existing mapped pointer.
    ///
    /// # Safety
    ///
    /// This does not know or care if `ptr` or `len` are valid. That is,
    /// it may be null, not at a proper page boundary, point to a size
    /// different from `len`, or worse yet, point to a properly mapped
    /// pointer from some other allocation system.
    ///
    /// Generally don't use this unless you are entirely sure you are
    /// doing so correctly.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate vmap;
    /// use vmap::{Map, Protect};
    /// use std::fs::OpenOptions;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// let file = OpenOptions::new().read(true).open("src/lib.rs")?;
    /// let page = unsafe {
    ///     let len = vmap::allocation_size();
    ///     let ptr = vmap::os::map_file(&file, 0, len, Protect::ReadOnly)?;
    ///     Map::from_ptr(ptr, len)
    /// };
    /// assert_eq!(b"fast and safe memory-mapped IO", &page[33..63]);
    /// # Ok(())
    /// # }
    /// ```
    pub unsafe fn from_ptr(ptr: *mut u8, len: usize) -> Self {
        Self(MapMut::from_ptr(ptr, len))
    }

    /// Transfer ownership of the map into a mutable map.
    ///
    /// This will change the protection of the mapping. If the original file
    /// was not opened with write permissions, this will error.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate vmap;
    /// # extern crate tempdir;
    /// use vmap::Map;
    /// use std::io::Write;
    /// use std::fs::OpenOptions;
    /// use std::path::PathBuf;
    /// # use std::fs;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("into_map_mut");
    /// # fs::write(&path, b"this is a test")?;
    /// let file = OpenOptions::new().read(true).write(true).open(&path)?;
    ///
    /// // Map the beginning of the file
    /// let map = Map::file(&file, 0, 14)?;
    /// assert_eq!(b"this is a test", &map[..]);
    ///
    /// let mut map = map.into_map_mut()?;
    /// {
    ///     let mut data = &mut map[..];
    ///     data.write_all(b"that")?;
    /// }
    /// assert_eq!(b"that is a test", &map[..]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn into_map_mut(self) -> ConvertResult<MapMut, Self> {
        unsafe {
            let (ptr, len) = Size::page().bounds(self.0.ptr, self.0.len);
            match protect(ptr, len, Protect::ReadWrite) {
                Ok(()) => Ok(self.0),
                Err(err) => Err((err, self)),
            }
        }
    }

    /// Transfer ownership of the map into a mutable map.
    ///
    /// This will change the protection of the mapping. If the original file
    /// was not opened with write permissions, this will error.
    ///
    /// This will cause the original map to be dropped if the protection change
    /// fails. Using `into_map_mut` allows the original map to be retained in the
    /// case of a failure.
    #[deprecated(since = "0.4", note = "use try_into or into_map_mut instead")]
    pub fn make_mut(self) -> Result<MapMut> {
        Ok(self.into_map_mut()?)
    }

    /// Updates the advise for the entire mapped region..
    pub fn advise(&self, access: AdviseAccess, usage: AdviseUsage) -> Result<()> {
        self.0.advise(access, usage)
    }

    /// Updates the advise for a specific range of the mapped region.
    pub fn advise_range(
        &self,
        off: usize,
        len: usize,
        access: AdviseAccess,
        usage: AdviseUsage,
    ) -> Result<()> {
        self.0.advise_range(off, len, access, usage)
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
    /// Create a new anonymous mapping at least as large as the hint.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate vmap;
    /// use vmap::{MapMut, Protect};
    /// use std::io::Write;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// let mut map = MapMut::new(200, Protect::ReadCopy)?;
    /// {
    ///     let mut data = &mut map[..];
    ///     assert!(data.len() >= 200);
    ///     data.write_all(b"test")?;
    /// }
    /// assert_eq!(b"test", &map[..4]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(hint: usize, prot: Protect) -> Result<Self> {
        unsafe {
            let len = Size::allocation().round(hint);
            let ptr = map_anon(len, prot)?;
            Ok(Self::from_ptr(ptr, len))
        }
    }

    /// Creates a new read/write map object using the full range of a file.
    ///
    /// # Examples
    /// ```
    /// # extern crate vmap;
    /// use std::fs::OpenOptions;
    /// use vmap::MapMut;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// let map = MapMut::open("README.md")?;
    /// assert_eq!(map.is_empty(), false);
    /// assert_eq!(b"fast and safe memory-mapped IO", &map[113..143]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn open<P: AsRef<Path> + ?Sized>(path: &P) -> Result<Self> {
        match OpenOptions::new().read(true).write(true).open(path) {
            Err(e) => Err(Error::io(Operation::MapFile, e)),
            Ok(file) => match file.metadata() {
                Err(e) => Err(Error::io(Operation::MapFile, e)),
                Ok(data) => unsafe { Self::file_unchecked(&file, 0, data.len() as usize) },
            },
        }
    }

    /// Create a new mutable map object from a range of a file.
    pub fn file(f: &File, offset: usize, length: usize) -> Result<Self> {
        let ptr = file_checked(f, offset, length, Protect::ReadWrite)?;
        Ok(unsafe { Self::from_ptr(ptr, length) })
    }

    /// Create a new mutable map object from a maximum range of a file. Unlike
    /// `file`, the length is only a maximum size to map. If the length of the
    /// file is less than the requested range, the returned mapping will be
    /// shortened to match the file.
    pub fn file_max(f: &File, offset: usize, max_length: usize) -> Result<Option<Self>> {
        match file_max(f, offset, max_length, Protect::ReadWrite)? {
            Some((ptr, len)) => Ok(Some(unsafe { Self::from_ptr(ptr, len) })),
            None => Ok(None),
        }
    }

    /// Create a new mutable map object from a range of a file without bounds
    /// checking.
    ///
    /// # Safety
    ///
    /// This does not verify that the requsted range is valid for the file.
    /// This can be useful in a few scenarios:
    /// 1. When the range is already known to be valid.
    /// 2. When a valid sub-range is known and not exceeded.
    /// 3. When the range will become valid and is not used until then.
    pub unsafe fn file_unchecked(f: &File, offset: usize, length: usize) -> Result<Self> {
        let ptr = file_unchecked(f, offset, length, Protect::ReadWrite)?;
        Ok(Self::from_ptr(ptr, length))
    }

    /// Create a new private map object from a range of a file.
    ///
    /// Initially, the mapping will be shared with other processes, but writes
    /// will be kept private.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate vmap;
    /// use vmap::MapMut;
    /// use std::io::Write;
    /// use std::fs::OpenOptions;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// let file = OpenOptions::new().read(true).open("src/lib.rs")?;
    /// let mut map = MapMut::copy(&file, 33, 30)?;
    /// assert_eq!(map.is_empty(), false);
    /// assert_eq!(b"fast and safe memory-mapped IO", &map[..]);
    /// {
    ///     let mut data = &mut map[..];
    ///     data.write_all(b"slow")?;
    /// }
    /// assert_eq!(b"slow and safe memory-mapped IO", &map[..]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn copy(f: &File, offset: usize, length: usize) -> Result<Self> {
        let ptr = file_checked(f, offset, length, Protect::ReadCopy)?;
        Ok(unsafe { Self::from_ptr(ptr, length) })
    }

    /// Create a new private map object from a range of a file.  Unlike
    /// `copy`, the length is only a maximum size to map. If the length of the
    /// file is less than the requested range, the returned mapping will be
    /// shortened to match the file.
    ///
    /// Initially, the mapping will be shared with other processes, but writes
    /// will be kept private.
    pub fn copy_max(f: &File, offset: usize, max_length: usize) -> Result<Option<Self>> {
        match file_max(f, offset, max_length, Protect::ReadCopy)? {
            Some((ptr, len)) => Ok(Some(unsafe { Self::from_ptr(ptr, len) })),
            None => Ok(None),
        }
    }

    /// Create a new private map object from a range of a file without bounds checking.
    ///
    /// Initially, the mapping will be shared with other processes, but writes
    /// will be kept private.
    ///
    /// # Safety
    ///
    /// This does not verify that the requsted range is valid for the file.
    /// This can be useful in a few scenarios:
    /// 1. When the range is already known to be valid.
    /// 2. When a valid sub-range is known and not exceeded.
    /// 3. When the range will become valid before any write occurs.
    pub unsafe fn copy_unchecked(f: &File, offset: usize, length: usize) -> Result<Self> {
        let ptr = file_unchecked(f, offset, length, Protect::ReadCopy)?;
        Ok(Self::from_ptr(ptr, length))
    }

    /// Constructs a new map object from an existing mapped pointer.
    ///
    /// # Safety
    ///
    /// This does not know or care if `ptr` or `len` are valid. That is,
    /// it may be null, not at a proper page boundary, point to a size
    /// different from `len`, or worse yet, point to a properly mapped
    /// pointer from some other allocation system.
    ///
    /// Generally don't use this unless you are entirely sure you are
    /// doing so correctly.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate vmap;
    /// # extern crate tempdir;
    /// use vmap::{MapMut, Protect};
    /// use std::fs::{self, OpenOptions};
    /// use std::path::PathBuf;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("into_map_mut");
    /// # fs::write(&path, b"this is a test")?;
    /// let file = OpenOptions::new().read(true).open("src/lib.rs")?;
    /// let page = unsafe {
    ///     let len = vmap::allocation_size();
    ///     let ptr = vmap::os::map_file(&file, 0, len, Protect::ReadOnly)?;
    ///     MapMut::from_ptr(ptr, len)
    /// };
    /// assert_eq!(b"fast and safe memory-mapped IO", &page[33..63]);
    /// # Ok(())
    /// # }
    /// ```
    pub unsafe fn from_ptr(ptr: *mut u8, len: usize) -> Self {
        Self { ptr, len }
    }

    /// Transfer ownership of the map into a mutable map.
    ///
    /// This will change the protection of the mapping. If the original file
    /// was not opened with write permissions, this will error.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate vmap;
    /// # extern crate tempdir;
    /// use vmap::MapMut;
    /// use std::io::Write;
    /// use std::fs::OpenOptions;
    /// use std::path::PathBuf;
    /// # use std::fs;
    ///
    /// # fn main() -> vmap::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("into_map_mut");
    /// # fs::write(&path, b"this is a test")?;
    /// let file = OpenOptions::new().read(true).write(true).open(&path)?;
    ///
    /// let mut map = MapMut::file(&file, 0, 14)?;
    /// assert_eq!(b"this is a test", &map[..]);
    /// {
    ///     let mut data = &mut map[..];
    ///     data.write_all(b"that")?;
    /// }
    ///
    /// let map = map.into_map()?;
    /// assert_eq!(b"that is a test", &map[..]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn into_map(self) -> ConvertResult<Map, Self> {
        unsafe {
            let (ptr, len) = Size::page().bounds(self.ptr, self.len);
            match protect(ptr, len, Protect::ReadWrite) {
                Ok(()) => Ok(Map(self)),
                Err(err) => Err((err, self)),
            }
        }
    }

    /// Transfer ownership of the map into a mutable map.
    ///
    /// This will change the protection of the mapping. If the original file
    /// was not opened with write permissions, this will error.
    ///
    /// This will cause the original map to be dropped if the protection change
    /// fails. Using `into_map` allows the original map to be retained in the
    /// case of a failure.
    #[deprecated(since = "0.4", note = "use try_into or into_map instead")]
    pub fn make_read_only(self) -> Result<Map> {
        Ok(self.into_map()?)
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

    /// Updates the advise for the entire mapped region..
    pub fn advise(&self, access: AdviseAccess, usage: AdviseUsage) -> Result<()> {
        unsafe {
            let (ptr, len) = Size::page().bounds(self.ptr, self.len);
            advise(ptr, len, access, usage)
        }
    }

    /// Updates the advise for a specific range of the mapped region.
    pub fn advise_range(
        &self,
        off: usize,
        len: usize,
        access: AdviseAccess,
        usage: AdviseUsage,
    ) -> Result<()> {
        if off + len > self.len {
            Err(Error::input(Operation::Advise, Input::InvalidRange))
        } else {
            unsafe {
                let (ptr, len) = Size::page().bounds(self.ptr.add(off), len);
                advise(ptr, len, access, usage)
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
                let (ptr, len) = Size::allocation().bounds(self.ptr, self.len);
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
