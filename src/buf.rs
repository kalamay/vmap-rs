//! Read/Write types for buffering.
//!
//! Both the [`Buffer`](struct.Buffer.html) and
//! [`RingBuffer`](struct.RingBuffer.html) are fixed size anonymous allocations
//! utilizing circular address mappinng. The circular mapping ensures that
//! the entire readable or writable slice may always be addressed as a single,
//! contiguous allocation. However, these two types differ in one key way:
//! the [`Buffer`](struct.Buffer.html) may only written to as readable space
//! is consumed, whereas the [`RingBuffer`](struct.RingBuffer.html) is always
//! writable and will overwrite unconsumed space as needed.

use ::AllocSize;
use ::os::{map_ring, unmap_ring};

use std;
use std::slice;
use std::io::{Result, BufRead, Read, Write};
use std::cmp;



/// Common input trait for all buffers.
pub trait SeqRead: BufRead {
    /// Get the mapped readable pointer without any offset.
    fn as_read_ptr(&self) -> *const u8;

    /// Get the offset from the read pointer for the current read position.
    fn read_offset(&self) -> usize;

    /// Get the total number of readable bytes after the read offset.
    fn read_len(&self) -> usize;

    /// Test if all read bytes have been consumed.
    #[inline]
    fn is_empty(&self) -> bool { self.read_len() == 0 }

    /// Get an immutable slice covering the read region of the buffer.
    #[inline]
    fn as_read_slice(&self, max: usize) -> &[u8] {
        unsafe {
            slice::from_raw_parts(
                self.as_read_ptr().offset(self.read_offset() as isize),
                cmp::min(self.read_len(), max))
        }
    }

    /// Perform a read and consume from the read slice.
    fn read_from(&mut self, into: &mut [u8]) -> Result<usize> {
        let len = {
            let src = self.as_read_slice(into.len());
            let len = src.len();
            into[..len].copy_from_slice(src);
            len
        };
        self.consume(len);
        Ok(len)
    }
}



/// Common output trait for all buffers.
pub trait SeqWrite {
    /// Get the mapped writable pointer without any offset.
    fn as_write_ptr(&mut self) -> *mut u8;

    /// Get the offset from the write pointer for the current read position.
    fn write_offset(&self) -> usize;

    /// Get the total number of bytes that may be written after the current write offset.
    fn write_len(&self) -> usize;

    /// Bump the write offset after writing into the writable slice.
    ///
    /// This is a low-level call intended to be used for common write behavior.
    /// While this is safe to call improperly (without having written), it would
    /// result in stale information in the buffer.
    fn feed(&mut self, len: usize);

    /// Test if there is no room for furthur writes.
    #[inline]
    fn is_full(&self) -> bool { self.write_len() == 0 }

    /// Get a mutable slice covering the write region of the buffer.
    #[inline]
    fn as_write_slice(&mut self, max: usize) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(
                self.as_write_ptr().offset(self.write_offset() as isize),
                cmp::min(self.write_len(), max))
        }
    }

    /// Perform a write and feed into the write slice.
    fn write_into(&mut self, from: &[u8]) -> Result<usize> {
        let len = {
            let dst = self.as_write_slice(from.len());
            let len = dst.len();
            dst.copy_from_slice(&from[..len]);
            len
        };
        self.feed(len);
        Ok(len)
    }
}



/*
pub struct BufReader<T: SeqRead, R: Read> {
    buf: T,
    src: R,
}

pub struct BufWriter<T: SeqRead, W: Write> {
    buf: T,
    dst: W,
}
*/



