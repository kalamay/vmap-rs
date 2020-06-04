extern crate libc;

use std::io::{Error, Result};
use std::os::raw::c_int;

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
        Err(Error::last_os_error())
    } else {
        Ok(fd as c_int)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
extern crate rand;

#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn memfd_open() -> Result<c_int> {
    use self::rand::distributions::Alphanumeric;
    use self::rand::{thread_rng, Rng};

    const OFLAGS: c_int = libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC;

    // There *must* be a better way to do this...
    let mut path: [i8; 18] = [
        0x2f, 0x74, 0x6d, 0x70, 0x2f, 0x76, 0x6d, 0x61, 0x70, 0x2d, // "/tmp/vmap-"
        0x58, 0x58, 0x58, 0x58, 0x58, 0x58, 0x58, 0x00, // "XXXXXXX\0"
    ];

    let mut rng = thread_rng();
    let end = path.len() - 1;

    loop {
        for dst in &mut path[10..end] {
            *dst = rng.sample(&Alphanumeric) as i8;
        }

        let fd = unsafe { libc::shm_open(path.as_ptr(), OFLAGS, 0600) };
        if fd < 0 {
            let err = Error::last_os_error();
            if err.raw_os_error() != Some(libc::EEXIST) {
                return Err(err);
            }
        } else {
            unsafe { libc::shm_unlink(path.as_ptr()) };
            return Ok(fd);
        }
    }
}
