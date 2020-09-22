//! Types for working with various map operation errors.

use std::os::raw::c_int;
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

/// A type for storing platform-specific kernel error codes.
///
/// This is *not* used libc or Windows errors (i.e. `errno` or
/// `GetLastError`). The `std::io::Error` holds the platform errors.
/// However, on macOS and iOS, some platform invocations use a different
/// error mechanism (i.e. `kern_return_t`). Currently, on other platforms
/// this isn't actually used by this library. However, to maintain cross-
/// platform use of the error, it is implemented across all platforms.
pub type KernelResult = c_int;

/// A list specifying general categories of map errors.
#[non_exhaustive]
pub struct Error {
    repr: Repr,
    op: Operation,
}

enum Repr {
    Io(io::Error),
    Input(Input),
    Kernel(kernel::Error),
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

    /// Returns an error that wraps a [`KernelResult`] along with an [`Operation`].
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
    /// [`KernelResult`]: type.KernelResult.html
    /// [`Operation`]: enum.Operation.html
    pub fn kernel(op: Operation, code: KernelResult) -> Self {
        Self {
            repr: Repr::Kernel(kernel::Error(code)),
            op,
        }
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
        Self::io(op, io::Error::last_os_error())
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
        if let Repr::Io(e) = &self.repr {
            e.raw_os_error()
        } else {
            None
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
            Repr::Kernel(ref err) => err.kind(),
            Repr::Input(_) => io::ErrorKind::InvalidInput,
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
            Repr::Kernel(_) => None,
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
            Repr::Kernel(ref err) => ("kernel", err as &dyn fmt::Debug),
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
            Repr::Kernel(ref err) => err as &dyn fmt::Display,
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
    /// The [`AdviseAccess`] or [`AdviseUsage`] could not be applied to the
    /// provided memory region.
    ///
    /// [`AdviseAccess`]: ../enum.AdviseAccess.html
    /// [`AdviseUsage`]: ../enum.AdviseUsage.html
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
    /// An unexpected `null` pointer was mapped.
    NullPtr,
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
            Input::NullPtr => "null pointer",
        }
    }
}

impl fmt::Display for Input {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str(self.as_str())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
mod kernel {
    use super::{fmt, io, KernelResult};

    pub struct Error(pub KernelResult);

    impl fmt::Debug for Error {
        fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(fmt, "\"{}\"", self)
        }
    }

