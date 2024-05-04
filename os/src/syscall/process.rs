//! Process management syscallsuse core::mem::size_of;

use core::mem::size_of;

use crate::{
    config::MAX_SYSCALL_NUM,
    mm::{translated_byte_buffer, MapPermission, VirtAddr},
    task::{
        change_program_brk,
        current_user_token,
        exit_current_and_run_next,
        get_start_time,
        set_syscall_times,
        get_task_status,
        map_memory,
        unmap_memory,
        suspend_current_and_run_next,
        TaskStatus
    }
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// Task information
#[allow(dead_code)]
pub struct TaskInfo {
    /// Task status in it's life cycle
    status: TaskStatus,
    /// The numbers of syscall called by task
    syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Total running time of task
    time: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

fn copy(origin: &[u8], target: alloc::vec::Vec<&mut [u8]>) {
    let len = target.iter().map(|x|{(**x).len()}).sum::<usize>();
    assert_eq!(len, origin.len(), "copy failed: length not equal");
    let mut index = 0;
    for r in target.into_iter() {
        for b in r.iter_mut() {
            *b = origin[index];
            index = index + 1;
        }
    }
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    const SIZE: usize = size_of::<TimeVal>();
    if let Ok(regions) = translated_byte_buffer(current_user_token(), ts as *const u8, SIZE) {
        let now = crate::timer::get_time_us();
        let mut buffer = [0u8; SIZE];
        unsafe {
            let raw_ptr = buffer.as_mut_ptr() as usize as *mut TimeVal;
            *raw_ptr = TimeVal {
                sec: now / 1_000_000,
                usec: now % 1_000_000,
            };
        }
        copy(&buffer, regions);
        0
    } else {
        -1
    }
}

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info");
    const SIZE: usize = size_of::<TaskInfo>();
    if let Ok(regions) = translated_byte_buffer(current_user_token(), ti as *const u8, SIZE) {
        let mut buffer = alloc::vec![0u8; SIZE]; // size of TaskInfo is too large, so we choose to allocate on kernel heap
        unsafe {
            let info = (buffer.as_mut_ptr() as usize as *mut TaskInfo).as_mut().unwrap();
            info.time = crate::timer::get_time_ms() - get_start_time();
            info.status = get_task_status();
            set_syscall_times(&mut info.syscall_times);
        }
        copy(&buffer, regions);
        0
    } else {
        -1
    }
}

// YOUR JOB: Implement mmap.(啊这懒得改名字了)
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    trace!("kernel: sys_mmap");
    if start % crate::config::PAGE_SIZE != 0 {
        return -1;
    }
    if port & (!0x7) != 0 || port & 0x7 == 0 {
        return -1;
    }
    let start_virtaddr: VirtAddr = start.into();
    let end_virtaddr: VirtAddr = (start + len).into();
    let flags = (port as u8) << 1;
    map_memory(start_virtaddr, end_virtaddr, MapPermission::from_bits(flags).unwrap() | MapPermission::U)
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    if start % crate::config::PAGE_SIZE != 0 {
        return -1;
    }
    let start_virtaddr: VirtAddr = start.into();
    let end_virtaddr: VirtAddr = (start + len).into();
    unmap_memory(start_virtaddr, end_virtaddr)
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
