extern crate winapi;

use std::os::windows::raw::HANDLE;
use ::{Protect, Flush};

use std::{ptr, mem};
use std::io::{Result, Error};
use std::fs::File;
use std::os::raw::c_void;
use std::os::windows::io::AsRawHandle;

use self::winapi::shared::minwindef::DWORD;
use self::winapi::shared::basetsd::SIZE_T;
use self::winapi::um::winnt::{
    MEM_RESERVE, MEM_RELEASE, PAGE_NOACCESS,
    PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY
};
use self::winapi::um::sysinfoapi::{GetSystemInfo, SYSTEM_INFO, LPSYSTEM_INFO};
use self::winapi::um::fileapi::FlushFileBuffers;
use self::winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use self::winapi::um::memoryapi::{
    CreateFileMappingW, MapViewOfFileEx, UnmapViewOfFile, FlushViewOfFile,
    VirtualAlloc, VirtualFree, VirtualProtect,
    FILE_MAP_READ, FILE_MAP_WRITE, FILE_MAP_COPY
};

struct MapHandle {
    map: HANDLE
}

impl MapHandle {
    pub unsafe fn new(file: HANDLE, prot: DWORD, len: usize) -> Result<Self> {
        let map = CreateFileMappingW(file,
                                     ptr::null_mut(),
                                     prot,
                                     (len >> 32) as DWORD,
                                     (len & 0xffffffff) as DWORD,
                                     ptr::null());
        if map.is_null() {
            Err(Error::last_os_error())
        } else {
            Ok(Self { map })
        }
    }

    pub unsafe fn view_ptr(&self, access: DWORD, off: usize, len: usize, at: *mut c_void) -> *mut c_void {
        MapViewOfFileEx(self.map,
                        access as DWORD,
                        (off >> 32) as DWORD,
                        (off & 0xffffffff) as DWORD,
                        len as SIZE_T,
                        at)
    }

    pub unsafe fn view(&self, access: DWORD, off: usize, len: usize, at: *mut c_void) -> Result<*mut u8> {
        let pg = self.view_ptr(access, off, len, at);
        if pg.is_null() {
            Err(Error::last_os_error())
        } else {
            Ok(pg as *mut u8)
        }
    }
}

impl Drop for MapHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.map); }
    }
}

/// Requests the page size from the system.
pub fn page_size() -> usize {
    unsafe {
        let mut info: SYSTEM_INFO = mem::uninitialized();
        GetSystemInfo(&mut info as LPSYSTEM_INFO);
        info.dwPageSize
    }
}

/// Requests the allocation granularity from the system.
pub fn allocation_size() -> usize {
    unsafe {
        let mut info: SYSTEM_INFO = mem::uninitialized();
        GetSystemInfo(&mut info as LPSYSTEM_INFO);
        info.dwAllocationGranularity as usize
    }
}

/// Memory maps a given range of a file.
pub unsafe fn map_file(file: &File, off: usize, len: usize, prot: Protect) -> Result<*mut u8> {
    let (prot, access) = match prot {
        Protect::ReadOnly => (PAGE_READONLY, FILE_MAP_READ),
        Protect::ReadWrite => (PAGE_READWRITE, FILE_MAP_READ|FILE_MAP_WRITE),
        Protect::ReadCopy => (PAGE_READWRITE|PAGE_WRITECOPY, FILE_MAP_READ|FILE_MAP_COPY),
    };

    let map = MapHandle::new(file.as_raw_handle(), prot, 0)?;
    map.view(access, off, len, ptr::null_mut())
}

/// Creates an anonymous allocation.
pub unsafe fn map_anon(len: usize) -> Result<*mut u8> {
    let map = MapHandle::new(INVALID_HANDLE_VALUE, PAGE_READWRITE, len)?;
    map.view(FILE_MAP_READ|FILE_MAP_WRITE, 0, len, ptr::null_mut())
}

unsafe fn reserve(len: usize) -> Result<*mut c_void> {
    let pg = VirtualAlloc(ptr::null_mut(), len as SIZE_T, MEM_RESERVE, PAGE_NOACCESS);
    if pg.is_null() {
        Err(Error::last_os_error())
    } else {
        VirtualFree(pg, 0, MEM_RELEASE);
        Ok(pg)
    }
}

unsafe fn map_ring_handle(map: &MapHandle, len: usize, pg: *mut c_void) -> Result<*mut u8> {
    let a = map.view(FILE_MAP_READ|FILE_MAP_WRITE, 0, len, pg)?;
    let b = map.view(FILE_MAP_READ|FILE_MAP_WRITE, 0, len, pg.offset(len as isize));
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
pub unsafe fn map_ring(len: usize) -> Result<*mut u8> {
    let full = 2 * len;
    let map = MapHandle::new(INVALID_HANDLE_VALUE, PAGE_READWRITE, full)?;

    let mut n = 0;
    loop {
        let pg = reserve(full)?;
        let rc = map_ring_handle(&map, len, pg);
        if rc.is_ok() || n == 5 {
            return rc;
        }
        n += 1;
    }
}

/// Unmaps a page range from a previos mapping.
pub unsafe fn unmap(pg: *mut u8, _len: usize) -> Result<()> {
    if UnmapViewOfFile(pg as *mut c_void) != 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Unmaps a ring mapping created by `map_ring`.
pub unsafe fn unmap_ring(pg: *mut u8, len: usize) -> Result<()> {
    if UnmapViewOfFile(pg.offset(len as isize) as *mut c_void) == 0 {
        Err(Error::last_os_error())
    } else {
        UnmapViewOfFile(pg as *mut c_void);
        Ok(())
    }
}

/// Changes the protection for a page range.
pub unsafe fn protect(pg: *mut u8, len: usize, prot: Protect) -> Result<()> {
    let prot = match prot {
        Protect::ReadOnly => PAGE_READONLY,
        Protect::ReadWrite => PAGE_READWRITE,
        Protect::ReadCopy => PAGE_READWRITE,
    };
    let mut old = 0;
    if VirtualProtect(pg as *mut c_void, len, prot, &mut old) == 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Writes modified whole pages back to the filesystem.
pub unsafe fn flush(pg: *mut u8, file: &File, len: usize, mode: Flush) -> Result<()> {
    if FlushViewOfFile(pg as *mut c_void, len as SIZE_T) == 0 {
        return Err(Error::last_os_error())
    }
    match mode {
        Flush::Sync => if FlushFileBuffers(file.as_raw_handle()) == 0 {
            Err(Error::last_os_error())
        } else {
            Ok(())
        }
        Flush::Async => Ok(())
    }
}

