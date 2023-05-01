/*
   Fibonacci farm
   Is generated a sequence of i from 1 to 45.
   Each worker of the farm compute the i-th
   Fibonacci number.
*/

use pspp::core::orchestrator::get_global_orchestrator;
use pspp::{
    node::{
        in_node::{In, InNode},
        inout_node::{InOut, InOutNode},
        out_node::{Out, OutNode},
    },
    parallel, propagate,
    pspp::Parallel,
};

struct Source {
    streamlen: usize,
}
impl Out<i32> for Source {
    fn run(&mut self) -> Option<i32> {
        let mut ret = None;
        if self.streamlen > 0 {
            ret = Some(self.streamlen as i32);
            self.streamlen = self.streamlen - 1;
        }
        ret
    }
}

pub fn fibonacci_reccursive(n: i32) -> u64 {
    if n < 0 {
        panic!("{} is negative!", n);
    }
    match n {
        0 => panic!("zero is not a right argument to fibonacci_reccursive()!"),
        1 | 2 => 1,
        3 => 2,
        /*
        50    => 12586269025,
        */
        _ => fibonacci_reccursive(n - 1) + fibonacci_reccursive(n - 2),
    }
}

#[derive(Clone)]
struct WorkerA {}
impl InOut<i32, u64> for WorkerA {
    fn run(&mut self, input: i32) -> Option<u64> {
        Some(fibonacci_reccursive(input))
    }
    fn number_of_replicas(&self) -> usize {
        8
    }
}

struct Sink {
    counter: usize,
}
impl In<u64, usize> for Sink {
    fn run(&mut self, input: u64) {
        println!("{}", input);
        self.counter += 1;
    }

    fn finalize(self) -> Option<usize> {
        println!("End");
        Some(self.counter)
    }
}

#[test]
fn test_farm() {
    env_logger::init();

    let mut p = parallel![Source { streamlen: 45 }, WorkerA {}, Sink { counter: 0 }];
    p.start();
    let res = p.wait_and_collect();
    assert_eq!(res.unwrap(), 45);
}
