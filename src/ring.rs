use super::{unmap_ring, Seq};

use std;
use std::io::{Result, BufRead, Read, Write};
use std::cmp;



#[derive(Debug)]
pub struct Ring {
    ptr: *mut u8,
    len: usize,
    rpos: u64,
    wpos: u64,
}

impl Ring {
    pub unsafe fn new(ptr: *mut u8, len: usize) -> Self {
        Ring { ptr: ptr, len: len, rpos: 0, wpos: 0 }
    }
}

impl Drop for Ring {
    fn drop(&mut self) {
        unsafe { unmap_ring(self.ptr, self.len) }.unwrap_or_default();
    }
}

impl Seq for Ring {
    fn mut_ptr(&self) -> *mut u8 { self.ptr }
    fn read_offset(&self) -> isize { (self.rpos % self.len as u64) as isize }
    fn write_offset(&self) -> isize { (self.wpos % self.len as u64) as isize }
    fn capacity(&self) -> usize { self.len }
    fn readable(&self) -> usize { (self.wpos - self.rpos) as usize }

    fn feed(&mut self, len: usize) {
        self.wpos += cmp::min(len, self.writable()) as u64;
    }
}

impl BufRead for Ring {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        Ok(self.head(std::usize::MAX))
    }

    fn consume(&mut self, len: usize) {
        self.rpos += cmp::min(len, self.readable()) as u64;
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



#[derive(Debug)]
pub struct UnboundRing {
    ptr: *mut u8,
    len: usize,
    rlen: u64,
    wpos: u64,
}

impl UnboundRing {
    pub unsafe fn new(ptr: *mut u8, len: usize) -> Self {
        UnboundRing { ptr: ptr, len: len, rlen: 0, wpos: 0 }
    }
}

impl Drop for UnboundRing {
    fn drop(&mut self) {
        unsafe { unmap_ring(self.ptr, self.len) }.unwrap_or_default()
    }
}

impl Seq for UnboundRing {
    fn mut_ptr(&self) -> *mut u8 { self.ptr }
    fn read_offset(&self) -> isize { ((self.wpos - self.rlen) % self.len as u64) as isize }
    fn write_offset(&self) -> isize { (self.wpos % self.len as u64) as isize }
    fn capacity(&self) -> usize { self.len }
    fn readable(&self) -> usize { self.rlen as usize }
    fn writable(&self) -> usize { self.capacity() }

    fn feed(&mut self, len: usize) {
        self.wpos += cmp::min(len, self.writable()) as u64;
        self.rlen = cmp::min(self.rlen + len as u64, self.capacity() as u64);
    }
}

impl BufRead for UnboundRing {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        Ok(self.head(std::usize::MAX))
    }

    fn consume(&mut self, len: usize) {
        self.rlen -= cmp::min(len, self.readable()) as u64;
    }
}

impl Read for UnboundRing {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.read_from(buf)
    }
}

impl Write for UnboundRing {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.write_into(buf)
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        let len = {
            let dst = self.tail(buf.len());
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
    use super::super::{Alloc, Seq};
    use std::io::{Write, BufRead};

    #[test]
    fn size() {
        let alloc = Alloc::new();
        let mut ring = alloc.ring(1000).expect("failed to create ring");
        assert_eq!(ring.capacity(), alloc.page_size(1));
        assert_eq!(ring.readable(), 0);
        assert_eq!(ring.writable(), alloc.page_size(1));

        let bytes = String::from("test").into_bytes();
        ring.write_all(&bytes).expect("failed to write all bytes");
        assert_eq!(ring.capacity(), alloc.page_size(1));
        assert_eq!(ring.readable(), 4);
        assert_eq!(ring.writable(), alloc.page_size(1) - 4);
    }

    #[test]
    fn wrap() {
        let alloc = Alloc::new();
        let mut ring = alloc.ring(1000).expect("failed to create ring");
        // pick some bytes that won't fit evenly in the ring
        let bytes = b"anthropomorphologically";
        let n = ring.capacity() / bytes.len();
        for _ in 0..n {
            ring.write_all(bytes).expect("failed to write");
        }
        assert_eq!(ring.readable(), n * bytes.len());
        ring.consume((n-1) * bytes.len());
        assert_eq!(ring.readable(), bytes.len());
        ring.write_all(bytes).expect("failed to write");
        assert_eq!(ring.readable(), 2*bytes.len());

        let cmp = b"anthropomorphologicallyanthropomorphologically";
        assert_eq!(ring.head(cmp.len()), &cmp[..]);
    }

    #[test]
    fn overwrite() {
        let alloc = Alloc::new();
        let mut ring = alloc.unbound_ring(1000).expect("failed to create ring");
        let bytes = b"anthropomorphologically";
        let n = 2*ring.capacity() / bytes.len() + 1;
        for _ in 0..n {
            ring.write_all(bytes).expect("failed to write");
        }
        assert_eq!(ring.readable(), ring.capacity());

        let cmp = b"anthropomorphologicallyanthropomorphologically";
        let end = bytes.len() - (ring.capacity() % bytes.len());
        assert_eq!(ring.head(10), &cmp[end..(end+10)]);
    }
}

