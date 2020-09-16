use std::slice;

/// General trait for working with any memory-safe representation of a
/// contiguous region of arbitrary memory.
pub trait Span {
    /// Get the length of the allocated region.
    fn len(&self) -> usize;

    /// Get the pointer to the start of the allocated region.
    fn as_ptr(&self) -> *const u8;

    /// Get a reference to the memory span as a native slice.
    #[inline]
    fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.as_ptr(), self.len()) }
    }

    /// Tests if the span covers zero bytes.
    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Tests if the mapped pointer has the correct alignment.
    #[inline]
    fn is_aligned_to(&self, alignment: usize) -> bool {
        (self.as_ptr() as *const _ as *const () as usize) % alignment == 0
    }
}

/// General trait for working with any memory-safe representation of a
/// contiguous region of arbitrary memory with interior mutability.
pub trait SpanMut: Span {
    /// Get a mutable pointer to the start of the allocated region.
    fn as_mut_ptr(&self) -> *mut u8;

    /// Get a mutable reference to the memory span as a native slice.
    #[inline]
    fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.as_mut_ptr(), self.len()) }
    }
}
