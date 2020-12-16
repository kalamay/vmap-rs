use std::os::raw::c_int;

use crate::{Error, Operation, Result};

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn memfd_open() -> Result<c_int> {
    use std::os::raw::c_char;
    const NAME: &[u8] = b"vmap\0";
    let fd = unsafe {
        libc::syscall(
            libc::SYS_memfd_create,
            NAME.as_ptr() as *const c_char,
            libc::MFD_CLOEXEC,
        )
    };
    if fd < 0 {
        Err(Error::last_os_error(Operation::MemoryFd))
    } else {
        Ok(fd as c_int)
    }
}

#[cfg(target_os = "freebsd")]
pub fn memfd_open() -> Result<c_int> {
    let fd = unsafe { libc::shm_open(libc::SHM_ANON, libc::O_RDWR, 0o600) };
    if fd < 0 {
        Err(Error::last_os_error(Operation::MemoryFd))
    } else {
        Ok(fd as c_int)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "freebsd")))]
pub fn memfd_open() -> Result<c_int> {
    const OFLAGS: c_int = libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC;
    let mut path_bytes: [u8; 14] = *b"/vmap-XXXXXXX\0";

    for i in (0..10000000).cycle() {
        let path = {
            use std::io::Write;
            write!(&mut path_bytes[6..], "{:0>7}", i).unwrap();
            std::ffi::CStr::from_bytes_with_nul(&path_bytes).unwrap()
        };

        let fd = unsafe { libc::shm_open(path.as_ptr(), OFLAGS, 0o600) };
        if fd < 0 {
            let err = Error::last_os_error(Operation::MemoryFd);
            if err.raw_os_error() != Some(libc::EEXIST) {
                return Err(err);
            }
        } else {
            unsafe { libc::shm_unlink(path.as_ptr()) };
            return Ok(fd);
        }
    }
    unreachable!();
}
