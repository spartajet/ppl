use std::{
    collections::VecDeque,
    marker::PhantomData,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier, Condvar, Mutex,
    },
};

use dyn_clone::DynClone;
use log::{trace, warn};
use std::collections::BTreeMap;

use crate::{
    channel::{
        channel::{Channel, InputChannel, OutputChannel},
        err::ChannelError,
    },
    core::orchestrator::{JobInfo, Orchestrator},
    task::{Message, Task},
};

use super::node::Node;

/// Trait defining a node that receive an input and produce an output.
///
/// # Examples:
///
/// A node that receive an integer and increment it by one:
/// ```
/// use pspp::node::{inout_node::{InOut, InOutNode}};
/// #[derive(Clone)]
/// struct Worker {}
/// impl InOut<i32, i32> for Worker {
///    fn run(&mut self, input: i32) -> Option<i32> {
///        Some(input + 1)
///    }
/// }
/// ```
///
///
pub trait InOut<TIn, TOut>: DynClone {
    /// This method is called each time the node receive an input.
    fn run(&mut self, input: TIn) -> Option<TOut>;
    /// If `is_producer` is `true` then this method will be called by the rts immediately
    /// after the execution of `run`.
    /// This method is called by the rts until a None is returned.
    /// When None is returned, the node will wait for another input.
    /// This method can be useful when we have a node that produce multiple output.
    fn produce(&mut self) -> Option<TOut> {
        None
    }
    /// This method return the number of replicas of the node.
    /// Overload this method allow to choose the number of replicas of the node.
    fn number_of_replicas(&self) -> usize {
        1
    }
    /// This method return a boolean that represent if the node receive the input in an ordered way.
    /// Overload this method allow to choose if the node is ordered or not.
    fn is_ordered(&self) -> bool {
        false
    }
    fn broadcasting(&self) -> bool {
        // to be implemented
        false
    }
    fn a2a(&self) -> bool {
        // to be implemented
        false
    }
    /// This method return a boolean that represent if the node is a producer or not.
    /// Overload this method allow to choose if the node produce multiple output or not.
    fn is_producer(&self) -> bool {
        false
    }
}

struct OrderedSplitter {
    latest: usize,
    start: usize,
}
impl OrderedSplitter {
    fn new() -> OrderedSplitter {
        OrderedSplitter {
            latest: 0,
            start: 0,
        }
    }
    fn get(&self) -> (usize, usize) {
        (self.latest, self.start)
    }
    fn set(&mut self, latest: usize, start: usize) {
        self.latest = latest;
        self.start = start;
    }
}

pub struct InOutNode<TIn: Send, TOut: Send, TCollected, TNext: Node<TOut, TCollected>> {
    job_infos: Vec<JobInfo>,
    channels: Vec<OutputChannel<Message<TIn>>>,
    next_node: Arc<TNext>,
    ordered: bool,
    producer: bool,
    ordered_splitter: Arc<(Mutex<OrderedSplitter>, Condvar)>,
    storage: Mutex<BTreeMap<usize, Message<TIn>>>,
    next_msg: AtomicUsize,
    phantom: PhantomData<(TOut, TCollected)>,
}

impl<
        TIn: Send + 'static,
        TOut: Send + 'static,
        TCollected,
        TNext: Node<TOut, TCollected> + Send + Sync + 'static,
    > Node<TIn, TCollected> for InOutNode<TIn, TOut, TCollected, TNext>
{
    fn send(&self, input: Message<TIn>, rec_id: usize) -> Result<(), ChannelError> {
        let mut rec_id = rec_id;
        if rec_id >= self.job_infos.len() {
            rec_id %= self.job_infos.len();
        }

        let Message { op, order } = input;
        match &op {
            Task::NewTask(_e) => {
                if self.channels.len() == 1
                    && self.ordered
                    && order != self.next_msg.load(Ordering::SeqCst)
                {
                    self.save_to_storage(Message::new(op, rec_id), order);
                    self.send_pending();
                } else {
                    let res = self.channels[rec_id].send(Message::new(op, order));
                    if res.is_err() {
                        panic!("Error: Cannot send message!");
                    }

                    if self.ordered {
                        let old_c = self.next_msg.load(Ordering::SeqCst);
                        self.next_msg.store(old_c + 1, Ordering::SeqCst);
                    }
                }
            }
            Task::Dropped => {
                if self.channels.len() == 1
                    && self.ordered
                    && order != self.next_msg.load(Ordering::SeqCst)
                {
                    self.save_to_storage(Message::new(op, rec_id), order);
                    self.send_pending();
                } else {
                    let res = self.channels[rec_id].send(Message::new(op, order));
                    if res.is_err() {
                        panic!("Error: Cannot send message!");
                    }

                    if self.ordered {
                        let old_c = self.next_msg.load(Ordering::SeqCst);
                        self.next_msg.store(old_c + 1, Ordering::SeqCst);
                    }
                }
            }
            Task::Terminate => {
                if self.channels.len() == 1
                    && self.ordered
                    && order != self.next_msg.load(Ordering::SeqCst)
                {
                    self.save_to_storage(Message::new(op, order), order);
                    self.send_pending();
                } else {
                    for ch in &self.channels {
                        let err = ch.send(Message::new(Task::Terminate, order));
                        if err.is_err() {
                            panic!("Error: Cannot send message!");
                        }
                    }

                    if self.ordered {
                        self.next_msg.store(order, Ordering::SeqCst)
                    }
                }
            }
        }
        Ok(())
    }

    fn collect(mut self) -> Option<TCollected> {
        self.wait();
        match Arc::try_unwrap(self.next_node) {
            Ok(nn) => nn.collect(),
            Err(_) => panic!("Error: Cannot collect results inout."),
        }
    }

    fn get_num_of_replicas(&self) -> usize {
        self.job_infos.len()
    }
}

