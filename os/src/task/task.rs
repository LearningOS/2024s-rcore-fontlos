//! Types related to task management

// 有序键值对
use alloc::collections::BTreeMap;

use super::TaskContext;

/// The task control block (TCB) of a task.
#[derive(Clone)]
pub struct TaskControlBlock {
    /// The task status in it's lifecycle
    pub task_status: TaskStatus,
    /// The task context
    pub task_cx: TaskContext,
    /// 任务信息
    pub task_info: TaskInfo,
}

/// 任务信息块
#[derive(Clone)]
pub struct TaskInfo {
    /// 任务是否进行
    pub is_dispatched: bool,
    /// 调度时间
    pub start_time: usize,
    /// 调用次数
    pub syscall_times: BTreeMap<usize, u32>
}

impl TaskInfo {
    /// 初始化任务信息
    pub fn new() -> Self {
        TaskInfo {
            is_dispatched: false,
            start_time: 0,
            syscall_times: BTreeMap::new()
        }
    }
    /// 开始调度
    pub fn dispatch(&mut self) {
        if !self.is_dispatched {
            self.start_time = crate::timer::get_time_ms();
            self.is_dispatched = true;
        }
    }
}
/// The status of a task
#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    /// uninitialized
    UnInit,
    /// ready to run
    Ready,
    /// running
    Running,
    /// exited
    Exited,
}
