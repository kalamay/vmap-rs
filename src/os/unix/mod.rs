use crate::{AdviseAccess, AdviseUsage, Flush, Protect};

use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::ptr;

use libc::{
    c_void, madvise, mlock, mmap, mprotect, msync, munlock, munmap, off_t, sysconf, MADV_DONTNEED,
    MADV_NORMAL, MADV_RANDOM, MADV_SEQUENTIAL, MADV_WILLNEED, MAP_ANON, MAP_FAILED, MAP_PRIVATE,
    MAP_SHARED, MS_ASYNC, MS_SYNC, PROT_EXEC, PROT_READ, PROT_WRITE, _SC_PAGESIZE,
};

use crate::{Error, Operation, Result};

use self::Operation::*;

// For macOS and iOS we use the mach vm system for rings. The posix module
// does work correctly on these targets, but it necessitates an otherwise
// uneeded file descriptor.
#[cfg(all(feature = "io", any(target_os = "macos", target_os = "ios")))]
mod mach;
#[cfg(all(feature = "io", any(target_os = "macos", target_os = "ios")))]
pub use self::mach::{map_ring, unmap_ring};

// For non-mach targets load the POSIX version of the ring mapping functions.
#[cfg(all(feature = "io", not(any(target_os = "macos", target_os = "ios"))))]
mod posix;
#[cfg(all(feature = "io", not(any(target_os = "macos", target_os = "ios"))))]
pub use self::posix::{map_ring, unmap_ring};

/// Requests the page size and allocation granularity from the system.
pub fn system_info() -> (u32, u32) {
    let size = unsafe { sysconf(_SC_PAGESIZE) as u32 };
    (size, size)
}

fn result(op: Operation, pg: *mut c_void) -> Result<*mut u8> {
    if pg == MAP_FAILED {
        Err(Error::last_os_error(op))
    } else {
        Ok(pg as *mut u8)
    }
}

/// Memory maps a given range of a file.
pub fn map_file(file: &File, off: usize, len: usize, prot: Protect) -> Result<*mut u8> {
    let (prot, flags) = match prot {
        Protect::ReadOnly => (PROT_READ, MAP_SHARED),
        Protect::ReadWrite => (PROT_READ | PROT_WRITE, MAP_SHARED),
        Protect::ReadCopy => (PROT_READ | PROT_WRITE, MAP_PRIVATE),
        Protect::ReadExec => (PROT_READ | PROT_EXEC, MAP_PRIVATE),
    };
    unsafe {
        result(
            MapFile,
            mmap(
                ptr::null_mut(),
                len,
                prot,
                flags,
                file.as_raw_fd(),
                off as off_t,
            ),
        )
    }
}

/// Creates an anonymous allocation.
pub fn map_anon(len: usize, prot: Protect) -> Result<*mut u8> {
    let (prot, flags) = match prot {
        Protect::ReadOnly => (PROT_READ, MAP_SHARED),
        Protect::ReadWrite => (PROT_READ | PROT_WRITE, MAP_ANON | MAP_SHARED),
        Protect::ReadCopy => (PROT_READ | PROT_WRITE, MAP_ANON | MAP_PRIVATE),
        Protect::ReadExec => (PROT_READ | PROT_EXEC, MAP_ANON | MAP_PRIVATE),
    };
    unsafe { result(MapAnonymous, mmap(ptr::null_mut(), len, prot, flags, -1, 0)) }
}

/// Unmaps a page range from a previos mapping.
///
/// # Safety
///
/// This does not know or care if `pg` or `len` are valid. That is,
/// it may be null, not at a proper page boundary, point to a size
/// different from `len`, or worse yet, point to a properly mapped
/// pointer from some other allocation system.
///
/// Generally don't use this unless you are entirely sure you are
/// doing so correctly.
pub unsafe fn unmap(pg: *mut u8, len: usize) -> Result<()> {
    if munmap(pg as *mut c_void, len) < 0 {
        Err(Error::last_os_error(Unmap))
    } else {
        Ok(())
    }
}

/// Changes the protection for a page range.
///
/// # Safety
///
/// This does not know or care if `pg` or `len` are valid. That is,
/// it may be null, not at a proper page boundary, point to a size
/// different from `len`, or worse yet, point to a properly mapped
/// pointer from some other allocation system.
///
/// Generally don't use this unless you are entirely sure you are
/// doing so correctly.
pub unsafe fn protect(pg: *mut u8, len: usize, prot: Protect) -> Result<()> {
    let prot = match prot {
        Protect::ReadOnly => PROT_READ,
        Protect::ReadWrite => PROT_READ | PROT_WRITE,
        Protect::ReadCopy => PROT_READ | PROT_WRITE,
        Protect::ReadExec => PROT_READ | PROT_EXEC,
    };
    if mprotect(pg as *mut c_void, len, prot) != 0 {
        Err(Error::last_os_error(Protect))
    } else {
        Ok(())
    }
}

/// Writes modified whole pages back to the filesystem.
///
/// # Safety
///
/// This does not know or care if `pg` or `len` are valid. That is,
/// it may be null, not at a proper page boundary, point to a size
/// different from `len`, or worse yet, point to a properly mapped
/// pointer from some other allocation system.
///
/// Generally don't use this unless you are entirely sure you are
/// doing so correctly.
pub unsafe fn flush(pg: *mut u8, _file: &File, len: usize, mode: Flush) -> Result<()> {
    let flags = match mode {
        Flush::Sync => MS_SYNC,
        Flush::Async => MS_ASYNC,
    };
    if msync(pg as *mut c_void, len, flags) < 0 {
        Err(Error::last_os_error(Flush))
    } else {
        Ok(())
    }
}

/// Updates the advise for the page range.
///
/// # Safety
///
/// This does not know or care if `pg` or `len` are valid. That is,
/// it may be null, not at a proper page boundary, point to a size
/// different from `len`, or worse yet, point to a properly mapped
/// pointer from some other allocation system.
///
/// Generally don't use this unless you are entirely sure you are
/// doing so correctly.
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
        Err(Error::last_os_error(Advise))
    } else {
        Ok(())
    }
}

/// Locks physical pages into memory.
///
/// # Safety
///
/// This does not know or care if `pg` or `len` are valid. That is,
/// it may be null, not at a proper page boundary, point to a size
/// different from `len`, or worse yet, point to a properly mapped
/// pointer from some other allocation system.
///
/// Generally don't use this unless you are entirely sure you are
/// doing so correctly.
pub unsafe fn lock(pg: *mut u8, len: usize) -> Result<()> {
    if mlock(pg as *mut c_void, len) < 0 {
        Err(Error::last_os_error(Lock))
    } else {
        Ok(())
    }
}

/// Unlocks physical pages from memory.
///
/// # Safety
///
/// This does not know or care if `pg` or `len` are valid. That is,
/// it may be null, not at a proper page boundary, point to a size
/// different from `len`, or worse yet, point to a properly mapped
/// pointer from some other allocation system.
///
/// Generally don't use this unless you are entirely sure you are
/// doing so correctly.
pub unsafe fn unlock(pg: *mut u8, len: usize) -> Result<()> {
    if munlock(pg as *mut c_void, len) < 0 {
        Err(Error::last_os_error(Unlock))
    } else {
        Ok(())
    }
}
