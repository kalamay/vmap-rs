use std::fmt;
use std::ops::{Deref, DerefMut};
use std::slice;
use std::sync::Arc;

use super::{Mapped, MappedMut};

/// Byte range used by a slice.
pub type ByteRange = std::ops::Range<usize>;

/// Splits a byte range at a mid point offset from the start of the range.
/// That is, a mid point of 0 splits the range at the current start position.
fn split_range(r: ByteRange, mid: usize) -> (ByteRange, ByteRange) {
    let s = r.start;
    let e = r.end;
    match mid {
        0 => (ByteRange { start: s, end: s }, r),
        m if m >= e - s => (r, ByteRange { start: e, end: e }),
        m => (
            ByteRange {
                start: s,
                end: s + m,
            },
            ByteRange {
                start: s + m,
                end: e,
            },
        ),
    }
}

fn trim_range(r: ByteRange, len: usize) -> ByteRange {
    if r.start > len {
        ByteRange {
            start: len,
            end: len,
        }
    } else if r.end > len {
        ByteRange {
            start: r.start,
            end: len,
        }
    } else {
        r
    }
}

/// Slice over a map reference.
///
/// # Example
/// ```
/// # extern crate vmap;
/// use std::fs::OpenOptions;
/// use vmap::{Map, Slice};
///
/// # fn main() -> std::io::Result<()> {
/// let map = Map::open("README.md")?;
/// let slice = Slice::new(&map, 113..143);
/// assert_eq!(slice.is_empty(), false);
/// assert_eq!(b"fast and safe memory-mapped IO", &slice[..]);
/// let (left, right) = slice.split_at(9);
/// assert_eq!(left.is_empty(), false);
/// assert_eq!(b"fast and ", &left[..]);
/// assert_eq!(right.is_empty(), false);
/// assert_eq!(b"safe memory-mapped IO", &right[..]);
/// # Ok(())
/// # }
/// ```
pub struct Slice<'a, M> {
    map: &'a M,
    rng: ByteRange,
}

impl<'a, M> Slice<'a, M>
where
    M: Mapped,
{
    /// Create a new slice from a range of a map.
    pub fn new(map: &'a M, rng: ByteRange) -> Self {
        let len = map.len();
        Self {
            map: map,
            rng: trim_range(rng, len),
        }
    }

    /// Splits a slice into two new slices and consuming self.
    pub fn split_at(self, mid: usize) -> (Self, Self) {
        let (l, r) = split_range(self.rng, mid);
        (
            Self {
                map: self.map,
                rng: l,
            },
            Self {
                map: self.map,
                rng: r,
            },
        )
    }
}

impl<M> Mapped for Slice<'_, M>
where
    M: Mapped,
{
    fn len(&self) -> usize {
        self.rng.len()
    }

    fn as_ptr(&self) -> *const u8 {
        unsafe { self.map.as_ptr().offset(self.rng.start as isize) }
    }
}

impl<M> MappedMut for Slice<'_, M>
where
    M: MappedMut,
{
    fn as_mut_ptr(&self) -> *mut u8 {
        unsafe { self.map.as_mut_ptr().offset(self.rng.start as isize) }
    }
}

impl<M> Deref for Slice<'_, M>
where
    M: Mapped,
{
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.as_ptr(), self.len()) }
    }
}

impl<M> DerefMut for Slice<'_, M>
where
    M: MappedMut,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.as_mut_ptr(), self.len()) }
    }
}

impl<M> AsRef<[u8]> for Slice<'_, M>
where
    M: Mapped,
{
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

impl<M> AsMut<[u8]> for Slice<'_, M>
where
    M: MappedMut,
{
    #[inline]
    fn as_mut(&mut self) -> &mut [u8] {
        self.deref_mut()
    }
}

impl<'a, M> From<&'a M> for Slice<'a, M>
where
    M: Mapped,
{
    fn from(map: &'a M) -> Self {
        let end = map.len();
        Self {
            map: map,
            rng: ByteRange { start: 0, end: end },
        }
    }
}

impl<M> fmt::Debug for Slice<'_, M>
where
    M: Mapped,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.as_ref())
    }
}

//--------------------------------------------------------------------------

/// Slice over a reference counted map.
///
/// # Example
/// ```
/// # extern crate vmap;
/// use std::fs::OpenOptions;
/// use vmap::{Map, ArcSlice};
///
/// # fn main() -> std::io::Result<()> {
/// let slice = {
///     let map = Map::open("README.md")?;
///     ArcSlice::new(map, 113..143)
/// };
/// assert_eq!(slice.is_empty(), false);
/// assert_eq!(b"fast and safe memory-mapped IO", &slice[..]);
/// let (left, right) = slice.split_at(9);
/// assert_eq!(left.is_empty(), false);
/// assert_eq!(b"fast and ", &left[..]);
/// assert_eq!(right.is_empty(), false);
/// assert_eq!(b"safe memory-mapped IO", &right[..]);
/// # Ok(())
/// # }
/// ```
pub struct ArcSlice<M> {
    map: Arc<M>,
    rng: ByteRange,
}

