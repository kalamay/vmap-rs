use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Error, ErrorKind, Result};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::slice;

use crate::os::{advise, flush, lock, map_anon, map_file, protect, unlock, unmap};
use crate::{AdviseAccess, AdviseUsage, AllocSize, Flush, Protect};

/// General trait for working with any mapped value.
pub trait Mapped {
    /// Get the length of the allocated region.
    fn len(&self) -> usize;

    /// Get the pointer to the start of the allocated region.
    fn as_ptr(&self) -> *const u8;

    /// Tests if the mapped pointer has the correct alignment.
    fn is_aligned_to(&self, alignment: usize) -> bool {
        (self.as_ptr() as *const _ as *const () as usize) % alignment == 0
    }
}

/// General trait for working with any mutably mapped value.
pub trait MappedMut: Mapped {
    /// Get a mutable pointer to the start of the allocated region.
    fn as_mut_ptr(&self) -> *mut u8;
}

/// Allocation of one or more read-only sequential pages.
///
/// # Example
///
/// ```
/// # extern crate vmap;
/// use vmap::{Map, AdviseAccess, AdviseUsage};
/// use std::fs::OpenOptions;
///
/// # fn main() -> std::io::Result<()> {
/// let file = OpenOptions::new().read(true).open("README.md")?;
/// let page = Map::file(&file, 113, 30)?;
/// page.advise(AdviseAccess::Sequential, AdviseUsage::WillNeed)?;
/// assert_eq!(b"fast and safe memory-mapped IO", &page[..]);
/// assert_eq!(b"safe", &page[9..13]);
/// # Ok(())
/// # }
/// ```
pub struct Map {
    base: MapMut,
}

fn file_checked(f: &File, off: usize, len: usize, prot: Protect) -> Result<*mut u8> {
    if f.metadata()?.len() < off as u64 + len as u64 {
        Err(Error::new(ErrorKind::InvalidInput, "map range not in file"))
    } else {
        unsafe { file_unchecked(f, off, len, prot) }
    }
}

fn file_max(
    f: &File,
    off: usize,
    mut maxlen: usize,
    prot: Protect,
) -> Result<Option<(*mut u8, usize)>> {
    let len = f.metadata()?.len();
    if len <= off as u64 {
        Ok(None)
    } else {
        maxlen = std::cmp::min((len - (off as u64)) as usize, maxlen);
        Ok(Some((
            unsafe { file_unchecked(f, off, maxlen, prot) }?,
            maxlen,
        )))
    }
}

unsafe fn file_unchecked(f: &File, off: usize, len: usize, prot: Protect) -> Result<*mut u8> {
    let sz = AllocSize::new();
    let roff = sz.truncate(off);
    let rlen = sz.round(len + (off - roff));
    let ptr = map_file(f, roff, rlen, prot)?;
    Ok(ptr.offset((off - roff) as isize))
}

