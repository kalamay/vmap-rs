#![allow(non_camel_case_types)]

use std::os::raw::{c_int, c_uint};

use libc::uintptr_t;

use crate::{Error, Operation, Result};

use self::Operation::*;

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

const KERN_SUCCESS: kern_return_t = 0;

const VM_PROT_READ: vm_prot_t = 0x01;
const VM_PROT_WRITE: vm_prot_t = 0x02;
const VM_PROT_DEFAULT: vm_prot_t = VM_PROT_READ | VM_PROT_WRITE;

const VM_FLAGS_FIXED: c_int = 0x0000;
const VM_FLAGS_ANYWHERE: c_int = 0x0001;
const VM_FLAGS_OVERWRITE: c_int = 0x4000;

const VM_INHERIT_NONE: vm_inherit_t = 2;

extern "C" {
    fn mach_task_self() -> mach_port_t;

    fn vm_allocate(
        target_task: vm_map_t,
        address: *mut vm_address_t,
        size: vm_size_t,
        flags: c_int,
    ) -> kern_return_t;

    fn vm_deallocate(
        target_task: vm_map_t,
        address: vm_address_t,
        size: vm_size_t,
    ) -> kern_return_t;

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
        inheritance: vm_inherit_t,
    ) -> kern_return_t;

    fn mach_make_memory_entry(
        target_task: vm_map_t,
        size: *mut vm_size_t,
        offset: vm_offset_t,
        permission: vm_prot_t,
        object_handle: *mut mem_entry_name_port_t,
        parent_entry: mem_entry_name_port_t,
    ) -> kern_return_t;
}

/// Creates an anonymous circular allocation.
///
/// The length is the size of the sequential range, and the offset of
/// `len+1` refers to the same memory location at offset `0`. The circle
/// continues to up through the offset of `2*len - 1`.
pub fn map_ring(len: usize) -> Result<*mut u8> {
    let port = unsafe { mach_task_self() };
    let mut addr: vm_address_t = 0;
    let mut len = len as vm_size_t;
    let mut map_port: mem_entry_name_port_t = 0;

    let ret = unsafe { vm_allocate(port, &mut addr, 2 * len, VM_FLAGS_ANYWHERE) };
    if ret != KERN_SUCCESS {
        return Err(Error::kernel(RingAllocate, ret));
    }

    let ret = unsafe { vm_allocate(port, &mut addr, len, VM_FLAGS_FIXED | VM_FLAGS_OVERWRITE) };
    if ret != KERN_SUCCESS {
        unsafe {
            vm_deallocate(port, addr, 2 * len);
        }
        return Err(Error::kernel(RingPrimary, ret));
    }

    let ret =
        unsafe { mach_make_memory_entry(port, &mut len, addr, VM_PROT_DEFAULT, &mut map_port, 0) };
    if ret != KERN_SUCCESS {
        unsafe {
            vm_deallocate(port, addr, 2 * len);
        }
        return Err(Error::kernel(RingEntry, ret));
    }

    let mut half = addr + len;
    let ret = unsafe {
        vm_map(
            port,
            &mut half,
            len,
            0, // mask
            VM_FLAGS_FIXED | VM_FLAGS_OVERWRITE,
            map_port,
            0,     // offset
            false, // copy
            VM_PROT_DEFAULT,
            VM_PROT_DEFAULT,
            VM_INHERIT_NONE,
        )
    };
    if ret != KERN_SUCCESS {
        unsafe {
            vm_deallocate(port, addr, 2 * len);
        }
        return Err(Error::kernel(RingSecondary, ret));
    }

    Ok(addr as *mut u8)
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
    let port = mach_task_self();
    let ret = vm_deallocate(port, pg as vm_address_t, 2 * len);
    if ret != KERN_SUCCESS {
        Err(Error::kernel(RingDeallocate, ret))
    } else {
        Ok(())
    }
}