    impl fmt::Display for Error {
        fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(fmt, "unexpected kernel error {}", self.0)
        }
    }

    impl Error {
        pub fn kind(&self) -> io::ErrorKind {
            io::ErrorKind::Other
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod kernel {
    use super::{fmt, io, KernelResult};
    use std::ffi::CStr;
    use std::os::raw::c_char;

    extern "C" {
        fn mach_error_string(code: KernelResult) -> *const c_char;
    }

    pub struct Error(pub KernelResult);

    impl fmt::Debug for Error {
        fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(fmt, "\"{}\"", self)
        }
    }

    impl fmt::Display for Error {
        fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
            let msg = unsafe { CStr::from_ptr(mach_error_string(self.0)) };
            match msg.to_str() {
                Err(err) => write!(fmt, "invalid kernel error {} ({})", self.0, err),
                Ok(val) => write!(fmt, "{} (kernel error {})", val, self.0),
            }
        }
    }

    impl Error {
        pub fn kind(&self) -> io::ErrorKind {
            match self.0 {
                KERN_INVALID_ADDRESS
                | KERN_INVALID_ARGUMENT
                | KERN_INVALID_CAPABILITY
                | KERN_INVALID_HOST
                | KERN_INVALID_LEDGER
                | KERN_INVALID_MEMORY_CONTROL
                | KERN_INVALID_NAME
                | KERN_INVALID_OBJECT
                | KERN_INVALID_POLICY
                | KERN_INVALID_PROCESSOR_SET
                | KERN_INVALID_RIGHT
                | KERN_INVALID_SECURITY
                | KERN_INVALID_TASK
                | KERN_INVALID_VALUE => io::ErrorKind::InvalidInput,
                _ => io::ErrorKind::Other,
            }
        }
    }

    //pub const KERN_SUCCESS: KernelResult = 0;
    pub const KERN_INVALID_ADDRESS: KernelResult = 1;
    //pub const KERN_PROTECTION_FAILURE: KernelResult = 2;
    //pub const KERN_NO_SPACE: KernelResult = 3;
    pub const KERN_INVALID_ARGUMENT: KernelResult = 4;
    //pub const KERN_FAILURE: KernelResult = 5;
    //pub const KERN_RESOURCE_SHORTAGE: KernelResult = 6;
    //pub const KERN_NOT_RECEIVER: KernelResult = 7;
    //pub const KERN_NO_ACCESS: KernelResult = 8;
    //pub const KERN_MEMORY_FAILURE: KernelResult = 9;
    //pub const KERN_MEMORY_ERROR: KernelResult = 10;
    //pub const KERN_ALREADY_IN_SET: KernelResult = 11;
    //pub const KERN_NOT_IN_SET: KernelResult = 12;
    //pub const KERN_NAME_EXISTS: KernelResult = 13;
    //pub const KERN_ABORTED: KernelResult = 14;
    pub const KERN_INVALID_NAME: KernelResult = 15;
    pub const KERN_INVALID_TASK: KernelResult = 16;
    pub const KERN_INVALID_RIGHT: KernelResult = 17;
    pub const KERN_INVALID_VALUE: KernelResult = 18;
    //pub const KERN_UREFS_OVERFLOW: KernelResult = 19;
    pub const KERN_INVALID_CAPABILITY: KernelResult = 20;
    //pub const KERN_RIGHT_EXISTS: KernelResult = 21;
    pub const KERN_INVALID_HOST: KernelResult = 22;
    //pub const KERN_MEMORY_PRESENT: KernelResult = 23;
    //pub const KERN_MEMORY_DATA_MOVED: KernelResult = 24;
    //pub const KERN_MEMORY_RESTART_COPY: KernelResult = 25;
    pub const KERN_INVALID_PROCESSOR_SET: KernelResult = 26;
    //pub const KERN_POLICY_LIMIT: KernelResult = 27;
    pub const KERN_INVALID_POLICY: KernelResult = 28;
    pub const KERN_INVALID_OBJECT: KernelResult = 29;
    //pub const KERN_ALREADY_WAITING: KernelResult = 30;
    //pub const KERN_DEFAULT_SET: KernelResult = 31;
    //pub const KERN_EXCEPTION_PROTECTED: KernelResult = 32;
    pub const KERN_INVALID_LEDGER: KernelResult = 33;
    pub const KERN_INVALID_MEMORY_CONTROL: KernelResult = 34;
    pub const KERN_INVALID_SECURITY: KernelResult = 35;
    //pub const KERN_NOT_DEPRESSED: KernelResult = 36;
    //pub const KERN_TERMINATED: KernelResult = 37;
    //pub const KERN_LOCK_SET_DESTROYED: KernelResult = 38;
    //pub const KERN_LOCK_UNSTABLE: KernelResult = 39;
    //pub const KERN_LOCK_OWNED: KernelResult = 40;
    //pub const KERN_LOCK_OWNED_SELF: KernelResult = 41;
    //pub const KERN_SEMAPHORE_DESTROYED: KernelResult = 42;
    //pub const KERN_RPC_SERVER_TERMINATED: KernelResult = 43;
    //pub const KERN_RPC_TERMINATE_ORPHAN: KernelResult = 44;
    //pub const KERN_RPC_CONTINUE_ORPHAN: KernelResult = 45;
    //pub const KERN_NOT_SUPPORTED: KernelResult = 46;
    //pub const KERN_NODE_DOWN: KernelResult = 47;
    //pub const KERN_NOT_WAITING: KernelResult = 48;
    //pub const KERN_OPERATION_TIMED_OUT: KernelResult = 49;
    //pub const KERN_CODESIGN_ERROR: KernelResult = 50;
    //pub const KERN_POLICY_STATIC: KernelResult = 51;
    //pub const KERN_RETURN_MAX: KernelResult = 0x100;
}