/// Fixed-size reliable read/write buffer with sequential address mapping.
///
/// This uses a circular address mapping scheme. That is, for any buffer of
/// size `N`, the pointer address range of `0..N` maps to the same physical
/// memory as the range `N..2*N`. This guarantees that the entire read or
/// write range may be addressed as a single sequence of bytes.
///
/// Unlike the [`RingBuffer`](struct.RingBuffer.html), this type otherise
/// acts as a "normal" buffer. Writes fill up the buffer, and when full, no
/// furthur writes may be performed until a read occurs. The writable length
/// sequence is the capacity of the buffer, less any pending readable bytes.
///
/// # Example
///
/// ```
/// # extern crate vmap;
/// #
/// use vmap::buf::{Buffer, SeqWrite};
/// use std::io::{BufRead, Read, Write};
///
/// # fn main() -> std::io::Result<()> {
/// let mut buf = Buffer::new(4000)?;
/// let mut i = 1;
/// while buf.write_len() > 20 {
///     write!(&mut buf, "this is test line {}\n", i)?;
///     i += 1;
/// }
/// assert!(write!(&mut buf, "this is test line {}\n", i).is_err());
///
/// let mut line = String::new();
/// let len = buf.read_line(&mut line)?;
/// assert_eq!(len, 20);
/// assert_eq!(line, "this is test line 1\n");
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Buffer {
    ptr: *mut u8,
    len: usize,
    rpos: u64,
    wpos: u64,
}

impl Buffer {
    /// Constructs a new buffer instance.
    ///
    /// The hint is a minimum size for the buffer. This size will be rounded up
    /// to the nearest page size for the actual capacity. The allocation will
    /// occupy double the space in the virtual memory table, but the physical
    /// memory usage will remain at the desired capacity.
    pub fn new(hint: usize) -> Result<Self> {
        let len = AllocSize::new().round(hint);
        unsafe {
            let ptr = map_ring(len)?;
            Ok(Self { ptr: ptr, len: len, rpos: 0, wpos: 0 })
        }
    }

    /// Gets the number of bytes that the buffer has currently allocated space for.
    pub fn capacity(&self) -> usize { self.len }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe { unmap_ring(self.ptr, self.len) }.unwrap_or_default();
    }
}

impl SeqRead for Buffer {
    fn as_read_ptr(&self) -> *const u8 { self.ptr }
    fn read_offset(&self) -> usize { (self.rpos % self.len as u64) as usize }
    fn read_len(&self) -> usize { (self.wpos - self.rpos) as usize }
}

impl SeqWrite for Buffer {
    fn as_write_ptr(&mut self) -> *mut u8 { self.ptr }
    fn write_offset(&self) -> usize { (self.wpos % self.len as u64) as usize }
    fn write_len(&self) -> usize { self.len - self.read_len() }
    fn feed(&mut self, len: usize) {
        self.wpos += cmp::min(len, self.write_len()) as u64;
    }
}

impl BufRead for Buffer {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        Ok(self.as_read_slice(std::usize::MAX))
    }

    fn consume(&mut self, len: usize) {
        self.rpos += cmp::min(len, self.read_len()) as u64;
    }
}

impl Read for Buffer {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.read_from(buf)
    }
}

impl Write for Buffer {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.write_into(buf)
    }

    fn flush(&mut self) -> Result<()> { Ok(()) }
}



/// Fixed-size lossy read/write buffer with sequential address mapping.
///
/// This uses a circular address mapping scheme. That is, for any buffer of
/// size `N`, the pointer address range of `0..N` maps to the same physical
/// memory as the range `N..2*N`. This guarantees that the entire read or
/// write range may be addressed as a single sequence of bytes.
///
/// Unlike the [`Buffer`](struct.Buffer.html), writes to this type may evict
/// bytes from the read side of the queue. The writeable size is always equal
/// to the overall capacity of the buffer.
///
/// # Example
///
/// ```
/// # extern crate vmap;
/// #
/// use vmap::buf::RingBuffer;
/// use std::io::{BufRead, Read, Write};
///
/// # fn main() -> std::io::Result<()> {
/// let mut buf = RingBuffer::new(4000)?;
/// let mut i = 1;
/// let mut total = 0;
/// while total < buf.capacity() {
///     let tmp = format!("this is test line {}\n", i);
///     write!(buf, "{}", tmp);
///     total += tmp.len();
///     i += 1;
/// }
///
/// let mut line = String::new();
/// let len = buf.read_line(&mut line)?;
/// println!("total = {}", total);
/// assert_eq!(len, 20);
/// assert_eq!(&line[line.len()-20..], "this is test line 2\n");
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct RingBuffer {
    ptr: *mut u8,
    len: usize,
    rlen: u64,
    wpos: u64,
}

