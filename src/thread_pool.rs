use crossbeam_deque::{Injector, Stealer, Worker, Steal};
use log::trace;
use std::collections::BTreeMap;
use std::error::Error;
use std::marker::PhantomData;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Barrier, RwLock, Mutex};
use std::{fmt, hint, iter, mem};

use crate::channel::channel::Channel;
use crate::core::orchestrator::{get_global_orchestrator, JobInfo, Orchestrator};

type Func<'a> = Box<dyn FnOnce() + Send + 'a>;

enum Job {
    NewJob(Func<'static>),
    Terminate,
}

#[derive(Debug)]
pub struct ThreadPoolError {
    details: String,
}

impl ThreadPoolError {
    fn new(msg: &str) -> ThreadPoolError {
        ThreadPoolError {
            details: msg.to_string(),
        }
    }
}

impl fmt::Display for ThreadPoolError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}

impl Error for ThreadPoolError {
    fn description(&self) -> &str {
        &self.details
    }
}



// Struct representing a worker in the thread pool.
struct ThreadPoolWorker {
    id: usize,
    worker: Mutex<Worker<Job>>,
    stealers: RwLock<Vec<Stealer<Job>>>,
    global: Arc<Injector<Job>>,
    total_tasks: Arc<AtomicUsize>,
}
impl ThreadPoolWorker {
    fn new(id: usize, global: Arc<Injector<Job>>, total_tasks: Arc<AtomicUsize>) -> Self {
        let worker = Mutex::new(Worker::new_fifo());
        let stealers = RwLock::new(Vec::new());
        Self {
            id,
            worker,
            stealers,
            global,
            total_tasks,

        }
    }


    // Fetch a task. If the local queue is empty, try to steal a batch of tasks from the global queue.
    // If the global queue is empty, try to steal a task from one of the other threads.
    fn fetch_task(&self) -> Option<Job> {
        if let Some(job) = self.pop() {
            return Some(job);
        } else if let Some(job) = self.steal_from_global() {
            return Some(job);
        } else if let Some(job) = self.steal() {
            return Some(job);
        }
        None
    }

    /// This is the main loop of the thread.
    fn run(&self) {
        let mut stop = false;
        loop {
            let res = self.fetch_task();
            match res {
                Some(task) => match task {
                    Job::NewJob(func) => {
                        (func)();
                        self.task_done();
                    }
                    Job::Terminate => stop = true,
                },
                None => {
                    if stop {
                        break;
                    } else {
                        continue;
                    }
                }
            }
        }
    }

    // Push a job to the local queue.
    fn push(&self, job: Job) {
        let worker = self.worker.lock().unwrap();
        worker.push(job);
    }

    // Pop a job from the local queue.
    fn pop(&self) -> Option<Job> {
        let worker = self.worker.lock().unwrap();
        worker.pop()
    }

    // Steal a job from another worker.
    fn steal(&self) -> Option<Job> {
        let stealers = self.stealers.read().unwrap();
        for stealer in stealers.iter() {
            loop {
                match stealer.steal() {
                    Steal::Success(job) => return Some(job),
                    Steal::Empty => break,
                    Steal::Retry => continue,
                }
            }
        }
        None
    }

    // Steal a job from the global queue.
    fn steal_from_global(&self) -> Option<Job> {
        let worker_queue = self.worker.lock().unwrap();
        loop {
            match self.global.steal_batch_and_pop(&worker_queue) {
                Steal::Success(job) => return Some(job),
                Steal::Empty => return None,
                Steal::Retry => continue,
            };
        }
    }

    // Add a stealer to the list of stealers.
    fn add_stealer(&self, stealer: Stealer<Job>) {
        let mut stealers = self.stealers.write().unwrap();
        stealers.push(stealer);
    }

    // Clean the list of stealers.
    fn clean_stealers(&self) {
        let mut stealers = self.stealers.write().unwrap();
        stealers.clear();
    }

    // Get the stealer of the local queue.
    fn stealer(&self) -> Stealer<Job> {
        let worker = self.worker.lock().unwrap();
        worker.stealer()
    }

    // Warn task done.
    fn task_done(&self) {
        self.total_tasks.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }

}
///Struct representing a thread pool.
pub struct ThreadPool {
    jobs_info: Vec<JobInfo>,
    workers: Vec<Arc<ThreadPoolWorker>>,
    total_tasks: Arc<AtomicUsize>,
    injector: Arc<Injector<Job>>,
    orchestrator: Arc<Orchestrator>,
}

