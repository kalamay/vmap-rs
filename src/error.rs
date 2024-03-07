//! Types for working with various map operation errors.

use std::{fmt, io};

/// A specialized `Result` type for map operations.
pub type Result<T> = std::result::Result<T, Error>;

/// A specialiazed `Result` type for conversion operations.
///
/// The origin `self` type is consumed When converting between [`Map`]
/// and [`MapMut`] types. The `ConvertResult` returns the original input
/// value on failure so that it isn't necessarily dropped. This allows
/// a failure to be handled while stil maintaining the existing mapping.
///
/// [`Map`]: struct.Map.html
/// [`MapMut`]: struct.MapMut.html
pub type ConvertResult<T, F> = std::result::Result<T, (Error, F)>;

impl<F> From<(Error, F)> for Error {
    /// Converts the `(Error, F)` tuple from a [`ConvertResult`] result into
    /// an [`Error`], dropping the failed map in the process.
    ///
    /// [`ConvertResult`]: type.ConvertResult.html
    /// [`Error`]: type.Error.html
    fn from(value: (Error, F)) -> Error {
        value.0
    }
}

/// A list specifying general categories of map errors.
pub struct Error {
    repr: Repr,
    op: Operation,
}

enum Repr {
    Io(io::Error),
    Input(Input),
    System(system_error::Error),
}

impl Error {
    /// Returns an error that wraps a `std::io::Error` along with an [`Operation`].
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::ErrorKind;
    /// use vmap::{Error, Operation};
    ///
    /// println!("I/O error: {:?}", Error::io(
    ///     Operation::MapFile,
    ///     ErrorKind::NotFound.into(),
    /// ));
    /// ```
    ///
    /// [`Operation`]: enum.Operation.html
    pub fn io(op: Operation, err: io::Error) -> Self {
        Self {
            repr: Repr::Io(err),
            op,
        }
    }

    /// Returns an error that wraps an [`Input`] type along with an [`Operation`].
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::{Error, Operation, Input};
    ///
    /// println!("Input error: {:?}", Error::input(
    ///     Operation::MapFile,
    ///     Input::InvalidRange,
    /// ));
    /// ```
    ///
    /// [`Input`]: enum.Input.html
    /// [`Operation`]: enum.Operation.html
    pub fn input(op: Operation, input: Input) -> Self {
        Self {
            repr: Repr::Input(input),
            op,
        }
    }

    /// Returns an error that wraps a `system_error::Error` along with an [`Operation`].
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::ErrorKind;
    /// use vmap::{Error, Operation};
    ///
    /// println!("System error: {:?}", Error::system(
    ///     Operation::MapFile,
    ///     system_error::Error::last_os_error()
    /// ));
    /// ```
    ///
    /// [`system_error::Error`]: https://docs.rs/system_error/0.1.1/system_error/struct.Error.html
    /// [`Operation`]: enum.Operation.html
    pub fn system(op: Operation, err: system_error::Error) -> Self {
        Self {
            repr: Repr::System(err),
            op,
        }
    }

    /// Returns an error that wraps a [`system_error::KernelCode`] along with an [`Operation`].
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::{Error, Operation};
    ///
    /// println!("Kernel error: {:?}", Error::kernel(
    ///     Operation::RingAllocate,
    ///     1,
    /// ));
    /// ```
    ///
    /// [`system_error::KernelCode`]: https://docs.rs/system_error/0.1.1/system_error/type.KernelCode.html
    /// [`Operation`]: enum.Operation.html
    pub fn kernel(op: Operation, code: system_error::KernelCode) -> Self {
        Self::system(op, system_error::Error::from_raw_kernel_error(code))
    }

    /// Returns an error representing the last OS error which occurred.
    ///
    /// This function reads the value of `errno` for the target platform (e.g.
    /// `GetLastError` on Windows) and will return a corresponding instance of
    /// `Error` for the error code.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::{Error, Operation};
    ///
    /// println!("last OS error: {:?}", Error::last_os_error(Operation::MapFile));
    /// ```
    pub fn last_os_error(op: Operation) -> Self {
        Self::system(op, system_error::Error::last_os_error())
    }

