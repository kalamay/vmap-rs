use super::{Ring, SeqRead, SeqWrite};
use crate::Result;

use std::{
    fmt,
    io::{self, BufRead, ErrorKind, Read, Write},
    ops::{Deref, DerefMut},
};

/// The `BufReader` adds buffering to any reader using a specialized buffer.
///
/// This is very similar `std::io::BufReader`, but it uses a [`Ring`] for the
/// internal buffer, and it provides a configurable low water mark.
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
    #[inline]
    pub fn lowat(&self) -> usize {
        self.lowat
    }

    /// Set the low-water level.
    ///
    /// When the internal buffer content length drops to this level or below, a
    /// subsequent call to `fill_buffer()` will request more from the inner reader.
    ///
    /// If it desired for `fill_buffer()` to always request a `read()`, you
    /// may use:
    ///
    /// ```
    /// # use vmap::io::BufReader;
    /// # fn main() -> std::io::Result<()> {
    /// let mut buf = BufReader::new(std::io::stdin(), 4096)?;
    /// buf.set_lowat(usize::MAX);
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn set_lowat(&mut self, val: usize) {
        self.lowat = val
    }

    /// Gets a reference to the underlying reader.
    #[inline]
    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    /// Gets a mutable reference to the underlying reader.
    #[inline]
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    /// Returns a reference to the internally buffered data.
    #[inline]
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

impl<R: Read + Write> Write for BufReader<R> {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    #[inline]
    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        self.inner.write_vectored(bufs)
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.inner.write_all(buf)
    }

    #[inline]
    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> io::Result<()> {
        self.inner.write_fmt(fmt)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
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
/// This is very similar `std::io::BufWriter`, but it uses a [`Ring`] for the
/// internal the buffer.
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
    inner: W,
    panicked: bool,
}

impl<W: Write> BufWriter<W> {
    /// Creates a new `BufWriter`.
    pub fn new(inner: W, capacity: usize) -> Result<Self> {
        Ok(Self::from_parts(inner, Ring::new(capacity)?))
    }

    /// Creates a new `BufWriter` using an allocated, and possibly populated,
    /// [`Ring`] instance. Consider calling [`Ring::clear()`] prior if the
    /// contents of the ring should be discarded.
    pub fn from_parts(inner: W, buf: Ring) -> Self {
        Self {
            buf,
            inner,
            panicked: false,
        }
    }

    /// Gets a reference to the underlying writer.
    #[inline]
    pub fn get_ref(&self) -> &W {
        &self.inner
    }

    /// Gets a mutable reference to the underlying writer.
    #[inline]
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    /// Unwraps this `BufWriter`, returning the underlying writer.
    ///
    /// On `Err`, the result is a tuple combining the error that occurred while
    /// flusing the buffer, and the buffer object.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::{self, Write, ErrorKind};
    /// use vmap::io::BufWriter;
    ///
    /// struct ErringWriter(usize);
    /// impl Write for ErringWriter {
    ///   fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    ///     // eventually fails with BrokenPipe
    /// #   match self.0.min(buf.len()) {
    /// #     0 => Err(ErrorKind::BrokenPipe.into()),
    /// #     n => { self.0 -= n; Ok(n) },
    /// #   }
    ///   }
    ///   fn flush(&mut self) -> io::Result<()> { Ok(()) }
    /// }
    ///
    /// # fn main() -> vmap::Result<()> {
    /// let mut stream = BufWriter::new(ErringWriter(6), 4096)?;
    /// stream.write_all(b"hello\nworld\n")?;
    ///
    /// // flush the buffer and get the original stream back
    /// let stream = match stream.into_inner() {
    ///     Ok(s) => s,
    ///     Err(e) => {
    ///         assert_eq!(e.error().kind(), ErrorKind::BrokenPipe);
    ///
    ///         // You can forcefully obtain the stream, however it is in an
    ///         // failing state.
    ///         let (recovered_writer, ring) = e.into_inner().into_parts();
    ///         assert_eq!(ring.unwrap().as_ref(), b"world\n");
    ///         recovered_writer
    ///     }
    /// };
    /// # Ok(())
    /// # }
    /// ```
    pub fn into_inner(mut self) -> std::result::Result<W, IntoInnerError<W>> {
        match self.flush_buf() {
            Err(e) => Err(IntoInnerError(self, e)),
            Ok(()) => Ok(self.into_parts().0),
        }
    }

