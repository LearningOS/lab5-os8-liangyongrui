use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
use alloc::vec::Vec;

pub fn sys_sleep(ms: usize) -> isize {
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}

// LAB5 HINT: you might need to maintain data structures used for deadlock detection
// during sys_mutex_* and sys_semaphore_* syscalls
pub fn sys_mutex_create(blocking: bool) -> isize {
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        process_inner.mutex_works[id] = true;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_works.push(true);
        process_inner.mutex_list.len() as isize - 1
    }
}

// LAB5 HINT: Return -0xDEAD if deadlock is detected
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    // take from Processor
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    let process = task.process.upgrade().unwrap();
    let tid = task_inner.res.as_ref().unwrap().tid;
    let mut process_inner = process.inner_exclusive_access();
    process_inner.mutex_need[tid] = Some(mutex_id);
    if process_inner.enable_deadlock_detect
        && check_mutex_dead(
            process_inner.mutex_need.clone(),
            process_inner.mutex_works.clone(),
            &process_inner.mutex_allocation,
            process_inner.mutex_finish.clone(),
        )
    {
        return -0xDEAD;
    }
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(task_inner);
    drop(process_inner);
    mutex.lock();
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.mutex_need[tid] = None;
    process_inner.mutex_allocation[tid].push(mutex_id);
    process_inner.mutex_works[mutex_id] = false;
    0
}

fn check_mutex_dead(
    // mutex_need[i] = j, 表示线程i 在等待 j 锁
    mut mutex_need: Vec<Option<usize>>,
    // mutex_works[i] = true 表示i还可以用
    mut mutex_works: Vec<bool>,
    // mutex_allocation[i] = [j,k], 表示线程i 持有 j,k 两个锁
    mutex_allocation: &[Vec<usize>],
    // mutex_finish[i] = true 表示线程i已经执行完
    mut mutex_finish: Vec<bool>,
) -> bool {
    while !mutex_finish.iter().all(|&val| val) {
        let Some(tid) = mutex_finish
            .iter()
            .enumerate()
            .filter(|(_, &val)| !val)
            .map(|(i, _)| i)
            .find(|&i| {
                mutex_need[i].filter(|&id|!mutex_works[id]).is_none()
            }) else { return true; };
        mutex_need[tid] = None;
        for &id in &mutex_allocation[tid] {
            mutex_works[id] = true
        }
        mutex_finish[tid] = true;
    }
    false
}

pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    let process = task.process.upgrade().unwrap();
    let tid = task_inner.res.as_ref().unwrap().tid;
    let mut process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    mutex.unlock();
    process_inner.mutex_allocation[tid].retain(|&x| x != mutex_id);
    process_inner.mutex_works[mutex_id] = true;
    0
}

pub fn sys_semaphore_create(res_count: usize) -> isize {
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        process_inner.semaphore_works[id] = res_count;
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.semaphore_works.push(res_count);
        let len = process_inner.semaphore_list.len();
        for a in &mut process_inner.semaphore_allocation {
            while a.len() < len {
                a.push(0);
            }
        }
        len - 1
    };
    id as isize
}

pub fn sys_semaphore_up(sem_id: usize) -> isize {
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    let process = task.process.upgrade().unwrap();
    let tid = task_inner.res.as_ref().unwrap().tid;
    let mut process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    sem.up();
    process_inner.semaphore_allocation[tid][sem_id] -= 1;
    process_inner.semaphore_works[sem_id] += 1;
    0
}

// LAB5 HINT: Return -0xDEAD if deadlock is detected
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    let process = task.process.upgrade().unwrap();
    let tid = task_inner.res.as_ref().unwrap().tid;
    let mut process_inner = process.inner_exclusive_access();
    process_inner.semaphore_need[tid] = Some(sem_id);
    if process_inner.enable_deadlock_detect
        && check_semaphore_down_dead(
            process_inner.semaphore_works.clone(),
            &process_inner.semaphore_allocation,
            process_inner.semaphore_need.clone(),
            process_inner.semaphore_finish.clone(),
        )
    {
        return -0xDEAD;
    }
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(task_inner);
    drop(process_inner);
    sem.down();
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.semaphore_need[tid] = None;
    process_inner.semaphore_allocation[tid][sem_id] += 1;
    process_inner.semaphore_works[sem_id] -= 1;
    0
}

fn check_semaphore_down_dead(
    // semaphore_works[i] = 2 表示i还可以用2次
    mut semaphore_works: Vec<usize>,
    // semaphore_allocation[i][j] = 2, 表示线程i 持有 j 锁 2次
    semaphore_allocation: &[Vec<usize>],
    // semaphore_need[i] = j, 表示线程i 在等待 j 锁
    mut semaphore_need: Vec<Option<usize>>,
    // semaphore_finish[i] = true 表示线程i已经执行完
    mut semaphore_finish: Vec<bool>,
) -> bool {
    while !semaphore_finish.iter().all(|&val| val) {
        let Some(tid) = semaphore_finish
            .iter()
            .enumerate()
            .filter(|(_, &val)| !val)
            .map(|(i, _)| i)
            .find(|&i| {
                semaphore_need[i].filter(|&id|semaphore_works[id] == 0).is_none()
            }) else { return true; };

        semaphore_need[tid] = None;
        for (id, &val) in semaphore_allocation[tid].iter().enumerate() {
            semaphore_works[id] += val;
        }
        semaphore_finish[tid] = true;
    }
    false
}

pub fn sys_condvar_create(_arg: usize) -> isize {
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}

pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}

pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}

// LAB5 YOUR JOB: Implement deadlock detection, but might not all in this syscall
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.enable_deadlock_detect = enabled != 0;
    0
}
