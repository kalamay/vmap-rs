use crate::{AdviseAccess, AdviseUsage, Flush, Protect};
use std::os::windows::raw::HANDLE;

use std::fs::File;
use std::os::raw::c_void;
use std::os::windows::io::AsRawHandle;
use std::{mem, ptr};

use winapi::shared::basetsd::SIZE_T;
use winapi::shared::minwindef::DWORD;
use winapi::um::fileapi::FlushFileBuffers;
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::memoryapi::{
    CreateFileMappingW, FlushViewOfFile, MapViewOfFileEx, UnmapViewOfFile, VirtualAlloc,
    VirtualFree, VirtualLock, VirtualProtect, VirtualUnlock, FILE_MAP_COPY, FILE_MAP_READ,
    FILE_MAP_WRITE,
};
use winapi::um::sysinfoapi::{GetSystemInfo, LPSYSTEM_INFO, SYSTEM_INFO};
use winapi::um::winnt::{
    MEM_RELEASE, MEM_RESERVE, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY,
};

use crate::{Error, Operation, Result};

use self::Operation::*;

struct MapHandle {
    map: HANDLE,
}

impl MapHandle {
    pub unsafe fn new(op: Operation, file: HANDLE, prot: DWORD, len: usize) -> Result<Self> {
        let map = CreateFileMappingW(
            file,
            ptr::null_mut(),
            prot,
            (len >> 16 >> 16) as DWORD,
            (len & 0xffffffff) as DWORD,
            ptr::null(),
        );
        if map.is_null() {
            Err(Error::last_os_error(op))
        } else {
            Ok(Self { map })
        }
    }

    pub unsafe fn view_ptr(
        &self,
        access: DWORD,
        off: usize,
        len: usize,
        at: *mut c_void,
    ) -> *mut c_void {
        MapViewOfFileEx(
            self.map,
            access as DWORD,
            (off >> 16 >> 16) as DWORD,
            (off & 0xffffffff) as DWORD,
            len as SIZE_T,
            at,
        )
    }

    pub unsafe fn view(
        &self,
        op: Operation,
        access: DWORD,
        off: usize,
        len: usize,
        at: *mut c_void,
    ) -> Result<*mut u8> {
        let pg = self.view_ptr(access, off, len, at);
        if pg.is_null() {
            Err(Error::last_os_error(op))
        } else {
            Ok(pg as *mut u8)
        }
    }
}

impl Drop for MapHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.map);
        }
    }
}

/// Requests the page size and allocation granularity from the system.
pub fn system_info() -> (u32, u32) {
    let info = unsafe {
        let mut info = mem::MaybeUninit::<SYSTEM_INFO>::uninit();
        GetSystemInfo(info.as_mut_ptr() as LPSYSTEM_INFO);
        info.assume_init()
    };
    (info.dwPageSize, info.dwAllocationGranularity)
}

/// Memory maps a given range of a file.
pub fn map_file(file: &File, off: usize, len: usize, prot: Protect) -> Result<*mut u8> {
    let (prot, access) = match prot {
        Protect::ReadOnly => (PAGE_READONLY, FILE_MAP_READ),
        Protect::ReadWrite => (PAGE_READWRITE, FILE_MAP_READ | FILE_MAP_WRITE),
        Protect::ReadCopy => (PAGE_WRITECOPY, FILE_MAP_COPY),
    };

    unsafe {
        let map = MapHandle::new(MapFileHandle, file.as_raw_handle(), prot, 0)?;
        map.view(MapFileView, access, off, len, ptr::null_mut())
    }
}

/// Creates an anonymous allocation.
pub fn map_anon(len: usize, prot: Protect) -> Result<*mut u8> {
    let (prot, access) = match prot {
        Protect::ReadOnly => (PAGE_READONLY, FILE_MAP_READ),
        Protect::ReadWrite => (PAGE_READWRITE, FILE_MAP_READ | FILE_MAP_WRITE),
        Protect::ReadCopy => (PAGE_WRITECOPY, FILE_MAP_COPY),
    };

    unsafe {
        let map = MapHandle::new(MapAnonymousHandle, INVALID_HANDLE_VALUE, prot, len)?;
        map.view(MapAnonymousView, access, 0, len, ptr::null_mut())
    }
}

