mod memfd;
use self::memfd::memfd_open;

use std::os::raw::c_int;
use std::ptr;

use libc::{
    c_void, close, ftruncate, mmap, off_t, MAP_ANON, MAP_FAILED, MAP_FIXED, MAP_PRIVATE,
    MAP_SHARED, PROT_READ, PROT_WRITE,
};

use super::unmap;
use crate::{Error, Operation, Result};

use self::Operation::*;

/// Creates an anonymous circular allocation.
///
/// The length is the size of the sequential range, and the offset of
/// `len+1` refers to the same memory location at offset `0`. The circle
/// continues to up through the offset of `2*len - 1`.
pub fn map_ring(len: usize) -> Result<*mut u8> {
    // Create a temporary file descriptor truncated to the ring size.
    let fd = tmp_open(len)?;
    let ret = wrap_fd(len, fd);
    unsafe {
        close(fd);
    }
    ret
}

fn wrap_fd(len: usize, fd: c_int) -> Result<*mut u8> {
    // Map anoymous into an initial address that will cover the duplicate
    // address range.
    let pg = map(
        RingAllocate,
        ptr::null_mut(),
        len * 2,
        MAP_PRIVATE | MAP_ANON,
        -1,
    )?;
    match wrap_ptr(pg, len, fd) {
        Err(err) => unsafe {
            unmap_ring(pg, len).unwrap_or_default();
            Err(err)
        },
        Ok(pg) => Ok(pg),
    }
}

fn wrap_ptr(pg: *mut u8, len: usize, fd: c_int) -> Result<*mut u8> {
    // Map the two halves of the buffer into adjacent adresses that use the
    // same file descriptor offset.
    map(RingPrimary, pg, len, MAP_SHARED | MAP_FIXED, fd)?;
    map(
        RingSecondary,
        unsafe { pg.add(len) },
        len,
        MAP_SHARED | MAP_FIXED,
        fd,
    )?;
    Ok(pg)
}

fn map(op: Operation, pg: *mut u8, len: usize, flags: c_int, fd: c_int) -> Result<*mut u8> {
    unsafe {
        let pg = mmap(pg as *mut c_void, len, PROT_READ | PROT_WRITE, flags, fd, 0);
        if pg == MAP_FAILED {
            Err(Error::last_os_error(op))
        } else {
            Ok(pg as *mut u8)
        }
    }
}

/// Unmaps a ring mapping created by `map_ring`.
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
pub unsafe fn unmap_ring(pg: *mut u8, len: usize) -> Result<()> {
    unmap(pg, 2 * len)
}

fn tmp_open(size: usize) -> Result<c_int> {
    let fd = memfd_open()?;
    if unsafe { ftruncate(fd, size as off_t) } < 0 {
        let err = Error::last_os_error(Operation::MemoryFd);
        unsafe {
            close(fd);
        }
        Err(err)
    } else {
        Ok(fd)
    }
}
