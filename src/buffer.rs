use std::slice;
use std::io::{Result, BufRead};
use std::cmp;

pub trait Seq: BufRead {
    fn mut_ptr(&self) -> *mut u8;
    fn read_offset(&self) -> isize;
    fn write_offset(&self) -> isize;

    fn capacity(&self) -> usize;
    fn readable(&self) -> usize;
    fn feed(&mut self, len: usize);

    fn writable(&self) -> usize { self.capacity() - self.readable() }

    fn is_empty(&self) -> bool { self.readable() == 0 }

    fn is_full(&self) -> bool { self.readable() == self.capacity() }

    fn head(&self, max: usize) -> &[u8] {
        unsafe {
            slice::from_raw_parts_mut(
                self.mut_ptr().offset(self.read_offset()),
                cmp::min(self.readable(), max))
        }
    }

    fn tail(&self, max: usize) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(
                self.mut_ptr().offset(self.write_offset()),
                cmp::min(self.writable(), max))
        }
    }

    fn read_from(&mut self, buf: &mut [u8]) -> Result<usize> {
        let len = {
            let src = self.head(buf.len());
            let len = src.len();
            buf[..len].copy_from_slice(src);
            len
        };
        self.consume(len);
        Ok(len)
    }

    fn write_into(&mut self, buf: &[u8]) -> Result<usize> {
        let len = {
            let dst = self.tail(buf.len());
            let len = dst.len();
            dst.copy_from_slice(&buf[..len]);
            len
        };
        self.feed(len);
        Ok(len)
    }
}