impl Map {
    /// Creates a new read-only map object using the full range of a file.
    ///
    /// # Example
    /// ```
    /// # extern crate vmap;
    /// use std::fs::OpenOptions;
    /// use vmap::Map;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let map = Map::open("README.md")?;
    /// assert_eq!(map.is_empty(), false);
    /// assert_eq!(b"fast and safe memory-mapped IO", &map[113..143]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn open<P: AsRef<Path> + ?Sized>(path: &P) -> Result<Self> {
        let file = OpenOptions::new().read(true).open(path)?;
        let size = file.metadata()?.len();
        unsafe { Self::file_unchecked(&file, 0, size as usize) }
    }

    /// Create a new map object from a range of a file.
    ///
    /// # Example
    ///
    /// ```
    /// # extern crate vmap;
    /// use std::fs::OpenOptions;
    /// use vmap::Map;
    ///
    /// # fn main() -> std::io::Result<()> {
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
    /// # Example
    ///
    /// ```
    /// # extern crate vmap;
    /// use std::fs::OpenOptions;
    /// use vmap::Map;
    ///
    /// # fn main() -> std::io::Result<()> {
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
    /// # Example
    ///
    /// ```
    /// # extern crate vmap;
    /// use std::fs::OpenOptions;
    /// use vmap::Map;
    ///
    /// # fn main() -> std::io::Result<()> {
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
    /// # Example
    ///
    /// ```
    /// # extern crate vmap;
    /// use vmap::{Map, Protect};
    /// use std::fs::OpenOptions;
    ///
    /// # fn main() -> std::io::Result<()> {
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
        Self {
            base: MapMut::from_ptr(ptr, len),
        }
    }

    /// Transfer ownership of the map into a mutable map.
    ///
    /// This will change the protection of the mapping. If the original file
    /// was not opened with write permissions, this will error.
    ///
    /// # Example
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
    /// # fn main() -> std::io::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("make_mut");
    /// # fs::write(&path, b"this is a test")?;
    /// let file = OpenOptions::new().read(true).write(true).open(&path)?;
    ///
    /// // Map the beginning of the file
    /// let map = Map::file(&file, 0, 14)?;
    /// assert_eq!(b"this is a test", &map[..]);
    ///
    /// let mut map = map.make_mut()?;
    /// {
    ///     let mut data = &mut map[..];
    ///     data.write_all(b"that")?;
    /// }
    /// assert_eq!(b"that is a test", &map[..]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn make_mut(self) -> Result<MapMut> {
        unsafe {
            let (ptr, len) = AllocSize::new().bounds(self.base.ptr, self.base.len);
            protect(ptr, len, Protect::ReadWrite)?;
        }
        Ok(self.base)
    }

    /// Updates the advise for the entire mapped region..
    pub fn advise(&self, access: AdviseAccess, usage: AdviseUsage) -> Result<()> {
        self.base.advise(access, usage)
    }

    /// Updates the advise for a specific range of the mapped region.
    pub fn advise_range(
        &self,
        off: usize,
        len: usize,
        access: AdviseAccess,
        usage: AdviseUsage,
    ) -> Result<()> {
        self.base.advise_range(off, len, access, usage)
    }

    /// Lock all mapped physical pages into memory.
    pub fn lock(&self) -> Result<()> {
        self.base.lock()
    }

    /// Lock a range of physical pages into memory.
    pub fn lock_range(&self, off: usize, len: usize) -> Result<()> {
        self.base.lock_range(off, len)
    }

    /// Unlock all mapped physical pages into memory.
    pub fn unlock(&self) -> Result<()> {
        self.base.unlock()
    }

    /// Unlock a range of physical pages into memory.
    pub fn unlock_range(&self, off: usize, len: usize) -> Result<()> {
        self.base.unlock_range(off, len)
    }
}

impl Mapped for Map {
    #[inline]
    fn len(&self) -> usize {
        self.base.len()
    }

    #[inline]
    fn as_ptr(&self) -> *const u8 {
        self.base.as_ptr()
    }
}

impl Deref for Map {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        self.base.deref()
    }
}