impl Clone for ThreadPool {
    /// Create a new threadpool from an existing one, using the same number of threads.
    fn clone(&self) -> Self {
        let orchestrator = self.orchestrator.clone();
        ThreadPool::new(self.workers.len(), orchestrator)
    }
}

impl ThreadPool {
    fn new(num_threads: usize, orchestrator: Arc<Orchestrator>) -> Self {
        trace!("Creating new threadpool");
        let jobs_info;
        let mut workers = Vec::with_capacity(num_threads);

        let total_tasks = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(num_threads));
        let mut funcs = Vec::new();

        let injector = Arc::new(Injector::new());

        for i in 0..num_threads {
            let global = Arc::clone(&injector);
            let total_tasks_cp = Arc::clone(&total_tasks);
            let worker = ThreadPoolWorker::new(i, global, total_tasks_cp);
            workers.push(Arc::new(worker));
        }

        for worker in &workers {
            let mut tmp = Vec::new();
            let stealer = worker.stealer();
            for other in &workers {
                if worker.id != other.id {
                    other.add_stealer(stealer.clone());
                    tmp.push(other.stealer());
                }
            }
            for stealer in tmp {
                worker.add_stealer(stealer);
            }
        }

        for worker in &workers {
            let barrier = Arc::clone(&barrier);
            let worker = Arc::clone(&worker);
            let func = move || {
                barrier.wait();
                worker.run();
            };
            funcs.push(Box::new(func));
        }

        jobs_info = orchestrator.push_multiple(funcs);

        Self {
            workers,
            jobs_info,
            total_tasks,
            injector,
            orchestrator,
        }
    }

    pub fn new_with_global_registry(num_threads: usize) -> Self {
        let orchestrator = get_global_orchestrator();
        Self::new(num_threads, orchestrator)
    }

    /// Execute a function `task` on a thread in the thread pool.
    pub fn execute<F>(&self, task: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.injector.push(Job::NewJob(Box::new(task)));
        self.total_tasks
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }

    /// Block until all current jobs in the thread pool are finished.
    pub fn wait(&self) {
        while (self.total_tasks.load(std::sync::atomic::Ordering::Acquire) != 0)
            || !self.injector.is_empty()
        {
            hint::spin_loop();
        }
    }
    /// Applies in parallel the function `f` on a iterable object `iter`.
    ///
    /// # Examples
    ///
    /// Increment of 1 all the elements in a vector concurrently:
    ///
    /// ```
    /// use pspp::thread_pool::ThreadPool;
    ///
    /// let mut pool = ThreadPool::new_with_global_registry(8);
    /// let mut vec = vec![0; 100];
    ///
    /// pool.par_for(&mut vec, |el: &mut i32| *el = *el + 1);
    /// pool.wait(); // wait the threads to finish the jobs
    ///
    pub fn par_for<Iter: IntoIterator, F>(&mut self, iter: Iter, f: F)
    where
        F: FnOnce(Iter::Item) + Send + 'static + Copy,
        <Iter as IntoIterator>::Item: Send,
    {
        self.scoped(|s| {
            iter.into_iter().for_each(|el| s.execute(move || (f)(el)));
        });
    }
    /// Applies in parallel the function `f` on a iterable object `iter`,
    /// producing a new iterator with the results.
    ///
    /// # Examples
    ///
    /// Produce a vec of `String` from the elements of a vector `vec` concurrently:
    ///
    /// ```
    /// use pspp::thread_pool::ThreadPool;
    ///
    /// let mut pool = ThreadPool::new_with_global_registry(8);
    /// let mut vec = vec![0i32; 100];
    ///
    /// let res: Vec<String> = pool.par_map(&mut vec, |el| -> String {
    ///            String::from("Hello from: ".to_string() + &el.to_string())
    ///       }).collect();
    ///
    pub fn par_map<Iter: IntoIterator, F, R>(&mut self, iter: Iter, f: F) -> impl Iterator<Item = R>
    where
        F: FnOnce(Iter::Item) -> R + Send + Copy,
        <Iter as IntoIterator>::Item: Send,
        R: Send + 'static,
    {
        let (rx, tx) = Channel::channel(true);
        let arc_tx = Arc::new(tx);
        let mut unordered_map = BTreeMap::<usize, R>::new();
        self.scoped(|s| {
            iter.into_iter().enumerate().for_each(|el| {
                let cp = Arc::clone(&arc_tx);
                s.execute(move || {
                    let err = cp.send((el.0, f(el.1)));
                    if err.is_err() {
                        panic!("Error: {}", err.unwrap_err());
                    }
                });
            });
        });
        self.wait();
        while !rx.is_empty() {
            let msg = rx.receive();
            match msg {
                Ok(Some((order, result))) => {
                    unordered_map.insert(order, result);
                }
                Ok(None) => continue,
                Err(e) => panic!("Error: {}", e),
            };
        }
        unordered_map.into_values()
    }

    /// Borrows the thread pool and allows executing jobs on other
    /// threads during that scope via the argument of the closure.
    pub fn scoped<'pool, 'scope, F, R>(&'pool mut self, f: F) -> R
    where
        F: FnOnce(&Scope<'pool, 'scope>) -> R,
    {
        let scope = Scope {
            pool: self,
            _marker: PhantomData,
        };
        f(&scope)
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        trace!("Closing threadpool");
        for worker in &self.workers {
            worker.clean_stealers();
        }

        for worker in &self.workers {
            worker.push(Job::Terminate);
        }

        for job in &self.jobs_info {
            job.wait();
        }
    }
}
/// A scope to executes scoped jobs in the thread pool.
pub struct Scope<'pool, 'scope> {
    pool: &'pool mut ThreadPool,
    _marker: PhantomData<::std::cell::Cell<&'scope mut ()>>,
}

