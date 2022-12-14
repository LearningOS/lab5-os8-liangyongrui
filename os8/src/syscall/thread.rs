use crate::{
    mm::kernel_token,
    task::{add_task, current_task, TaskControlBlock},
    trap::{trap_handler, TrapContext},
};
use alloc::vec;
use alloc::{sync::Arc, vec::Vec};
pub fn sys_thread_create(entry: usize, arg: usize) -> isize {
    let task = current_task().unwrap();
    let process = task.process.upgrade().unwrap();
    // create a new thread
    let new_task = Arc::new(TaskControlBlock::new(
        Arc::clone(&process),
        task.inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .ustack_base,
        true,
    ));
    let new_task_inner = new_task.inner_exclusive_access();
    let new_task_res = new_task_inner.res.as_ref().unwrap();
    let new_task_tid = new_task_res.tid;
    let new_task_trap_cx = new_task_inner.get_trap_cx();
    *new_task_trap_cx = TrapContext::app_init_context(
        entry,
        new_task_res.ustack_top(),
        kernel_token(),
        new_task.kernel_stack.get_top(),
        trap_handler as usize,
    );
    new_task_trap_cx.x[10] = arg;

    let mut process_inner = process.inner_exclusive_access();
    let len = process_inner.semaphore_list.len();
    while process_inner.tasks.len() < new_task_tid + 1 {
        process_inner.tasks.push(None);
        process_inner.mutex_allocation.push(Vec::new());
        process_inner.mutex_need.push(None);
        process_inner.mutex_finish.push(false);
        process_inner.semaphore_allocation.push(vec![0; len]);
        process_inner.semaphore_need.push(None);
        process_inner.semaphore_finish.push(false);
    }
    process_inner.tasks[new_task_tid] = Some(Arc::clone(&new_task));
    process_inner.mutex_allocation[new_task_tid] = Vec::new();
    process_inner.mutex_need[new_task_tid] = None;
    process_inner.mutex_finish[new_task_tid] = false;
    process_inner.semaphore_allocation[new_task_tid] = vec![0; len];
    process_inner.semaphore_need[new_task_tid] = None;
    process_inner.semaphore_finish[new_task_tid] = false;
    // add new task to scheduler
    add_task(Arc::clone(&new_task));
    new_task_tid as isize
}

pub fn sys_gettid() -> isize {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid as isize
}

/// thread does not exist, return -1
/// thread has not exited yet, return -2
/// otherwise, return thread's exit code
pub fn sys_waittid(tid: usize) -> i32 {
    let task = current_task().unwrap();
    let process = task.process.upgrade().unwrap();
    let task_inner = task.inner_exclusive_access();
    let mut process_inner = process.inner_exclusive_access();
    // a thread cannot wait for itself
    if task_inner.res.as_ref().unwrap().tid == tid {
        return -1;
    }
    let mut exit_code: Option<i32> = None;
    let waited_task = process_inner.tasks[tid].as_ref();
    if let Some(waited_task) = waited_task {
        if let Some(waited_exit_code) = waited_task.inner_exclusive_access().exit_code {
            exit_code = Some(waited_exit_code);
        }
    } else {
        // waited thread does not exist
        return -1;
    }
    if let Some(exit_code) = exit_code {
        // dealloc the exited thread
        process_inner.tasks[tid] = None;
        exit_code
    } else {
        // waited thread has not exited
        -2
    }
}
