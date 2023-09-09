use std::sync::{Arc, Mutex};

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
    let tp = ThreadPool::new();
    for i in 1..45 {
        tp.execute(move || {
            fib(i);
        });
    }
    tp.wait();
    unsafe{ Orchestrator::delete_global_orchestrator(); }
}

#[test]
#[serial]
fn test_scoped_thread() {
    let mut vec = vec![0; 100];
    let mut tp = ThreadPool::new();

    tp.scope(|s| {
        for e in vec.iter_mut() {
            s.execute(move || {
                *e += 1;
            });
        }
    });
    unsafe{ Orchestrator::delete_global_orchestrator(); }
}

#[test]
#[serial]
fn test_par_for_each() {
    let mut vec = vec![0; 100];
    let mut tp = ThreadPool::new();

    tp.par_for_each(&mut vec, |el: &mut i32| *el += 1);
    unsafe{ Orchestrator::delete_global_orchestrator(); }
    assert_eq!(vec, vec![1i32; 100])
}

#[test]
#[serial]
fn test_par_map() {
    let mut vec = Vec::new();
    let mut tp = ThreadPool::new();

    for i in 0..10000 {
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
    unsafe{ Orchestrator::delete_global_orchestrator(); }
    assert!(check)
}

#[test]
#[serial]
fn test_par_for() {
    let mut tp = ThreadPool::new();

    let vec = {
        let mut v = Vec::with_capacity(100);
        (0..100).for_each(|_| v.push(Arc::new(Mutex::new(0))));
        v
    };

    tp.par_for(0..100, 2, |i| {
        let mut lock = vec[i].lock().unwrap();
        *lock += 1;
    });

    let mut check = true;

    (0..100).for_each(|i| {
        let lock = vec[i].lock().unwrap();
        if *lock != 1 {
            check = false;
        }
    });

    unsafe{ Orchestrator::delete_global_orchestrator(); }
    assert!(check)
}

// Test par_map_reduce
#[test]
#[serial]
fn test_par_map_reduce() {
    let mut vec = Vec::new();
    let mut tp = ThreadPool::new();

    for _i in 0..100000 {
        for i in 0..10 {
            vec.push(i);
        }
    }

    let res = tp.par_map_reduce(
        vec,
        |el| -> (i32, i32) { (el, 1) },
        |k, v| (k, v.iter().sum::<i32>()),
    );

    let mut check = true;
    for (k, v) in res {
        if v != 100000 {
            check = false;
        }
        println!("Key: {} Total: {}", k, v)
    }

    unsafe{ Orchestrator::delete_global_orchestrator(); }
    assert!(check)
}

#[test]
#[serial]
fn test_par_map_reduce_seq() {
    let mut vec = Vec::new();
    let mut tp = ThreadPool::new();

    for _i in 0..100000 {
        for i in 0..10 {
            vec.push(i);
        }
    }

    let res = tp.par_map(vec, |el| -> (i32, i32) { (el, 1) });
    let res = tp.par_reduce(res, |k, v| (k, v.iter().sum::<i32>()));

    let mut check = true;
    for (k, v) in res {
        if v != 100000 {
            check = false;
        }
        println!("Key: {} Total: {}", k, v)
    }

    unsafe{ Orchestrator::delete_global_orchestrator(); }
    assert!(check)
}

#[test]
#[serial]
fn test_multiple_threadpool() {
    let tp_1 = ThreadPool::new();
    let tp_2 = ThreadPool::new();
    ::scopeguard::defer! {
        tp_1.wait();
        tp_2.wait();

    }
    unsafe{ Orchestrator::delete_global_orchestrator(); }
}

fn square(x: f64) -> f64 {
    x * x
}

#[test]
#[serial]
fn test_simple_map() {
    let mut pool = ThreadPool::new(); // Create a new threadpool
    let mut counter = 1.0;
    let mut numbers: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];

    // Transform the vec of integers into a vec of strings
    let res: Vec<f64> = pool.par_map(&mut numbers, |el| square(*el)).collect();

    for el in res {
        assert_eq!(el.sqrt(), counter);
        counter += 1.0;
    }

    unsafe{ Orchestrator::delete_global_orchestrator();}
}
