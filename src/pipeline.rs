use crate::node::{node::Node, out_node::*, out_monode::*};

pub struct Pipeline<TOut: Send, TCollected, TNext: Node<TOut, TCollected>> {
    first_block: OutNode<TOut, TCollected, TNext>,
}
impl<TOut: Send + 'static, TCollected, TNext: Node<TOut, TCollected> + Send + Sync + 'static>
    Pipeline<TOut, TCollected, TNext>
{
    pub fn new(first_block: OutNode<TOut, TCollected, TNext>) -> Pipeline<TOut, TCollected, TNext> {
        Pipeline { first_block }
    }
    pub fn start(&mut self) {
        let err = self.first_block.start();
        if err.is_err() {
            panic!("Error: Cannot start thread!");
        }
    }

    pub fn collect(self) -> Option<TCollected> {
        Node::<TOut, TCollected>::collect(self.first_block)
    }
}


#[macro_export]
macro_rules! pipeline_propagate {
    ($s1:expr) => {
        {
            let mut block = InNode::new(0, $s1, false).unwrap();
            block
        }
    };

    ($s1:expr $(, $tail:expr)*) => {
        {
            let mut block = InOutNode::new(0, $s1,
                pipeline_propagate!($($tail),*),
                false).unwrap();
            block
        }
    };
}

#[macro_export]
macro_rules! pipeline {
    ($s1:expr $(, $tail:expr)*) => {
        {
            let mut block = OutNode::new(0, $s1,
                pipeline_propagate!($($tail),*)).unwrap();

            let mut pipeline = Pipeline::new(block);
            pipeline.start();
            pipeline
        }
    };
}

pub struct Parallel<TOut: Send, TCollected, TNext: Node<TOut, TCollected>> {
    first_block: OutMoNode<TOut, TCollected, TNext>,
}
impl<TOut: Send + 'static, TCollected, TNext: Node<TOut, TCollected> + Send + Sync + 'static>
    Parallel<TOut, TCollected, TNext>
{
    pub fn new(first_block: OutMoNode<TOut, TCollected, TNext>) -> Parallel<TOut, TCollected, TNext> {
        Parallel { first_block }
    }
    pub fn start(&mut self) {
        let err = self.first_block.start();
        if err.is_err() {
            panic!("Error: Cannot start thread!");
        }
    }

    pub fn collect(self) -> Option<TCollected> {
        Node::<TOut, TCollected>::collect(self.first_block)
    }
}



#[macro_export]
macro_rules! parallel_propagate {
    ($s1:expr) => {
        {
            let mut block = InNode::new(0, $s1, false).unwrap();
            vec![block]
        }
    };
    ($s1:expr $(, $tail:expr)*) => {
        {
            let (logic, threads) = $s1;
            let mut blocks = Vec::new();
            let next_stages = parallel_propagate!($($tail),*);
            

            for i in 0..threads {
                let mut block = InOutMoNode::new(0, logic.clone(),
                    next_stages,
                    false).unwrap();
                blocks.push(block);
            }
            blocks
        }
    };



}

#[macro_export]
macro_rules! parallel {
    ($s1:expr $(, $tail:expr)*) => {
        {
            let mut block = OutMoNode::new(0, $s1,
                parallel_propagate!($($tail),*)).unwrap();
            let mut p = Parallel::new(block);
            p.start();
            p
        }
    };
}
