use std::ops::{Deref, DerefMut};

mod sealed {
    pub trait Sealed {}

    impl Sealed for crate::Map {}
    impl Sealed for crate::MapMut {}
    impl<'a> Sealed for &'a [u8] {}
    impl<'a> Sealed for &'a mut [u8] {}
}

/// General trait for working with any memory-safe representation of a
/// contiguous region of arbitrary memory.
pub trait Span: Deref<Target = [u8]> + Sized + sealed::Sealed {
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
/// contiguous region of arbitrary memory with interior mutability.
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