impl<'pool, 'scope> Scope<'pool, 'scope> {
    /// Execute a function `task` on a thread in the thread pool.
    pub fn execute<F>(&self, task: F)
    where
        F: FnOnce() + Send + 'scope,
    {
        let task = unsafe { mem::transmute::<Func<'scope>, Func<'static>>(Box::new(task)) };
        self.pool.injector.push(Job::NewJob(Box::new(task)));
        self.pool
            .total_tasks
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::ThreadPool;
    use crate::core::orchestrator::Orchestrator;
    use serial_test::serial;

    fn fib(n: i32) -> u64 {
        if n < 0 {
            panic!("{} is negative!", n);
        }
        match n {
            0 => panic!("zero is not a right argument to fib()!"),
            1 | 2 => 1,
            3 => 2,
            _ => fib(n - 1) + fib(n - 2),
        }
    }

    #[test]
    #[serial]
    fn test_threadpool() {
        let tp = ThreadPool::new_with_global_registry(8);
        for i in 1..45 {
            tp.execute(move || {
                fib(i);
            });
        }
        tp.wait();
        Orchestrator::delete_global_orchestrator();
    }

    #[test]
    #[serial]
    fn test_scoped_thread() {
        let mut vec = vec![0; 100];
        let mut tp = ThreadPool::new_with_global_registry(8);

        tp.scoped(|s| {
            for e in vec.iter_mut() {
                s.execute(move || {
                    *e += 1;
                });
            }
        });

        tp.wait();
        Orchestrator::delete_global_orchestrator();
        assert_eq!(vec, vec![1i32; 100])
    }

    #[test]
    #[serial]
    fn test_par_for() {
        let mut vec = vec![0; 100];
        let mut tp = ThreadPool::new_with_global_registry(8);

        tp.par_for(&mut vec, |el: &mut i32| *el += 1);
        tp.wait();
        Orchestrator::delete_global_orchestrator();
        assert_eq!(vec, vec![1i32; 100])
    }

    #[test]
    #[serial]
    fn test_par_map() {
        env_logger::init();
        let mut vec = Vec::new();
        let mut tp = ThreadPool::new_with_global_registry(16);

        for i in 0..1000 {
            vec.push(i);
        }
        let res: Vec<String> = tp
            .par_map(vec, |el| -> String {
                "Hello from: ".to_string() + &el.to_string()
            })
            .collect();

        let mut check = true;
        for (i, str) in res.into_iter().enumerate() {
            if str != "Hello from: ".to_string() + &i.to_string() {
                check = false;
            }
        }
        Orchestrator::delete_global_orchestrator();
        assert!(check)
    }

    #[test]
    #[serial]
    fn test_multiple_threadpool() {
        let tp_1 = ThreadPool::new_with_global_registry(4);
        let tp_2 = ThreadPool::new_with_global_registry(4);
        ::scopeguard::defer! {
            tp_1.wait();
            tp_2.wait();

        }
        Orchestrator::delete_global_orchestrator();
    }
}