    /// Returns the OS error that this error represents (if any).
    ///
    /// If this `Error` was constructed via `last_os_error`, then this function
    /// will return `Some`, otherwise it will return `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::{Error, Input, Operation};
    ///
    /// fn print_os_error(err: &Error) {
    ///     if let Some(raw_os_err) = err.raw_os_error() {
    ///         println!("raw OS error: {:?}", raw_os_err);
    ///     } else {
    ///         println!("Not an OS error");
    ///     }
    /// }
    ///
    /// // Will print "raw OS error: ...".
    /// print_os_error(&Error::last_os_error(Operation::MapFile));
    /// // Will print "Not an OS error".
    /// print_os_error(&Error::input(Operation::MapFile, Input::InvalidRange));
    /// ```
    pub fn raw_os_error(&self) -> Option<i32> {
        match &self.repr {
            Repr::Io(err) => err.raw_os_error(),
            Repr::Input(_) => None,
            Repr::System(err) => err.raw_os_error(),
        }
    }

    /// Returns the corresponding `std::io::ErrorKind` for this error.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::ErrorKind;
    /// use vmap::{Error, Operation};
    ///
    /// fn print_error(err: Error) {
    ///     println!("{:?}", err.kind());
    /// }
    ///
    /// // Will print "Other".
    /// print_error(Error::last_os_error(Operation::MapFile));
    /// // Will print "NotFound".
    /// print_error(Error::io(Operation::MapFile, ErrorKind::NotFound.into()));
    /// ```
    pub fn kind(&self) -> io::ErrorKind {
        match self.repr {
            Repr::Io(ref err) => err.kind(),
            Repr::Input(_) => io::ErrorKind::InvalidInput,
            Repr::System(ref err) => err.kind(),
        }
    }

