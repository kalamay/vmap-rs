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
    use libc::c_char;
    use rand::distributions::Alphanumeric;
    use rand::{thread_rng, Rng};

    const OFLAGS: c_int = libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC;

    let mut path: [u8; 18] = *b"/tmp/vmap-XXXXXXX\0";

    let mut rng = thread_rng();
    let end = path.len() - 1;

    loop {
        for dst in &mut path[10..end] {
            *dst = rng.sample(&Alphanumeric) as u8;
        }

        let fd = unsafe { libc::shm_open(path.as_ptr() as *const c_char, OFLAGS, 0o600) };
        if fd < 0 {
            let err = Error::last_os_error(Operation::MemoryFd);
            if err.raw_os_error() != Some(libc::EEXIST) {
                return Err(err);
            }
        } else {
            unsafe { libc::shm_unlink(path.as_ptr() as *const c_char) };
            return Ok(fd);
        }
    }
}
