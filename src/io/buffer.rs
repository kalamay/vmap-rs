use super::{Ring, SeqRead, SeqWrite};
use crate::Result;

use std::{
    fmt,
    io::{self, BufRead, ErrorKind, Read, Write},
    ops::{Deref, DerefMut},
};

/// The `BufReader` adds buffering to any reader using a specialized buffer.
///
/// This is very similar `std::io::BufReader`, but it uses a
/// [`Ring`](struct.Ring.html) for the internal buffer, and it provides a
/// configurable low water mark.
///
/// # Examples
///
/// ```
/// use vmap::io::BufReader;
/// # use std::io::prelude::*;
/// # use std::net::{TcpListener, TcpStream};
///
/// # fn main() -> std::io::Result<()> {
/// # let srv = TcpListener::bind("127.0.0.1:0")?;
/// let sock = TcpStream::connect(srv.local_addr().unwrap())?;
/// # let (mut cli, _addr) = srv.accept()?;
/// let mut buf = BufReader::new(sock, 4000).expect("failed to create buffer");
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
        Ok(Self {
            buf: Ring::new(capacity)?,
            inner,
            lowat: 0,
        })
    }

    /// Get the low-water level.
    pub fn lowat(&self) -> usize {
        self.lowat
    }

    /// Set the low-water level.
    ///
    /// When the internal buffer content length drops to this level, a
    /// subsequent read will request more from the inner reader.
    pub fn set_lowat(&mut self, val: usize) {
        self.lowat = val
    }

    /// Gets a reference to the underlying reader.
    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    /// Gets a mutable reference to the underlying reader.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    /// Returns a reference to the internally buffered data.
    pub fn buffer(&self) -> &[u8] {
        self.buf.as_read_slice(std::usize::MAX)
    }

    /// Unwraps this `BufReader`, returning the underlying reader.
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Deref for BufReader<R> {
    type Target = R;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.get_ref()
    }
}

impl<R: Read> DerefMut for BufReader<R> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.get_mut()
    }
}

impl<R> AsRef<R> for BufReader<R>
where
    R: Read,
    <BufReader<R> as Deref>::Target: AsRef<R>,
{
    fn as_ref(&self) -> &R {
        self.deref()
    }
}

impl<R> AsMut<R> for BufReader<R>
where
    R: Read,
    <BufReader<R> as Deref>::Target: AsMut<R>,
{
    fn as_mut(&mut self) -> &mut R {
        self.deref_mut()
    }
}

impl<R: Read> Read for BufReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
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
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
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
/// # Examples
///
/// ```
/// use vmap::io::{BufReader, BufWriter};
/// # use std::io::prelude::*;
/// # use std::net::{TcpListener, TcpStream};
///
/// # fn main() -> std::io::Result<()> {
/// # let srv = TcpListener::bind("127.0.0.1:0")?;
/// let recv = TcpStream::connect(srv.local_addr().unwrap())?;
/// let send = /* accepted socked */
/// # srv.accept()?.0;
///
/// let mut wr = BufWriter::new(send, 4000).unwrap();
/// wr.write_all(b"hello\nworld\n")?;
/// wr.flush()?;
///
/// let mut rd = BufReader::new(recv, 4000).unwrap();
/// let mut line = String::new();
/// let len = rd.read_line(&mut line)?;
/// assert_eq!(line, "hello\n");
/// # Ok(())
/// # }
/// ```
pub struct BufWriter<W: Write> {
    buf: Ring,
    inner: Option<W>,
    panicked: bool,
}

impl<W: Write> BufWriter<W> {
    /// Creates a new `BufWriter`.
    pub fn new(inner: W, capacity: usize) -> Result<Self> {
        Ok(Self {
            buf: Ring::new(capacity)?,
            inner: Some(inner),
            panicked: false,
        })
    }

    /// Gets a reference to the underlying writer.
    pub fn get_ref(&self) -> &W {
        self.inner.as_ref().unwrap()
    }

    /// Gets a mutable reference to the underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        self.inner.as_mut().unwrap()
    }

    /// Unwraps this `BufWriter`, returning the underlying writer.
    pub fn into_inner(mut self) -> io::Result<W> {
        self.flush_buf()?;
        Ok(self.inner.take().unwrap())
    }

    fn flush_buf(&mut self) -> io::Result<()> {
        let mut written = 0;
        let len = self.buf.read_len();
        let mut ret = Ok(());
        while written < len {
            self.panicked = true;
            let r = self
                .inner
                .as_mut()
                .unwrap()
                .write(self.buf.as_read_slice(std::usize::MAX));
            self.panicked = false;

            match r {
                Ok(0) => {
                    ret = Err(ErrorKind::WriteZero.into());
                    break;
                }
                Ok(n) => written += n,
                Err(ref e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => {
                    ret = Err(e);
                    break;
                }
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
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
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

    fn flush(&mut self) -> io::Result<()> {
        self.flush_buf().and_then(|()| self.get_mut().flush())
    }
}