impl<
        TIn: Send + 'static,
        TOut: Send + 'static,
        TCollected,
        TNext: Node<TOut, TCollected> + Sync + Send + 'static,
    > InOutNode<TIn, TOut, TCollected, TNext>
{
    /// Create a new Node.
    /// The `handler` is the  struct that implement the trait `InOut` and defines
    /// the behavior of the node we're creating.
    /// `next_node` contains the stage that follows the node.
    /// If `blocking` is true the node will perform blocking operation on receive.
    /// If `pinning` is `true` the node will be pinned to the thread in position `id`.
    ///
    pub fn new(
        id: usize,
        handler: Box<dyn InOut<TIn, TOut> + Send + Sync>,
        next_node: TNext,
        blocking: bool,
        orchestrator: Arc<Orchestrator>,
    ) -> InOutNode<TIn, TOut, TCollected, TNext> {
        let mut funcs = Vec::new();
        let mut channels = Vec::new();
        let next_node = Arc::new(next_node);
        let replicas = handler.number_of_replicas();

        let splitter = Arc::new((Mutex::new(OrderedSplitter::new()), Condvar::new()));
        let ordered = handler.is_ordered();
        let producer = handler.is_producer();

        let mut handler_copies = Vec::with_capacity(replicas);
        for _i in 0..replicas - 1 {
            handler_copies.push(dyn_clone::clone_box(&*handler));
        }
        handler_copies.push(handler);

        let barrier = Arc::new(Barrier::new(replicas));

        for i in 0..replicas {
            let (channel_in, channel_out) = Channel::channel(blocking);
            channels.push(channel_out);
            let nn = Arc::clone(&next_node);
            let splitter_copy = Arc::clone(&splitter);
            let copy = handler_copies.pop().unwrap();
            let local_barrier = Arc::clone(&barrier);

            let func = move || {
                local_barrier.wait();
                Self::rts(i + id, copy, channel_in, &nn, replicas, &splitter_copy);
            };

            funcs.push(func);
        }

        InOutNode {
            channels,
            job_infos: orchestrator.push_multiple(funcs),
            next_node,
            ordered,
            producer,
            ordered_splitter: splitter,
            storage: Mutex::new(BTreeMap::new()),
            next_msg: AtomicUsize::new(0),
            phantom: PhantomData,
        }
    }

    fn rts(
        id: usize,
        mut node: Box<dyn InOut<TIn, TOut>>,
        channel_in: InputChannel<Message<TIn>>,
        next_node: &TNext,
        n_replicas: usize,
        ordered_splitter_handler: &(Mutex<OrderedSplitter>, Condvar),
    ) {
        // If next node have more replicas, i specify the first next node where i send my msg
        let mut counter = 0;
        if (next_node.get_num_of_replicas() > n_replicas) && n_replicas != 1 {
            counter = id * (next_node.get_num_of_replicas() / n_replicas);
        } else if next_node.get_num_of_replicas() <= n_replicas {
            // Standard case, not a2a
            counter = id;
        }
        trace!("Created a new Node! Id: {}", id);
        loop {
            // If next node have more replicas, when counter > next_replicas i reset the counter
            if (next_node.get_num_of_replicas() > n_replicas)
                && counter >= next_node.get_num_of_replicas()
            {
                counter = 0;
            }

            let input = channel_in.receive();

            match input {
                Ok(Some(Message { op, order })) => match op {
                    Task::NewTask(arg) => {
                        let output = node.run(arg);
                        if !node.is_producer() {
                            match output {
                                Some(msg) => {
                                    let err = next_node
                                        .send(Message::new(Task::NewTask(msg), order), counter);
                                    if err.is_err() {
                                        panic!("Error: {}", err.unwrap_err())
                                    }
                                }
                                None => {
                                    let err =
                                        next_node.send(Message::new(Task::Dropped, order), counter);
                                    if err.is_err() {
                                        panic!("Error: {}", err.unwrap_err())
                                    }
                                }
                            }
                        } else {
                            let mut tmp = VecDeque::new();
                            loop {
                                let splitter_out = node.produce();
                                match splitter_out {
                                    Some(msg) => {
                                        tmp.push_back(msg);
                                    }
                                    None => break,
                                }
                            }

                            if node.is_ordered() {
                                let (lock, cvar) = ordered_splitter_handler;
                                let mut ordered_splitter = lock.lock().unwrap();
                                loop {
                                    let (latest, end) = ordered_splitter.get();
                                    if latest == order {
                                        let mut count_splitter = end;
                                        while !tmp.is_empty() {
                                            let err = next_node.send(
                                                Message::new(
                                                    Task::NewTask(tmp.pop_front().unwrap()),
                                                    count_splitter,
                                                ),
                                                counter,
                                            );
                                            if err.is_err() {
                                                panic!("Error: {}", err.unwrap_err())
                                            }
                                            count_splitter += 1;
                                        }
                                        ordered_splitter.set(order + 1, count_splitter);
                                        cvar.notify_all();
                                        break;
                                    } else {
                                        let err = cvar.wait(ordered_splitter);
                                        if err.is_err() {
                                            panic!("Error: Poisoned mutex!");
                                        } else {
                                            ordered_splitter = err.unwrap();
                                        }
                                    }
                                }
                            } else {
                                while !tmp.is_empty() {
                                    let err = next_node.send(
                                        Message::new(
                                            Task::NewTask(tmp.pop_front().unwrap()),
                                            order,
                                        ),
                                        counter,
                                    );
                                    if err.is_err() {
                                        panic!("Error: {}", err.unwrap_err())
                                    }
                                }
                            }
                        }
                    }
                    Task::Dropped => {
                        let err = next_node.send(Message::new(Task::Dropped, order), counter);
                        if err.is_err() {
                            panic!("Error: {}", err.unwrap_err())
                        }
                    }
                    Task::Terminate => {
                        break;
                    }
                },
                Ok(None) => (),
                Err(e) => {
                    warn!("Error: {}", e);
                }
            }
            if next_node.get_num_of_replicas() > n_replicas {
                counter += 1;
            }
        }
    }

    fn wait(&mut self) {
        for job in &self.job_infos {
            job.wait();
        }

        // Change this that is really shitty
        let mut c = 0;
        if self.ordered && !self.producer {
            c = self.next_msg.load(Ordering::SeqCst); // No need to be seq_cst
        } else if self.ordered && self.producer {
            let (lock, _) = self.ordered_splitter.as_ref();
            let ordered_splitter = lock.lock().unwrap();
            (_, c) = ordered_splitter.get();
        }
        let err = self.next_node.send(Message::new(Task::Terminate, c), 0);
        if err.is_err() {
            panic!("Error: Cannot send message!");
        }
    }

    fn save_to_storage(&self, msg: Message<TIn>, order: usize) {
        let mtx = self.storage.lock();

        match mtx {
            Ok(mut queue) => {
                queue.insert(order, msg);
            }
            Err(_) => panic!("Error: Cannot lock the storage!"),
        }
    }

    fn send_pending(&self) {
        let mtx = self.storage.lock();

        match mtx {
            Ok(mut queue) => {
                let mut c = self.next_msg.load(Ordering::SeqCst);
                while queue.contains_key(&c) {
                    let msg = queue.remove(&c).unwrap();
                    let Message { op, order } = msg;
                    match &op {
                        Task::NewTask(_e) => {
                            let err = self.send(Message::new(op, c), order);
                            if err.is_err() {
                                panic!("Error: Cannot send message!");
                            }
                        }
                        Task::Dropped => {
                            let err = self.send(Message::new(op, c), order);
                            if err.is_err() {
                                panic!("Error: Cannot send message!");
                            }
                        }
                        Task::Terminate => {
                            let err = self.send(Message::new(op, c), 0);
                            if err.is_err() {
                                panic!("Error: Cannot send message!");
                            }
                        }
                    }
                    c = self.next_msg.load(Ordering::SeqCst);
                }
            }
            Err(_) => panic!("Error: Cannot lock the storage!"),
        }
    }
}