    /// Returns the corresponding [`Operation`] that cuased the error.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::{Error, Operation};
    ///
    /// fn print_operation(err: Error) {
    ///     println!("{:?}", err.operation());
    /// }
    ///
    /// // Will print "MapFile".
    /// print_operation(Error::last_os_error(Operation::MapFile));
    /// ```
    ///
    /// [`Operation`]: enum.Operation.html
    pub fn operation(&self) -> Operation {
        self.op
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self.repr {
            Repr::Io(ref err) => Some(err),
            Repr::Input(_) => None,
            Repr::System(_) => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self {
            repr: Repr::Io(err),
            op: Operation::None,
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (field, value) = match self.repr {
            Repr::Io(ref err) => ("io", err as &dyn fmt::Debug),
            Repr::Input(ref input) => ("input", input as &dyn fmt::Debug),
            Repr::System(ref err) => ("system", err as &dyn fmt::Debug),
        };
        fmt.debug_struct("Error")
            .field("op", &self.op)
            .field("kind", &self.kind())
            .field(field, value)
            .finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self.repr {
            Repr::Io(ref err) => err as &dyn fmt::Display,
            Repr::Input(ref input) => input as &dyn fmt::Display,
            Repr::System(ref err) => err as &dyn fmt::Display,
        };
        if let Some(op) = self.op.as_str() {
            write!(fmt, "failed to {}, {}", op, value)
        } else {
            value.fmt(fmt)
        }
    }
}

/// A list specifying general categories of erroneous operations.
///
/// This list is intended to grow over time and it is not recommended to
/// exhaustively match against it.
///
/// It is used with the [`Error`] type.
///
/// [`Error`]: struct.Error.html
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum Operation {
    /// The operation failed while attempting to map a file.
    MapFile,
    /// A map file handle failed to open.
    MapFileHandle,
    /// The view for a map file handle could not be created.
    MapFileView,
    /// The operation failed while attempting to allocate an anonymous mapping.
    MapAnonymous,
    /// An anonymous mapping handle failed to open.
    MapAnonymousHandle,
    /// The view for an anonymouse mapping handle could not be created.
    MapAnonymousView,
    /// A pointer could not be unmapped.
    Unmap,
    /// The [`Protect`] could not be applied to the provided memory region.
    ///
    /// [`Protect`]: ../enum.Protect.html
    Protect,
    /// The [`Advise`] could not be applied to the provided memory region.
    ///
    /// [`Advise`]: ../enum.Advise.html
    Advise,
    /// The physical page could not be locked into memory.
    Lock,
    /// The physical page could not be unlocked from memory.
    Unlock,
    /// A flush cannot be perfomed for the provided input.
    Flush,
    /// The full address space for a ring could not be allocated.
    RingAllocate,
    /// The full address space for a ring could not be deallocated.
    RingDeallocate,
    /// A virtual mapping entry could not be created.
    RingEntry,
    /// The mapping for the first half of the ring failed to allocate.
    RingPrimary,
    /// The mapping for the second half of the ring failed to allocate.
    RingSecondary,
    /// A temporary memory file descriptor failed to open.
    MemoryFd,
    /// Used for pure I/O errors to simplify wrapping a `std::io::Error` into an
    ///
    /// [`Error`]: struct.Error.html
    None,
}

impl Operation {
    /// Returns a display message fragment describing the `Operation` type.
    ///
    /// The result of `as_str` is used to `Display` the `Operation`.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::Operation;
    ///
    /// fn print_operation(op: Operation) {
    ///     println!("failed to {}", op.as_str().unwrap());
    /// }
    ///
    /// // Will print "failed to map file".
    /// print_operation(Operation::MapFile);
    /// ```
    pub fn as_str(&self) -> Option<&'static str> {
        match *self {
            Operation::MapFile => Some("map file"),
            Operation::MapFileHandle => Some("map file handle"),
            Operation::MapFileView => Some("map file view"),
            Operation::MapAnonymous => Some("map anonymous"),
            Operation::MapAnonymousHandle => Some("map anonymous handle"),
            Operation::MapAnonymousView => Some("map anonymous view"),
            Operation::Unmap => Some("unmap"),
            Operation::Protect => Some("protect mapped memory"),
            Operation::Advise => Some("advise mapped memory"),
            Operation::Lock => Some("lock mapped memory"),
            Operation::Unlock => Some("unlock mapped memory"),
            Operation::Flush => Some("flush mapped memory"),
            Operation::RingAllocate => Some("allocate full ring"),
            Operation::RingDeallocate => Some("deallocate full ring"),
            Operation::RingEntry => Some("make ring memory entry"),
            Operation::RingPrimary => Some("map ring first half"),
            Operation::RingSecondary => Some("map ring second half"),
            Operation::MemoryFd => Some("open memory fd"),
            Operation::None => None,
        }
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str(self.as_str().unwrap_or(""))
    }
}

/// A list specifying general categories of input mapping errors.
///
/// This list is intended to grow over time and it is not recommended to
/// exhaustively match against it.
///
/// It is used with the [`Error`] type.
///
/// [`Error`]: struct.Error.html
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum Input {
    /// The range of the requested file or bytes is invalid.
    InvalidRange,
}

impl Input {
    /// Returns a display message fragment describing the `Input` type.
    ///
    /// The result of `as_str` is used to `Display` the `Input`.
    ///
    /// # Examples
    ///
    /// ```
    /// use vmap::{Input, Operation};
    ///
    /// fn print_input(op: Operation, input: Input) {
    ///     println!("failed to {}, {}", op.as_str().unwrap(), input.as_str());
    /// }
    ///
    /// // Will print "failed to map file, invalid range"
    /// print_input(Operation::MapFile, Input::InvalidRange);
    /// ```
    pub fn as_str(&self) -> &'static str {
        match *self {
            Input::InvalidRange => "invalid range",
        }
    }
}

impl fmt::Display for Input {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str(self.as_str())
    }
}
