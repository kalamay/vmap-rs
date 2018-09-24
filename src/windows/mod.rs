extern crate winapi;

use super::{Protect, Flush};

use std::{ptr, mem};
use std::io::{Result, Error};
use std::fs::File;
use std::os::windows::io::{AsRawHandle, RawHandle};

use winapi::shared::basetsd::SIZE_T;
use winapi::shared::minwindef::DWORD;
use winapi::um::handleapi::{CloseHandle,INVALID_HANDLE_VALUE};
use winapi::um::memoryapi::{
    CreateFileMappingW, MapViewOfFile, MapViewOfFileEx,UnmapViewOfFile, VirtualProtect, FlushViewOfFile,
    FILE_MAP_READ, FILE_MAP_WRITE,
};
use winapi::um::winnt::{PAGE_READONLY, PAGE_READWRITE};

pub fn get_page_size() -> usize {
    unsafe {
        let mut info: SYSTEM_INFO = mem::uninitialized();
        GetSystemInfo(&mut info as LPSYSTEM_INFO);
        info.dwPageSize as usize
    }
}

pub unsafe fn map_file(file: &File, off: usize, len: usize, prot: Protect) -> Result<*mut u8> {
    let (prot, access) = match prot {
        Protect::ReadOnly => (PAGE_READONLY, FILE_MAP_READ),
        Protect::ReadWrite => (PAGE_READWRITE, FILE_MAP_READ|FILE_MAP_WRITE),
    };

    let map = CreateFileMappingW(file.as_raw_handle(), ptr::null_mut(),
                                 prot, 0, 0, ptr::null());
    if map.is_null() {
        Err(Error::last_os_error())
    }
    else {
        let pg = MapViewOfFile(map, acc,
                               (off >> 16 >> 16) as DWORD,
                               (off & 0xffffffff) as DWORD,
                               len as SIZE_T);
        CloseHandle(map);

        if pg.is_null() {
            Err(Error::last_os_error())
        } else {
            Ok(pg as *mut u8)
        }
    }
}

pub unsafe fn map_ring(len: usize) -> Result<*mut u8> {
    let full = (len * 2) as DWORD;
    let map = CreateFileMappingA(INVALID_HANDLE_VALUE,
                                 ptr::null_mut(),
                                 PAGE_READWRITE,
                                 full >> 32,
                                 full & 0xffffffff,
                                 ptr::null());
    if map == ptr::null_mut() {
        return Err(Error::last_os_error());
    }

    let a = MapViewOfFile(map, FILE_MAP_READ|FILE_MAP_WRITE, 0, 0, len);
    if a == ptr::null_mut() {
        let err = Err(Error::last_os_error());
        CloseHandle(map);
        return err;
    }

    let b = MapViewOfFileEx(map, FILE_MAP_READ|FILE_MAP_WRITE, 0, 0, len, a.offset(len));
    if b == ptr::null_mut() {
        let err = Err(Error::last_os_error());
        UnmapViewOfFile(a);
        CloseHandle(map);
        return err;
    }

    CloseHandle(map);
    Ok(a as *mut u8)
}

pub unsafe fn unmap(pg: *mut u8, _len: usize) -> Result<()> {
    if UnmapViewOfFile(pg) != 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

pub unsafe fn unmap_ring(pg: *mut u8, len: usize) {
    unmap(pg, len)?;
    unmap(pg.offset(len), len)?;
}

pub unsafe fn protect(pg: *mut u8, len: usize, prot: Protect) -> Result<()> {
    let prot = match prot {
        Protect::ReadOnly => PAGE_READONLY,
        Protect::ReadWrite => PAGE_READWRITE,
    };
    let mut old = 0;
    if VirtualProtect(pg, len, p, &mut old) != 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

pub unsafe fn flush(pg: *mut u8, len: usize, _mode: Flush) -> Result<()> {
    if FlushViewOfFile(pg, len as SIZE_T) != 0 {
        Err(Error::last_os_error())
    }
    else {
        Ok(())
    }
}

