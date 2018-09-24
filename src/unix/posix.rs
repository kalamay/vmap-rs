extern crate libc;

use std::os::raw::{c_int,c_char};

use self::libc::{
    off_t,
    mmap, munmap, ftruncate, close,
    PROT_READ, PROT_WRITE, MAP_SHARED, MAP_FAILED, MS_SYNC, MS_ASYNC, _SC_PAGESIZE
};

pub fn map_ring(len: usize) -> Result<*mut u8> {
    // Create a temporary file descriptor truncated to the ring size.
    let fd = open_tmp()?;
    let ret = wrap_fd(len, fd);
    close(fd);
    ret
}

fn wrap_fd(len: usize, fd: c_int) -> Result<*mut u8> {
    // Map anoymous into an initial address that will cover the duplicate
    // address range.
    let pg = map(ptr::null_mut(), len*2, MAP_PRIVATE|MAP_ANON, -1)?;
    match wrap_ptr(pg, len, fd) {
        Err(err) => { unmap_ring(pg, len); Err(err) },
        Ok(pg)
    }
}

fn wrap_ptr(pg: *mut u8, len: usize, fd: c_int) -> Result<*mut u8> {
    // Map the two halves of the buffer into adjacent adresses that use the
    // same file descriptor offset.
    map(pg, len, MAP_SHARED|MAP_FIXED, fd)?;
    map(unsafe { pg.offset(len as isize) }, len, MAP_SHARED|MAP_FIXED, fd)?;
    Ok(pg)
}

fn map(pg: *mut u8, len: usize, flags: c_int, fd: c_int) -> Result<*mut u8> {
    unsafe {
        let pg = mmap(pg as *mut c_void, len, PROT_READ|PROT_WRITE, flags, fd, 0);
        if pg == MAP_FAILED {
            Err(Error::last_os_error())
        }
        else {
            Ok(pg as *mut u8)
        }
    }
}

pub fn unmap_ring(pg: *mut u8, len: usize) -> Result<()> {
    unmap(pg, 2*len)
}

fn open_tmp(size: usize) -> Result<c_int> {
    let fd = open_tmp_fd()?
    unsafe {
        if ftruncate(fd, size as off_t) < 0 {
            let err = Error::last_os_error();
            close(fd);
            Err(err)
        }
        else {
            Ok(fd)
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn open_tmp_fd() -> Result<c_int> {
    const NAME : &[u8] = b"vmap";
    let fd = unsafe {
        libc::syscall(libc::SYS_memfd_create,
                      NAME.as_ptr() as *const c_char,
                      libc::MFD_CLOEXEC)
    };
    if fd < 0 {
        Err(Error::last_os_error())
    }
    else {
        Ok(fd as c_int)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn open_tmp_fd() -> Result<c_int> {
    const OFLAGS = libc::O_RDWR|libc::O_CREAT|libc::O_EXCL|libc::O_CLOEXEC;

    // There *must* be a better way to do this...
    let mut path : [i8; 18] = [
        0x2f,0x74,0x6d,0x70,0x2f,0x76,0x6d,0x61,0x70, // "/tmp/vmap"
        0x2d,0x58,0x58,0x58,0x58,0x58,0x58,0x58,0x00, // "-XXXXXXX\0"
    ];

    loop {
        unsafe {
            libc::mktemp((&mut path).as_mut_ptr());

            let fd = libc::shm_open(path, OFLAGS, 0600);
            if (fd < 0) {
                let err = Error::last_os_error()
                if (err.raw_os_error() == libc::EEXIST) { continue; }
                return Err(err)
            }

            libc::shm_unlink(path);
            return Ok(fd);
        }
    }
}