impl RingBuffer {
    /// Constructs a new ring buffer instance.
    ///
    /// The hint is a minimum size for the buffer. This size will be rounded up
    /// to the nearest page size for the actual capacity. The allocation will
    /// occupy double the space in the virtual memory table, but the physical
    /// memory usage will remain at the desired capacity.
    pub fn new(hint: usize) -> Result<Self> {
        let len = AllocSize::new().round(hint);
        unsafe {
            let ptr = map_ring(len)?;
            Ok(Self { ptr: ptr, len: len, rlen: 0, wpos: 0 })
        }
    }

    /// Gets the number of bytes that the buffer has currently allocated space for.
    pub fn capacity(&self) -> usize { self.len }
}

impl Drop for RingBuffer {
    fn drop(&mut self) {
        unsafe { unmap_ring(self.ptr, self.len) }.unwrap_or_default()
    }
}

impl SeqRead for RingBuffer {
    fn as_read_ptr(&self) -> *const u8 { self.ptr }
    fn read_offset(&self) -> usize { ((self.wpos - self.rlen) % self.len as u64) as usize }
    fn read_len(&self) -> usize { self.rlen as usize }
}

impl SeqWrite for RingBuffer {
    fn as_write_ptr(&mut self) -> *mut u8 { self.ptr }
    fn write_offset(&self) -> usize { (self.wpos % self.len as u64) as usize }
    fn write_len(&self) -> usize { self.len }
    fn feed(&mut self, len: usize) {
        self.wpos += cmp::min(len, self.write_len()) as u64;
        self.rlen = cmp::min(self.rlen + len as u64, self.len as u64);
    }
}

impl BufRead for RingBuffer {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        Ok(self.as_read_slice(std::usize::MAX))
    }

    fn consume(&mut self, len: usize) {
        self.rlen -= cmp::min(len, self.read_len()) as u64;
    }
}

impl Read for RingBuffer {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.read_from(buf)
    }
}

impl Write for RingBuffer {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.write_into(buf)
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        let len = {
            let dst = self.as_write_slice(buf.len());
            let len = dst.len();
            let tail = buf.len() - len;
            dst.copy_from_slice(&buf[tail..]);
            len
        };
        self.feed(len);
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}



#[cfg(test)]
mod tests {
    use super::{AllocSize, Buffer, RingBuffer, SeqRead, SeqWrite};
    use std::io::{Write, BufRead};

    #[test]
    fn size() {
        let sz = AllocSize::new();
        let mut buf = Buffer::new(1000).expect("failed to create buffer");
        assert_eq!(buf.capacity(), sz.size(1));
        assert_eq!(buf.read_len(), 0);
        assert_eq!(buf.write_len(), sz.size(1));

        let bytes = String::from("test").into_bytes();
        buf.write_all(&bytes).expect("failed to write all bytes");
        assert_eq!(buf.capacity(), sz.size(1));
        assert_eq!(buf.read_len(), 4);
        assert_eq!(buf.write_len(), sz.size(1) - 4);
    }

    #[test]
    fn wrap() {
        let mut buf = Buffer::new(1000).expect("failed to create ring buffer");
        // pick some bytes that won't fit evenly in the capacity
        let bytes = b"anthropomorphologically";
        let n = buf.capacity() / bytes.len();
        for _ in 0..n {
            buf.write_all(bytes).expect("failed to write");
        }
        assert_eq!(buf.read_len(), n * bytes.len());
        buf.consume((n-1) * bytes.len());
        assert_eq!(buf.read_len(), bytes.len());
        buf.write_all(bytes).expect("failed to write");
        assert_eq!(buf.read_len(), 2*bytes.len());

        let cmp = b"anthropomorphologicallyanthropomorphologically";
        assert_eq!(buf.as_read_slice(cmp.len()), &cmp[..]);
    }

    #[test]
    fn overwrite() {
        let mut ring = RingBuffer::new(1000).expect("failed to create ring");
        // pick some bytes that won't fit evenly in the capacity
        let bytes = b"anthropomorphologically";
        let n = 2*ring.capacity() / bytes.len() + 1;
        for _ in 0..n {
            ring.write_all(bytes).expect("failed to write");
        }
        assert_eq!(ring.read_len(), ring.capacity());

        let cmp = b"anthropomorphologicallyanthropomorphologically";
        let end = bytes.len() - (ring.capacity() % bytes.len());
        assert_eq!(ring.as_read_slice(10), &cmp[end..(end+10)]);
    }
}