impl AsRef<[u8]> for Map {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

impl fmt::Debug for Map {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Map")
            .field("ptr", &self.base.ptr)
            .field("len", &self.base.len)
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
    /// # Example
    ///
    /// ```
    /// # extern crate vmap;
    /// use vmap::{MapMut, Protect};
    /// use std::io::Write;
    ///
    /// # fn main() -> std::io::Result<()> {
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
            let len = AllocSize::new().round(hint);
            let ptr = map_anon(len, prot)?;
            Ok(Self::from_ptr(ptr, len))
        }
    }

    /// Creates a new read/write map object using the full range of a file.
    ///
    /// # Example
    /// ```
    /// # extern crate vmap;
    /// use std::fs::OpenOptions;
    /// use vmap::MapMut;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let map = MapMut::open("README.md")?;
    /// assert_eq!(map.is_empty(), false);
    /// assert_eq!(b"fast and safe memory-mapped IO", &map[113..143]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn open<P: AsRef<Path> + ?Sized>(path: &P) -> Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let size = file.metadata()?.len();
        unsafe { Self::file_unchecked(&file, 0, size as usize) }
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
    /// # Example
    ///
    /// ```
    /// # extern crate vmap;
    /// use vmap::MapMut;
    /// use std::io::Write;
    /// use std::fs::OpenOptions;
    ///
    /// # fn main() -> std::io::Result<()> {
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
    /// # Example
    ///
    /// ```
    /// # extern crate vmap;
    /// # extern crate tempdir;
    /// use vmap::{MapMut, Protect};
    /// use std::fs::{self, OpenOptions};
    /// use std::path::PathBuf;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("make_mut");
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
        Self { ptr: ptr, len: len }
    }

    /// Transfer ownership of the map into a mutable map.
    ///
    /// This will change the protection of the mapping. If the original file
    /// was not opened with write permissions, this will error.
    ///
    /// # Example
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
    /// # fn main() -> std::io::Result<()> {
    /// # let tmp = tempdir::TempDir::new("vmap")?;
    /// let path: PathBuf = /* path to file */
    /// # tmp.path().join("make_mut");
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
    /// let map = map.make_read_only()?;
    /// assert_eq!(b"that is a test", &map[..]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn make_read_only(self) -> Result<Map> {
        unsafe {
            let (ptr, len) = AllocSize::new().bounds(self.ptr, self.len);
            protect(ptr, len, Protect::ReadWrite)?;
        }
        Ok(Map { base: self })
    }

    /// Writes modifications back to the filesystem.
    ///
    /// Flushes will happen automatically, but this will invoke a flush and
    /// return any errors with doing so.
    pub fn flush(&self, file: &File, mode: Flush) -> Result<()> {
        unsafe {
            let (ptr, len) = AllocSize::new().bounds(self.ptr, self.len);
            flush(ptr, file, len, mode)
        }
    }

    /// Updates the advise for the entire mapped region..
    pub fn advise(&self, access: AdviseAccess, usage: AdviseUsage) -> Result<()> {
        unsafe {
            let (ptr, len) = AllocSize::new().bounds(self.ptr, self.len);
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
            return Err(Error::new(ErrorKind::InvalidInput, "range not in map"));
        }
        unsafe {
            let (ptr, len) = AllocSize::new().bounds(self.ptr.offset(off as isize), len);
            advise(ptr, len, access, usage)
        }
    }

    /// Lock all mapped physical pages into memory.
    pub fn lock(&self) -> Result<()> {
        unsafe {
            let (ptr, len) = AllocSize::new().bounds(self.ptr, self.len);
            lock(ptr, len)
        }
    }

    /// Lock a range of physical pages into memory.
    pub fn lock_range(&self, off: usize, len: usize) -> Result<()> {
        if off + len > self.len {
            return Err(Error::new(ErrorKind::InvalidInput, "range not in map"));
        }
        unsafe {
            let (ptr, len) = AllocSize::new().bounds(self.ptr.offset(off as isize), len);
            lock(ptr, len)
        }
    }

    /// Unlock all mapped physical pages into memory.
    pub fn unlock(&self) -> Result<()> {
        unsafe {
            let (ptr, len) = AllocSize::new().bounds(self.ptr, self.len);
            unlock(ptr, len)
        }
    }

    /// Unlock a range of physical pages into memory.
    pub fn unlock_range(&self, off: usize, len: usize) -> Result<()> {
        if off + len > self.len {
            return Err(Error::new(ErrorKind::InvalidInput, "range not in map"));
        }
        unsafe {
            let (ptr, len) = AllocSize::new().bounds(self.ptr.offset(off as isize), len);
            unlock(ptr, len)
        }
    }
}

impl Mapped for MapMut {
    #[inline]
    fn len(&self) -> usize {
        self.len
    }

    #[inline]
    fn as_ptr(&self) -> *const u8 {
        self.ptr
    }
}

impl MappedMut for MapMut {
    #[inline]
    fn as_mut_ptr(&self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for MapMut {
    fn drop(&mut self) {
        unsafe {
            if self.len > 0 {
                let (ptr, len) = AllocSize::new().bounds(self.ptr, self.len);
                unmap(ptr, len).unwrap_or_default();
            }
        }
    }
}

impl Deref for MapMut {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.as_ptr(), self.len()) }
    }
}

impl DerefMut for MapMut {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.as_mut_ptr(), self.len()) }
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