    /// Disassembles this [`BufWriter`] into the underlying writer and the [`Ring`]
    /// used for buffering, containing any buffered but unwritted data.
    ///
    /// If the underlying writer panicked during the previous write, the [`Ring`]
    /// will be wrapped in a [`WriterPanicked`] error. In this case, the content
    /// still buffered within the [`Ring`] may or may not have been written.
    ///
    /// # Example
    ///
    /// ```
    /// use std::io::{self, Write};
    /// use std::panic::{catch_unwind, AssertUnwindSafe};
    /// use vmap::io::BufWriter;
    ///
    /// struct PanickingWriter;
    /// impl Write for PanickingWriter {
    ///   fn write(&mut self, buf: &[u8]) -> io::Result<usize> { panic!() }
    ///   fn flush(&mut self) -> io::Result<()> { panic!() }
    /// }
    ///
    /// # fn main() -> vmap::Result<()> {
    /// let mut stream = BufWriter::new(PanickingWriter, 4096)?;
    /// stream.write_all(b"testing")?;
    /// let result = catch_unwind(AssertUnwindSafe(|| {
    ///     stream.flush().unwrap()
    /// }));
    /// assert!(result.is_err());
    /// let (recovered_writer, ring) = stream.into_parts();
    /// assert!(matches!(recovered_writer, PanickingWriter));
    /// assert_eq!(ring.unwrap_err().into_inner().as_ref(), b"testing");
    /// # Ok(())
    /// # }
    /// ```
    pub fn into_parts(self) -> (W, std::result::Result<Ring, WriterPanicked>) {
        // SAFETY: forget(self) prevents double dropping inner and buf.
        let inner = unsafe { std::ptr::read(&self.inner) };
        let buf = unsafe { std::ptr::read(&self.buf) };
        let buf = if self.panicked {
            Err(WriterPanicked(buf))
        } else {
            Ok(buf)
        };

        std::mem::forget(self);

        (inner, buf)
    }

    fn flush_buf(&mut self) -> io::Result<()> {
        loop {
            if self.buf.is_empty() {
                break Ok(());
            }

            self.panicked = true;
            let r = self.inner.write(self.buf.as_read_slice(std::usize::MAX));
            self.panicked = false;

            match r {
                Ok(0) => {
                    break Err(ErrorKind::WriteZero.into());
                }
                Ok(n) => self.buf.consume(n),
                Err(ref e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => break Err(e),
            }
        }
    }
}

impl<W: Write> Deref for BufWriter<W> {
    type Target = W;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.get_ref()
    }
}

impl<W: Write> DerefMut for BufWriter<W> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.get_mut()
    }
}

impl<W> AsRef<W> for BufWriter<W>
where
    W: Write,
    <BufWriter<W> as Deref>::Target: AsRef<W>,
{
    fn as_ref(&self) -> &W {
        self.deref()
    }
}

impl<W> AsMut<W> for BufWriter<W>
where
    W: Write,
    <BufWriter<W> as Deref>::Target: AsMut<W>,
{
    fn as_mut(&mut self) -> &mut W {
        self.deref_mut()
    }
}

impl<W: Write> Drop for BufWriter<W> {
    fn drop(&mut self) {
        if !self.panicked {
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
            let r = self.inner.write(buf);
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

impl<W: Write + Read> Read for BufWriter<W> {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }

    #[inline]
    fn read_vectored(&mut self, bufs: &mut [io::IoSliceMut<'_>]) -> io::Result<usize> {
        self.inner.read_vectored(bufs)
    }

    #[inline]
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.inner.read_to_end(buf)
    }

    #[inline]
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        self.inner.read_to_string(buf)
    }

    #[inline]
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.inner.read_exact(buf)
    }
}

/// An error returned by [`BufWriter::into_inner`] which combines an error that
/// happened while writing out the buffer, and the buffered writer object
/// which may be used to recover from the condition.
pub struct IntoInnerError<W: Write>(BufWriter<W>, io::Error);

impl<W: Write> IntoInnerError<W> {
    /// Returns the error which caused the call to [`BufWriter::into_inner()`]
    /// to fail.
    pub fn error(&self) -> &io::Error {
        &self.1
    }

    /// Consumes the [`IntoInnerError`] and returns the buffered writer which
    /// received the error.
    pub fn into_inner(self) -> BufWriter<W> {
        self.0
    }

    /// Consumes the [`IntoInnerError`] and returns the error which caused the call to
    /// [`BufWriter::into_inner()`] to fail. Unlike `error`, this can be used to
    /// obtain ownership of the underlying error.
    pub fn into_error(self) -> io::Error {
        self.1
    }

    /// Consumes the [`IntoInnerError`] and returns the error which caused the call to
    /// [`BufWriter::into_inner()`] to fail, and the underlying writer.
    pub fn into_parts(self) -> (io::Error, BufWriter<W>) {
        (self.1, self.0)
    }
}

/// Error returned for the buffered data from `BufWriter::into_parts` when the underlying
/// writer has previously panicked. The contents of the buffer may be partially written.
pub struct WriterPanicked(Ring);

impl WriterPanicked {
    /// Returns the [`Ring`] with possibly unwritten data.
    pub fn into_inner(self) -> Ring {
        self.0
    }

    const DESCRIPTION: &'static str = "writer panicked, unwritten data may remain";
}

impl fmt::Display for WriterPanicked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", Self::DESCRIPTION)
    }
}

impl fmt::Debug for WriterPanicked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriterPanicked")
            .field(
                "buffer",
                &format_args!("{}/{}", self.0.write_len(), self.0.write_capacity()),
            )
            .finish()
    }
}
