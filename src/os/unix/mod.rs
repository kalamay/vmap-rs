extern crate libc;

use ::{Protect, Flush};

use std::ptr;
use std::io::{Result, Error};
use std::fs::File;
use std::os::unix::io::{AsRawFd};

use self::libc::{
    c_void, off_t,
    mmap, munmap, mprotect, msync, sysconf,
    PROT_READ, PROT_WRITE, MAP_SHARED, MAP_PRIVATE, MAP_FAILED,
    MS_SYNC, MS_ASYNC, _SC_PAGESIZE
};

// For macOS and iOS we use the mach vm system for rings. The posix module
// does work correctly on these targets, but it necessitates an otherwise
// uneeded file descriptor.
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod mach;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use self::mach::{map_ring, unmap_ring};

// For non-mach targets load the POSIX version of the ring mapping functions.
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
mod posix;
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub use self::posix::{map_ring, unmap_ring};

/// Requests the page size from the system.
pub fn page_size() -> usize {
    unsafe { sysconf(_SC_PAGESIZE) as usize }
}

/// Memory maps a given range of a file.
pub unsafe fn map_file(file: &File, off: usize, len: usize, prot: Protect) -> Result<*mut u8> {
    let (prot, flags) = match prot {
        Protect::ReadOnly => (PROT_READ, MAP_SHARED),
        Protect::ReadWrite => (PROT_READ|PROT_WRITE, MAP_SHARED),
        Protect::ReadCopy => (PROT_READ|PROT_WRITE, MAP_PRIVATE),
    };
    let pg = mmap(ptr::null_mut(), len, prot, flags, file.as_raw_fd(), off as off_t);
    if pg == MAP_FAILED {
        Err(Error::last_os_error())
    }
    else {
        Ok(pg as *mut u8)
    }
}

/// Unmaps a page range from a previos mapping.
pub unsafe fn unmap(pg: *mut u8, len: usize) -> Result<()> {
    if munmap(pg as *mut c_void, len) < 0 {
        Err(Error::last_os_error())
    }
    else {
        Ok(())
    }
}

/// Changes the protection for a page range.
pub unsafe fn protect(pg: *mut u8, len: usize, prot: Protect) -> Result<()> {
    let prot = match prot {
        Protect::ReadOnly => PROT_READ,
        Protect::ReadWrite => PROT_READ|PROT_WRITE,
        Protect::ReadCopy => PROT_READ|PROT_WRITE,
    };
    if mprotect(pg as *mut c_void, len, prot) != 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Writes modified whole pages back to the filesystem.
pub unsafe fn flush(pg: *mut u8, _file: &File, len: usize, mode: Flush) -> Result<()> {
    let flags = match mode {
        Flush::Sync => MS_SYNC,
        Flush::Async => MS_ASYNC,
    };
    if msync(pg as *mut c_void, len, flags) < 0 {
        Err(Error::last_os_error())
    }
    else {
        Ok(())
    }
}

