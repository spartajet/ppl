use flume::{Receiver, Sender, TryRecvError};

use super::{
    channel,
    err::{ReceiverError, SenderError},
};

pub struct FlumeInputChannel<T> {
    rx: Receiver<T>,
}
impl<T: Send> channel::Receiver<T> for FlumeInputChannel<T> {
    fn receive(&self) -> Result<Option<T>, ReceiverError> {
        let err = self.rx.try_recv();
        match err {
            Ok(msg) => Ok(Some(msg)),
            Err(e) => match e {
                TryRecvError::Empty => Ok(None),
                TryRecvError::Disconnected => Err(ReceiverError),
            },
        }
    }

    fn is_empty(&self) -> bool {
        self.rx.is_empty()
    }
}

pub struct FlumeBlockingInputChannel<T> {
    rx: Receiver<T>,
}
impl<T: Send> channel::Receiver<T> for FlumeBlockingInputChannel<T> {
    fn receive(&self) -> Result<Option<T>, ReceiverError> {
        let err = self.rx.recv();
        match err {
            Ok(msg) => Ok(Some(msg)),
            Err(_e) => Err(ReceiverError),
        }
    }

    fn is_empty(&self) -> bool {
        self.rx.is_empty()
    }
}

pub struct FlumeOutputChannel<T> {
    tx: Sender<T>,
}

impl<T: Send> channel::Sender<T> for FlumeOutputChannel<T> {
    fn send(&self, msg: T) -> Result<(), SenderError> {
        let err = self.tx.send(msg);
        match err {
            Ok(()) => Ok(()),
            Err(_e) => Err(SenderError),
        }
    }
}
impl<T> Clone for FlumeOutputChannel<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

/// Channel is a factory for creating new channels.
/// It is a wrapper around the flume channel.
pub struct Channel;

impl Channel {
    pub fn channel<T: Send + 'static>(
        blocking: bool,
    ) -> (
        Box<dyn channel::Receiver<T> + Sync + Send>,
        Box<dyn channel::Sender<T> + Sync + Send>,
    ) {
        let (tx, rx) = flume::unbounded();
        if blocking {
            (
                Box::new(FlumeBlockingInputChannel { rx }),
                Box::new(FlumeOutputChannel { tx }),
            )
        } else {
            (
                Box::new(FlumeInputChannel { rx }),
                Box::new(FlumeOutputChannel { tx }),
            )
        }
    }
}
