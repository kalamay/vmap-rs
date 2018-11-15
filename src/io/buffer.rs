use super::{Ring, SeqRead, SeqWrite};

use std;
use std::io::{Result, Error, ErrorKind, BufRead, Read, Write};



/// The `BufReader` adds buffering to any reader using a specialized buffer.
///
/// This is very similar `std::io::BufReader`, but it uses a
/// [`Ring`](struct.Ring.html) for the internal buffer, and it provides a
/// configurable low water mark.
///
/// # Example
///
/// ```
/// # extern crate vmap;
/// #
/// use vmap::io::BufReader;
/// # use std::io::prelude::*;
/// # use std::net::{TcpListener, TcpStream};
///
/// # fn main() -> std::io::Result<()> {
/// # let srv = TcpListener::bind("127.0.0.1:54321")?;
/// let sock = TcpStream::connect("127.0.0.1:54321")?;
/// # let (mut cli, _addr) = srv.accept()?;
/// let mut buf = BufReader::new(sock, 4000)?;
/// # cli.write_all(b"hello\nworld\n")?;
/// let mut line = String::new();
/// let len = buf.read_line(&mut line)?;
/// assert_eq!(line, "hello\n");
/// # Ok(())
/// # }
/// ```
pub struct BufReader<R> {
    buf: Ring,
    inner: R,
    lowat: usize,
}

impl<R: Read> BufReader<R> {
    /// Creates a new `BufReader`.
    pub fn new(inner: R, capacity: usize) -> Result<Self> {
        Ok(Self { buf: Ring::new(capacity)?, inner: inner, lowat: 0 })
    }

    /// Get the low-water level.
    pub fn lowat(&self) -> usize { self.lowat }

    /// Set the low-water level.
    ///
    /// When the internal buffer content length drops to this level, a
    /// subsequent read will request more from the inner reader.
    pub fn set_lowat(&mut self, val: usize) { self.lowat = val }

    /// TODO
    pub fn get_ref(&self) -> &R { &self.inner }
    /// TODO
    pub fn get_mut(&mut self) -> &mut R { &mut self.inner }

    /// TODO
    pub fn buffer(&self) -> &[u8] {
        &self.buf.as_read_slice(std::usize::MAX)
    }

    /// TODO
    pub fn into_inner(self) -> R { self.inner }
}

impl<R: Read> Read for BufReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        // If the reader has been dequeued and the destination buffer is larger
        // than the internal buffer, then read directly into the destination.
        if self.buf.read_len() == 0 && buf.len() >= self.buf.write_capacity() {
            return self.inner.read(buf);
        }
        let nread = {
            let mut rem = self.fill_buf()?;
            rem.read(buf)?
        };
        self.consume(nread);
        Ok(nread)
    }
}

impl<R: Read> BufRead for BufReader<R> {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        if self.buf.read_len() <= self.lowat {
            let n = self.inner.read(self.buf.as_write_slice(std::usize::MAX))?;
            self.buf.feed(n);
        }
        Ok(self.buffer())
    }

    fn consume(&mut self, amt: usize) {
        self.buf.consume(amt);
    }
}



/// The `BufWriter` adds buffering to any writer using a specialized buffer.
///
/// This is very similar `std::io::BufWriter`, but it uses a
/// [`Ring`](struct.Ring.html) for internal the buffer.
///
/// # Example
///
/// ```
/// # extern crate vmap;
/// #
/// use vmap::io::{BufReader, BufWriter};
/// # use std::io::prelude::*;
/// # use std::net::{TcpListener, TcpStream};
///
/// # fn main() -> std::io::Result<()> {
/// # let srv = TcpListener::bind("127.0.0.1:54321")?;
/// let recv = TcpStream::connect("127.0.0.1:54321")?;
/// let send = /* accepted socked */
/// # srv.accept()?.0;
///
/// let mut wr = BufWriter::new(send, 4000)?;
/// wr.write_all(b"hello\nworld\n")?;
/// wr.flush()?;
///
/// let mut rd = BufReader::new(recv, 4000)?;
/// let mut line = String::new();
/// let len = rd.read_line(&mut line)?;
/// assert_eq!(line, "hello\n");
/// # Ok(())
/// # }
/// ```
pub struct BufWriter<W: Write> {
    buf: Ring,
    inner: Option<W>,
    panicked: bool
}

impl<W: Write> BufWriter<W> {
    /// Creates a new `BufWriter`.
    pub fn new(inner: W, capacity: usize) -> Result<Self> {
        Ok(Self {
            buf: Ring::new(capacity)?,
            inner: Some(inner),
            panicked: false
        })
    }

    /// TODO
    pub fn get_ref(&self) -> &W { &self.inner.as_ref().unwrap() }
    /// TODO
    pub fn get_mut(&mut self) -> &mut W { self.inner.as_mut().unwrap() }

    /// TODO
    pub fn buffer(&self) -> &[u8] {
        &self.buf.as_read_slice(std::usize::MAX)
    }

    /// TODO
    pub fn into_inner(mut self) -> Result<W> {
        self.flush_buf()?;
        Ok(self.inner.take().unwrap())
    }

    fn flush_buf(&mut self) -> Result<()> {
        let mut written = 0;
        let len = self.buf.read_len();
        let mut ret = Ok(());
        while written < len {
            self.panicked = true;
            let r = self.inner.as_mut().unwrap().write(self.buf.as_read_slice(std::usize::MAX));
            self.panicked = false;

            match r {
                Ok(0) => {
                    ret = Err(Error::new(ErrorKind::WriteZero,
                                         "failed to write the buffered data"));
                    break;
                }
                Ok(n) => written += n,
                Err(ref e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => { ret = Err(e); break }
            }
        }
        if written > 0 {
            self.buf.consume(written);
        }
        ret
    }
}

impl<W: Write> Drop for BufWriter<W> {
    fn drop(&mut self) {
        if self.inner.is_some() && !self.panicked {
            let _r = self.flush_buf();
        }
    }
}

impl<W: Write> Write for BufWriter<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        if buf.len() > self.buf.write_len() {
            self.flush_buf()?;
        }
        if buf.len() >= self.buf.write_len() {
            self.panicked = true;
            let r = self.inner.as_mut().unwrap().write(buf);
            self.panicked = false;
            r
        } else {
            self.buf.write(buf)
        }
    }
    fn flush(&mut self) -> Result<()> {
        self.flush_buf().and_then(|()| self.get_mut().flush())
    }
}