unsafe fn reserve(len: usize) -> Result<*mut c_void> {
    let pg = VirtualAlloc(ptr::null_mut(), len as SIZE_T, MEM_RESERVE, PAGE_NOACCESS);
    if pg.is_null() {
        Err(Error::last_os_error(RingAllocate))
    } else {
        VirtualFree(pg, 0, MEM_RELEASE);
        Ok(pg)
    }
}

#[cfg(feature = "io")]
unsafe fn map_ring_handle(map: &MapHandle, len: usize, pg: *mut c_void) -> Result<*mut u8> {
    let a = map.view(RingPrimary, FILE_MAP_READ | FILE_MAP_WRITE, 0, len, pg)?;
    let b = map.view(
        RingSecondary,
        FILE_MAP_READ | FILE_MAP_WRITE,
        0,
        len,
        pg.add(len),
    );
    if b.is_err() {
        UnmapViewOfFile(a as *mut c_void);
        b
    } else {
        Ok(a as *mut u8)
    }
}

/// Creates an anonymous circular allocation.
///
/// The length is the size of the sequential range, and the offset of
/// `len+1` refers to the same memory location at offset `0`. The circle
/// continues to up through the offset of `2*len - 1`.
#[cfg(feature = "io")]
pub fn map_ring(len: usize) -> Result<*mut u8> {
    let full = 2 * len;
    let map = unsafe { MapHandle::new(RingAllocate, INVALID_HANDLE_VALUE, PAGE_READWRITE, full)? };

    let mut n = 0;
    loop {
        let pg = unsafe { reserve(full)? };
        let rc = unsafe { map_ring_handle(&map, len, pg) };
        if rc.is_ok() || n == 5 {
            return rc;
        }
        n += 1;
    }
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
pub unsafe fn unmap(pg: *mut u8, _len: usize) -> Result<()> {
    if UnmapViewOfFile(pg as *mut c_void) != 0 {
        Err(Error::last_os_error(Unmap))
    } else {
        Ok(())
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
#[cfg(feature = "io")]
pub unsafe fn unmap_ring(pg: *mut u8, len: usize) -> Result<()> {
    if UnmapViewOfFile(pg.offset(len as isize) as *mut c_void) == 0 {
        Err(Error::last_os_error(RingDeallocate))
    } else {
        UnmapViewOfFile(pg as *mut c_void);
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
        Protect::ReadOnly => PAGE_READONLY,
        Protect::ReadWrite => PAGE_READWRITE,
        Protect::ReadCopy => PAGE_READWRITE,
    };
    let mut old = 0;
    if VirtualProtect(pg as *mut c_void, len, prot, &mut old) == 0 {
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
pub unsafe fn flush(pg: *mut u8, file: &File, len: usize, mode: Flush) -> Result<()> {
    if FlushViewOfFile(pg as *mut c_void, len as SIZE_T) == 0 {
        Err(Error::last_os_error(Flush))
    } else {
        match mode {
            Flush::Sync => {
                if FlushFileBuffers(file.as_raw_handle()) == 0 {
                    Err(Error::last_os_error(Flush))
                } else {
                    Ok(())
                }
            }
            Flush::Async => Ok(()),
        }
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
    _pg: *mut u8,
    _len: usize,
    _access: AdviseAccess,
    _usage: AdviseUsage,
) -> Result<()> {
    Ok(())
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
    if VirtualLock(pg as *mut c_void, len) == 0 {
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
    if VirtualUnlock(pg as *mut c_void, len) == 0 {
        Err(Error::last_os_error(Unlock))
    } else {
        Ok(())
    }
}
