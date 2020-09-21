use std::marker::PhantomData;
use std::mem::{align_of, size_of};
use std::ops::{Deref, DerefMut};

pub use zerocopy::{AsBytes, FromBytes};

use super::{Span, SpanMut};

/// AsType
///
/// Conceptually, this is very similar to the `zerocopy::LayoutVerified`, but
/// with some key differences:
///
/// 1. An `AsType` only requies a `Span` and only optionally requires a `Slice`.
///    The `LayoutVerified` requires a `ByteSlice` which requires the capability
///    of a `Slice`. For normal byte ranges, this difference is quite trivial,
///    but with a `Map` or `MapMut`, the requirement to split demands a type
///    that can properly manage references (i.e. `RefSlice` or `ArcSlice`) and
///    it forces an extra pointer indirection. Because `Map`s and `MapMut`s are
///    `Span`, the map can be owned directly by the `AsType`.
/// 2. The size requirement of the `Span` or `Slice` is a little more flexible.
///    When using a `LayoutVerified`, the `ByteSlice` must either be exactly the
///    size required by the verified type, or it must be split. This is, again,
///    fairly trivial restriction for a standard byte slice. With a `Map`, however,
///    the underlying allocation granularity is different per platform, and this
///    can make matching the `size_of::<T>()` to the `Map` tricky if not somewhat
///    uncessary. This can be addressed when mapping a range of a file or by using
///    a `Slice` to get an exact size, but overall this is an unnecessary restriction.
///    As long as the `Span` is at least large enough for the type, it is safe to use
///    the entire allocation.
pub struct AsType<T: ?Sized, S>(S, PhantomData<T>);

#[inline]
fn is_sized_for<T>(len: usize) -> bool {
    len >= size_of::<T>()
}

#[inline]
fn is_aligned_for<T>(ptr: *const u8) -> bool {
    (ptr as *const _ as *const () as usize) % align_of::<T>() == 0
}

impl<T, S> AsType<T, S>
where
    T: FromBytes,
    S: Span,
{
    /// TODO
    #[inline]
    pub fn new(span: S) -> Result<Self, S> {
        if is_sized_for::<T>(span.len()) && is_aligned_for::<T>(span.as_ptr()) {
            Ok(Self(span, PhantomData))
        } else {
            Err(span)
        }
    }

    /// TODO
    #[inline]
    pub fn unwrap(self) -> S {
        self.0
    }

    /// TODO
    #[inline]
    pub fn type_bytes(&self) -> &[u8] {
        &self.0[..size_of::<T>()]
    }

    /// TODO
    #[inline]
    pub fn tail_bytes(&self) -> &[u8] {
        &self.0[size_of::<T>()..]
    }

    /// TODO
    #[inline]
    pub fn tail_type<TT>(&self) -> Option<AsType<TT, &[u8]>>
    where
        TT: FromBytes,
    {
        AsType::new(self.tail_bytes()).ok()
    }
}

impl<T, S> AsType<T, S>
where
    T: FromBytes + AsBytes,
    S: SpanMut,
{
    /// TODO
    #[inline]
    pub fn tail_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.0[size_of::<T>()..]
    }

    /// TODO
    #[inline]
    pub fn tail_type_mut<TT>(&mut self) -> Option<AsType<TT, &mut [u8]>>
    where
        TT: FromBytes + AsBytes,
    {
        AsType::new(self.tail_bytes_mut()).ok()
    }
}

impl<T, S> Deref for AsType<T, S>
where
    T: FromBytes,
    S: Span,
{
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        unsafe { &*(self.0.as_ptr() as *const T) }
    }
}

impl<T, S> DerefMut for AsType<T, S>
where
    T: FromBytes + AsBytes,
    S: SpanMut,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *(self.0.as_mut_ptr() as *mut T) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MapMut, Protect};

    #[derive(AsBytes, FromBytes, Debug, PartialEq)]
    #[repr(C)]
    struct Thing {
        a: u32,
        b: u32,
    }

    #[test]
    fn test_span() {
        let map = MapMut::new(100, Protect::ReadWrite).expect("failed to create map");
        let mut thing = AsType::new(map).expect("failed to cast type");
        assert_eq!(Thing { a: 0, b: 0 }, *thing);
        thing.a = 0b01010101010101010101010101010101;
        thing.b = 0b10101010101010101010101010101010;
        assert_eq!(
            Thing {
                a: 0b01010101010101010101010101010101,
                b: 0b10101010101010101010101010101010,
            },
            *thing
        );

        let mut thing2 = thing.tail_type_mut().expect("failed to cast type");
        assert_eq!(Thing { a: 0, b: 0 }, *thing2);
        thing2.a = 0b00110011001100110011001100110011;
        thing2.b = 0b11001100110011001100110011001100;
        assert_eq!(
            Thing {
                a: 0b00110011001100110011001100110011,
                b: 0b11001100110011001100110011001100,
            },
            *thing2
        );

        assert_eq!(
            Thing {
                a: 0b01010101010101010101010101010101,
                b: 0b10101010101010101010101010101010,
            },
            *thing
        );
    }
}
