#![allow(non_camel_case_types)]

extern crate libc;

use std::os::raw::{c_int,c_uint, c_char};
use std::io::{Result,Error,ErrorKind};
use std::{fmt, error};
use std::ffi::CStr;

use self::libc::uintptr_t;

type kern_return_t = c_int;
type vm_offset_t = uintptr_t;
type vm_size_t = uintptr_t;
type mach_port_t = c_uint;
type vm_map_t = mach_port_t;
type vm_address_t = vm_offset_t;
type vm_prot_t = c_int;
type mem_entry_name_port_t = mach_port_t;
type vm_inherit_t = c_uint;
type boolean_t = bool;

const KERN_SUCCESS : kern_return_t = 0;

const VM_PROT_READ : vm_prot_t = 0x01;
const VM_PROT_WRITE : vm_prot_t = 0x02;
//const VM_PROT_EXECUTE : vm_prot_t = 0x04;
const VM_PROT_DEFAULT : vm_prot_t = VM_PROT_READ | VM_PROT_WRITE;

const VM_FLAGS_FIXED : c_int = 0x0000;
const VM_FLAGS_ANYWHERE : c_int = 0x0001;
//const VM_FLAGS_PURGABLE : c_int = 0x0002;
//const VM_FLAGS_RANDOM_ADDR : c_int = 0x0008;
//const VM_FLAGS_NO_CACHE : c_int = 0x0010;
//const VM_FLAGS_RESILIENT_CODESIGN : c_int = 0x0020;
//const VM_FLAGS_RESILIENT_MEDIA : c_int = 0x0040;
const VM_FLAGS_OVERWRITE : c_int = 0x4000;

//const VM_INHERIT_SHARE : vm_inherit_t = 0;
//const VM_INHERIT_COPY : vm_inherit_t = 1;
const VM_INHERIT_NONE : vm_inherit_t = 2;
//const VM_INHERIT_DONATE_COPY : vm_inherit_t = 3;

extern {
    fn mach_error_string(code: kern_return_t) -> *const c_char;

    fn mach_task_self() -> mach_port_t;

    fn vm_allocate(
        target_task: vm_map_t,
        address: *mut vm_address_t,
        size: vm_size_t,
        flags: c_int) -> kern_return_t;

    fn vm_deallocate(
        target_task: vm_map_t,
        address: vm_address_t,
        size: vm_size_t) -> kern_return_t;

    fn vm_map(
        target_task: vm_map_t,
        address: *mut vm_address_t,
        size: vm_size_t,
        mask: vm_address_t,
        flags: c_int,
        object: mem_entry_name_port_t,
        offset: vm_offset_t,
        copy: boolean_t,
        cur_protection: vm_prot_t,
        max_protection: vm_prot_t,
        inheritance: vm_inherit_t) -> kern_return_t;

    /*
    fn vm_protect(
        target_task: vm_map_t,
        address: vm_address_t,
        size: vm_size_t,
        set_maximum: boolean_t,
        new_protection: vm_prot_t) -> kern_return_t;
    */

    fn mach_make_memory_entry(
        target_task: vm_map_t,
        size: *mut vm_size_t,
        offset: vm_offset_t,
        permission: vm_prot_t,
        object_handle: *mut mem_entry_name_port_t,
        parent_entry: mem_entry_name_port_t) -> kern_return_t;
}

#[derive(Debug)]
pub struct MachError {
    code: kern_return_t,
    msg: &'static str
}

impl MachError {
    pub fn new(code: kern_return_t, msg: &'static str) -> Self {
        Self { code: code, msg: msg }
    }

    /*
    pub fn get_system(&self) -> c_int {
        (self.code >> 26) & 0x3f
    }

    pub fn get_subsystem(&self) -> c_int {
        (self.code >> 14) & 0xfff
    }

    pub fn get_code(&self) -> c_int {
        self.code & 0x3fff
    }
    */
}

impl error::Error for MachError {
    fn description(&self) -> &str { "mach error" }
    fn cause(&self) -> Option<&error::Error> { None }
}

impl fmt::Display for MachError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        unsafe {
            let msg = CStr::from_ptr(mach_error_string(self.code));
            match msg.to_str() {
                Err(ec) => write!(fmt, "{} (invalid error message: {})", self.msg, ec),
                Ok(val) => write!(fmt, "{}: {}", self.msg, val),
            }
        }
    }
}

pub fn map_ring(len: usize) -> Result<*mut u8> {
    unsafe {
        let port = mach_task_self();
        let mut addr : vm_address_t = 0;
        let mut len = len as vm_size_t;
        let mut map_port : mem_entry_name_port_t = 0;

        let ret = vm_allocate(port, &mut addr, 2*len, VM_FLAGS_ANYWHERE);
        if ret != KERN_SUCCESS {
            return Err(Error::new(ErrorKind::Other,
                                  MachError::new(ret, "failed to allocate full region")));
        }

        let ret = vm_allocate(port, &mut addr, len, VM_FLAGS_FIXED | VM_FLAGS_OVERWRITE);
        if ret != KERN_SUCCESS {
            vm_deallocate(port, addr, 2*len);
            return Err(Error::new(ErrorKind::Other,
                                  MachError::new(ret, "failed to allocate first half")));
        }

        let ret = mach_make_memory_entry(port, &mut len, addr, VM_PROT_DEFAULT, &mut map_port, 0);
        if ret != KERN_SUCCESS {
            vm_deallocate(port, addr, 2*len);
            return Err(Error::new(ErrorKind::Other,
                                  MachError::new(ret, "failed to make memory entry")));
        }

        let mut half = addr + len;
        let ret = vm_map(port,
                     &mut half,
                     len,
                     0, // mask
                     VM_FLAGS_FIXED | VM_FLAGS_OVERWRITE,
                     map_port,
                     0, // offset
                     false, // copy
                     VM_PROT_DEFAULT,
                     VM_PROT_DEFAULT,
                     VM_INHERIT_NONE);
        if ret != KERN_SUCCESS {
            vm_deallocate(port, addr, 2*len);
            return Err(Error::new(ErrorKind::Other,
                                  MachError::new(ret, "failed to map memory")));
        }

        Ok(addr as *mut u8)
    }
}

pub fn unmap_ring(pg: *mut u8, len: usize) {
    unsafe {
        let port = mach_task_self();
        vm_deallocate(port, pg as vm_address_t, 2*len);
    }
}

