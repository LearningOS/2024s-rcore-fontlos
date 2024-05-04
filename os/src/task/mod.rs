//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the operating system.
//!
//! Be careful when you see `__switch` ASM function in `switch.S`. Control flow around this function
//! might not be what you expect.

mod context;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use crate::loader::{get_app_data, get_num_app};
use crate::mm::{MemoryResult, MapPermission, PagePermissionError, VirtAddr};
use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use alloc::vec::Vec;
use lazy_static::*;
use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;

/// The task manager, where all the tasks are managed.
///
/// Functions implemented on `TaskManager` deals with all task state transitions
/// and task context switching. For convenience, you can find wrappers around it
/// in the module level.
///
/// Most of `TaskManager` are hidden behind the field `inner`, to defer
/// borrowing checks to runtime. You can see examples on how to use `inner` in
/// existing functions on `TaskManager`.
pub struct TaskManager {
    /// total number of tasks
    num_app: usize,
    /// use inner value to get mutable access
    inner: UPSafeCell<TaskManagerInner>,
}

/// The task manager inner in 'UPSafeCell'
struct TaskManagerInner {
    /// task list
    tasks: Vec<TaskControlBlock>,
    /// id of current `Running` task
    current_task: usize,
}

lazy_static! {
    /// a `TaskManager` global instance through lazy_static!
    pub static ref TASK_MANAGER: TaskManager = {
        println!("init TASK_MANAGER");
        let num_app = get_num_app();
        println!("num_app = {}", num_app);
        let mut tasks: Vec<TaskControlBlock> = Vec::new();
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(get_app_data(i), i));
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                })
            },
        }
    };
}

impl TaskManager {
    /// Run the first task in task list.
    ///
    /// Generally, the first task in task list is an idle task (we call it zero process later).
    /// But in ch4, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let mut inner = self.inner.exclusive_access();
        let next_task = &mut inner.tasks[0];
        next_task.task_status = TaskStatus::Running;
        // 开始调度
        next_task.task_info.dispatch();
        let next_task_cx_ptr = &next_task.task_cx as *const TaskContext;
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Ready;
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Exited;
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    /// Get the current 'Running' task's token.
    fn get_current_token(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_user_token()
    }

    /// Get the current 'Running' task's trap contexts.
    fn get_current_trap_cx(&self) -> &'static mut TrapContext {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_trap_cx()
    }

    /// Change the current 'Running' task's program break
    pub fn change_current_program_brk(&self, size: i32) -> Option<usize> {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].change_program_brk(size)
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some(next) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            inner.tasks[next].task_status = TaskStatus::Running;
            inner.current_task = next;
            // 开始调度
            inner.tasks[next].task_info.dispatch();
            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            // before this, we should drop local variables that must be dropped manually
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
            // go back to user mode
        } else {
            panic!("All applications completed!");
        }
    }

    // 获取当前任务状态
    fn get_task_status(&self) -> TaskStatus {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].task_status
    }

    /// 获取调度起始时间
    pub fn get_start_time(&self) -> usize {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].task_info.start_time
    }

    // syscall 调用次数的映射表
    fn set_syscall_times(&self, syscalls: &mut [u32; crate::config::MAX_SYSCALL_NUM]) {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        for (id, n) in inner.tasks[current].task_info.syscall_times.iter() {
            syscalls[*id] = *n;
        }
    }

    /// 针对 id 的 syscall 调用次数计数器
    pub fn syscall_counter(&self, syscall_id: usize) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let times = &mut inner.tasks[current].task_info.syscall_times;
        // 保存每个 syscall 的调用次数, 谨防 syscall_id 无效
        *times.entry(syscall_id).or_default() += 1;
    }

    /// 虚拟内存与物理内存的映射
    fn map_memory(&self, start_virtaddr: VirtAddr, end_virtaddr: VirtAddr, permission: MapPermission) -> isize {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let memset = &mut inner.tasks[current].memory_set;
        if memset.map_memory(start_virtaddr, end_virtaddr, permission).is_ok() {
            0
        } else {
            -1
        }
    }

    /// 取消映射
    fn unmap_memory(&self, start_virtaddr: VirtAddr, end_virtaddr: VirtAddr) -> isize {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let memset = &mut inner.tasks[current].memory_set;
        if memset.unmap_memory(start_virtaddr, end_virtaddr).is_ok() {
            0
        } else {
            -1
        }
    }

    /// 检查可读性
    pub fn check_readable(&self, va: VirtAddr) -> MemoryResult<()> {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let memset = &mut inner.tasks[current].memory_set;
        let a = memset.translate(va.floor())?;
        if a.readable() {
            Ok(())
        } else {
            Err(PagePermissionError::Unreadable.into())
        }
    }
    /// 检查可写性
    pub fn check_writeable(&self, va: VirtAddr) -> MemoryResult<()> {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let memset = &mut inner.tasks[current].memory_set;
        let a = memset.translate(va.floor())?;
        if a.readable() && a.writable() {
            Ok(())
        } else {
            Err(PagePermissionError::Unwritable.into())
        }
    }

    /// 检查可执行性
    pub fn check_executable(&self, va: VirtAddr) -> MemoryResult<()> {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let memset = &mut inner.tasks[current].memory_set;
        let a = memset.translate(va.floor())?;
        if a.readable() && a.executable() {
            Ok(())
        } else {
            Err(PagePermissionError::Unexecutable.into())
        }
    }
}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// Switch current `Running` task to the task we have found,
/// or there is no `Ready` task and we can exit with all applications completed
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// Change the status of current `Running` task into `Ready`.
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// Change the status of current `Running` task into `Exited`.
fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

/// Get the current 'Running' task's token.
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// Get the current 'Running' task's trap contexts.
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}

/// Change the current 'Running' task's program break
pub fn change_program_brk(size: i32) -> Option<usize> {
    TASK_MANAGER.change_current_program_brk(size)
}

/// 获取当前任务状态
pub fn get_task_status() -> TaskStatus {
    TASK_MANAGER.get_task_status()
}

/// 获取调度起始时间
pub fn get_start_time() -> usize {
    TASK_MANAGER.get_start_time()
}

/// 设置 syscall 调用次数
pub fn set_syscall_times(syscalls: &mut [u32; crate::config::MAX_SYSCALL_NUM]) {
    TASK_MANAGER.set_syscall_times(syscalls)
}

/// 针对 id 的系统调用计数器
pub fn syscall_counter(syscall_id: usize) {
    TASK_MANAGER.syscall_counter(syscall_id);
}

/// 虚拟内存与物理内存的映射
pub fn map_memory(start_virtaddr: VirtAddr, end_virtaddr: VirtAddr, permission: MapPermission) -> isize {
    TASK_MANAGER.map_memory(start_virtaddr, end_virtaddr, permission)
}

/// 取消映射
pub fn unmap_memory(start_virtaddr: VirtAddr, end_virtaddr: VirtAddr) -> isize {
    TASK_MANAGER.unmap_memory(start_virtaddr, end_virtaddr)
}

/// 检查可读性
pub fn check_readable(va: VirtAddr) -> MemoryResult<()> {
    TASK_MANAGER.check_readable(va)
}
/// 检查可写性
pub fn check_writeable(va: VirtAddr) -> MemoryResult<()> {
    TASK_MANAGER.check_writeable(va)
}

/// 检查可执行性
pub fn check_executable(va: VirtAddr) -> MemoryResult<()> {
    TASK_MANAGER.check_executable(va)
}