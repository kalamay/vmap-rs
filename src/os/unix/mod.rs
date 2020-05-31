extern crate libc;

use crate::{AdviseAccess, AdviseUsage, Flush, Protect};

use std::fs::File;
use std::io::{Error, Result};
use std::os::unix::io::AsRawFd;
use std::ptr;

use self::libc::{
    c_void, madvise, mlock, mmap, mprotect, msync, munlock, munmap, off_t, sysconf, MADV_DONTNEED,
    MADV_NORMAL, MADV_RANDOM, MADV_SEQUENTIAL, MADV_WILLNEED, MAP_ANON, MAP_FAILED, MAP_PRIVATE,
    MAP_SHARED, MS_ASYNC, MS_SYNC, PROT_READ, PROT_WRITE, _SC_PAGESIZE,
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

/// Requests the allocation granularity from the system.
pub fn allocation_size() -> usize {
    page_size()
}

fn result(pg: *mut c_void) -> Result<*mut u8> {
    if pg == MAP_FAILED {
        Err(Error::last_os_error())
    } else {
        Ok(pg as *mut u8)
    }
}

/// Memory maps a given range of a file.
pub unsafe fn map_file(file: &File, off: usize, len: usize, prot: Protect) -> Result<*mut u8> {
    let (prot, flags) = match prot {
        Protect::ReadOnly => (PROT_READ, MAP_SHARED),
        Protect::ReadWrite => (PROT_READ | PROT_WRITE, MAP_SHARED),
        Protect::ReadCopy => (PROT_READ | PROT_WRITE, MAP_PRIVATE),
    };
    result(mmap(
        ptr::null_mut(),
        len,
        prot,
        flags,
        file.as_raw_fd(),
        off as off_t,
    ))
}

/// Creates an anonymous allocation.
pub unsafe fn map_anon(len: usize) -> Result<*mut u8> {
    result(mmap(
        ptr::null_mut(),
        len,
        PROT_READ | PROT_WRITE,
        MAP_ANON | MAP_PRIVATE,
        -1,
        0,
    ))
}

/// Unmaps a page range from a previos mapping.
pub unsafe fn unmap(pg: *mut u8, len: usize) -> Result<()> {
    if munmap(pg as *mut c_void, len) < 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Changes the protection for a page range.
pub unsafe fn protect(pg: *mut u8, len: usize, prot: Protect) -> Result<()> {
    let prot = match prot {
        Protect::ReadOnly => PROT_READ,
        Protect::ReadWrite => PROT_READ | PROT_WRITE,
        Protect::ReadCopy => PROT_READ | PROT_WRITE,
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
    } else {
        Ok(())
    }
}

/// Updates the advise for the page range.
pub unsafe fn advise(
    pg: *mut u8,
    len: usize,
    access: AdviseAccess,
    usage: AdviseUsage,
) -> Result<()> {
    let adv = match access {
        AdviseAccess::Normal => MADV_NORMAL,
        AdviseAccess::Sequential => MADV_SEQUENTIAL,
        AdviseAccess::Random => MADV_RANDOM,
    } | match usage {
        AdviseUsage::Normal => 0,
        AdviseUsage::WillNeed => MADV_WILLNEED,
        AdviseUsage::WillNotNeed => MADV_DONTNEED,
    };

    if madvise(pg as *mut c_void, len, adv) < 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Locks physical pages into memory.
pub unsafe fn lock(pg: *mut u8, len: usize) -> Result<()> {
    if mlock(pg as *mut c_void, len) < 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Unlocks physical pages from memory.
pub unsafe fn unlock(pg: *mut u8, len: usize) -> Result<()> {
    if munlock(pg as *mut c_void, len) < 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}
