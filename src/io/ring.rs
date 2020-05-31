use super::{SeqRead, SeqWrite};
use crate::os::{map_ring, unmap_ring};
use crate::AllocSize;

use std;
use std::cmp;
use std::io::{BufRead, Read, Result, Write};

/// Fixed-size reliable read/write buffer with sequential address mapping.
///
/// This uses a circular address mapping scheme. That is, for any buffer of
/// size `N`, the pointer address range of `0..N` maps to the same physical
/// memory as the range `N..2*N`. This guarantees that the entire read or
/// write range may be addressed as a single sequence of bytes.
///
/// Unlike the [`InfiniteRing`](struct.InfiniteRing.html), this type otherise
/// acts as a "normal" buffer. Writes fill up the buffer, and when full, no
/// furthur writes may be performed until a read occurs. The writable length
/// sequence is the capacity of the buffer, less any pending readable bytes.
///
/// # Example
///
/// ```
/// # extern crate vmap;
/// #
/// use vmap::io::{Ring, SeqWrite};
/// use std::io::{BufRead, Read, Write};
///
/// # fn main() -> std::io::Result<()> {
/// let mut buf = Ring::new(4000)?;
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
pub struct Ring {
    ptr: *mut u8,
    len: usize,
    rpos: u64,
    wpos: u64,
}

impl Ring {
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
            Ok(Self {
                ptr: ptr,
                len: len,
                rpos: 0,
                wpos: 0,
            })
        }
    }
}

impl Drop for Ring {
    fn drop(&mut self) {
        unsafe { unmap_ring(self.ptr, self.len) }.unwrap_or_default();
    }
}

impl SeqRead for Ring {
    fn as_read_ptr(&self) -> *const u8 {
        self.ptr
    }
    fn read_offset(&self) -> usize {
        (self.rpos % self.len as u64) as usize
    }
    fn read_len(&self) -> usize {
        (self.wpos - self.rpos) as usize
    }
}

impl SeqWrite for Ring {
    fn as_write_ptr(&mut self) -> *mut u8 {
        self.ptr
    }
    fn write_offset(&self) -> usize {
        (self.wpos % self.len as u64) as usize
    }
    fn write_len(&self) -> usize {
        self.len - self.read_len()
    }
    fn write_capacity(&self) -> usize {
        self.len
    }
    fn feed(&mut self, len: usize) {
        self.wpos += cmp::min(len, self.write_len()) as u64;
    }
}

impl BufRead for Ring {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        Ok(self.as_read_slice(std::usize::MAX))
    }

    fn consume(&mut self, len: usize) {
        self.rpos += cmp::min(len, self.read_len()) as u64;
    }
}

impl Read for Ring {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.read_from(buf)
    }
}

impl Write for Ring {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.write_into(buf)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Fixed-size lossy read/write buffer with sequential address mapping.
///
/// This uses a circular address mapping scheme. That is, for any buffer of
/// size `N`, the pointer address range of `0..N` maps to the same physical
/// memory as the range `N..2*N`. This guarantees that the entire read or
/// write range may be addressed as a single sequence of bytes.
///
/// Unlike the [`Ring`](struct.Ring.html), writes to this type may evict
/// bytes from the read side of the queue. The writeable size is always equal
/// to the overall capacity of the buffer.
///
/// # Example
///
/// ```
/// # extern crate vmap;
/// #
/// use vmap::io::{InfiniteRing, SeqWrite};
/// use std::io::{BufRead, Read, Write};
///
/// # fn main() -> std::io::Result<()> {
/// let mut buf = InfiniteRing::new(4000)?;
/// let mut i = 1;
/// let mut total = 0;
/// while total < buf.write_capacity() {
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
pub struct InfiniteRing {
    ptr: *mut u8,
    len: usize,
    rlen: u64,
    wpos: u64,
}

impl InfiniteRing {
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
            Ok(Self {
                ptr: ptr,
                len: len,
                rlen: 0,
                wpos: 0,
            })
        }
    }
}

impl Drop for InfiniteRing {
    fn drop(&mut self) {
        unsafe { unmap_ring(self.ptr, self.len) }.unwrap_or_default()
    }
}

impl SeqRead for InfiniteRing {
    fn as_read_ptr(&self) -> *const u8 {
        self.ptr
    }
    fn read_offset(&self) -> usize {
        ((self.wpos - self.rlen) % self.len as u64) as usize
    }
    fn read_len(&self) -> usize {
        self.rlen as usize
    }
}

impl SeqWrite for InfiniteRing {
    fn as_write_ptr(&mut self) -> *mut u8 {
        self.ptr
    }
    fn write_offset(&self) -> usize {
        (self.wpos % self.len as u64) as usize
    }
    fn write_len(&self) -> usize {
        self.len
    }
    fn write_capacity(&self) -> usize {
        self.len
    }
    fn feed(&mut self, len: usize) {
        self.wpos += cmp::min(len, self.write_len()) as u64;
        self.rlen = cmp::min(self.rlen + len as u64, self.len as u64);
    }
}

impl BufRead for InfiniteRing {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        Ok(self.as_read_slice(std::usize::MAX))
    }

    fn consume(&mut self, len: usize) {
        self.rlen -= cmp::min(len, self.read_len()) as u64;
    }
}

impl Read for InfiniteRing {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.read_from(buf)
    }
}

impl Write for InfiniteRing {
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