impl<M> ArcSlice<M>
where
    M: Mapped,
{
    /// Create a new slice from a range of a map.
    pub fn new(map: M, rng: ByteRange) -> Self {
        let len = map.len();
        Self {
            map: Arc::new(map),
            rng: trim_range(rng, len),
        }
    }

    /// Splits a slice into two new slices and consuming self.
    pub fn split_at(self, mid: usize) -> (Self, Self) {
        let (l, r) = split_range(self.rng, mid);
        (
            Self {
                map: Arc::clone(&self.map),
                rng: l,
            },
            Self {
                map: self.map,
                rng: r,
            },
        )
    }
}

impl<M> Mapped for ArcSlice<M>
where
    M: Mapped,
{
    fn len(&self) -> usize {
        self.rng.len()
    }

    fn as_ptr(&self) -> *const u8 {
        unsafe { self.map.as_ptr().offset(self.rng.start as isize) }
    }
}

impl<M> MappedMut for ArcSlice<M>
where
    M: MappedMut,
{
    fn as_mut_ptr(&self) -> *mut u8 {
        unsafe { self.map.as_mut_ptr().offset(self.rng.start as isize) }
    }
}

impl<M> Deref for ArcSlice<M>
where
    M: Mapped,
{
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.as_ptr(), self.len()) }
    }
}

impl<M> DerefMut for ArcSlice<M>
where
    M: MappedMut,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.as_mut_ptr(), self.len()) }
    }
}

impl<M> AsRef<[u8]> for ArcSlice<M>
where
    M: Mapped,
{
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

impl<M> AsMut<[u8]> for ArcSlice<M>
where
    M: MappedMut,
{
    #[inline]
    fn as_mut(&mut self) -> &mut [u8] {
        self.deref_mut()
    }
}

impl<M> From<M> for ArcSlice<M>
where
    M: Mapped,
{
    fn from(map: M) -> Self {
        let end = map.len();
        Self {
            map: Arc::new(map),
            rng: ByteRange { start: 0, end: end },
        }
    }
}

impl<M> From<&Arc<M>> for ArcSlice<M>
where
    M: Mapped,
{
    fn from(map: &Arc<M>) -> Self {
        let end = map.len();
        Self {
            map: Arc::clone(map),
            rng: ByteRange { start: 0, end: end },
        }
    }
}

impl<M> From<ArcSlice<M>> for Arc<M>
where
    M: Mapped,
{
    fn from(slice: ArcSlice<M>) -> Arc<M> {
        slice.map
    }
}

impl<M> fmt::Debug for ArcSlice<M>
where
    M: Mapped,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{MapMut, Protect};

    #[test]
    fn test_split_range() {
        let r = ByteRange { start: 4, end: 10 };
        assert_eq!(r, 4..10);
        assert_eq!(split_range(r.clone(), 0), (4..4, 4..10));
        assert_eq!(split_range(r.clone(), 1), (4..5, 5..10));
        assert_eq!(split_range(r.clone(), 5), (4..9, 9..10));
        assert_eq!(split_range(r.clone(), 6), (4..10, 10..10));
        assert_eq!(split_range(r.clone(), 7), (4..10, 10..10));
    }

    #[test]
    fn test_slice() {
        let map = MapMut::new(100, Protect::ReadWrite).expect("failed to create map");
        let len = map.len();

        let mut slice = Slice::from(&map);
        slice[0] = 88;
        slice[10] = 99;

        let (l, r) = slice.split_at(6);
        assert_eq!(6, l.len());
        assert_eq!(len - 6, r.len());
        assert_eq!(88, l[0]);
        assert_eq!(99, r[4]);

        let (l, r) = r.split_at(4);
        assert_eq!(4, l.len());
        assert_eq!(len - 10, r.len());
        assert_eq!(99, r[0]);
    }

    #[test]
    fn test_arc_slice() {
        let (len, slice) = {
            let map = MapMut::new(100, Protect::ReadWrite).expect("failed to create map");
            let len = map.len();

            let mut slice = ArcSlice::from(map);
            slice[0] = 88;
            slice[10] = 99;

            (len, slice)
        };

        let (l, r) = slice.split_at(6);
        assert_eq!(6, l.len());
        assert_eq!(len - 6, r.len());
        assert_eq!(88, l[0]);
        assert_eq!(99, r[4]);

        let (l, r) = r.split_at(4);
        assert_eq!(4, l.len());
        assert_eq!(len - 10, r.len());
        assert_eq!(99, r[0]);
    }
}
