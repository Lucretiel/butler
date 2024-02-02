mod sleep;
mod tcp;

use std::cell::RefCell;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake};
use std::time::Duration;
use std::{future::Future, io};

use std::{
    collections::{binary_heap::PeekMut, BinaryHeap},
    task::Waker,
    time::Instant,
};

use slab::Slab;

use crate::sleep::{Sleep, Sleepers};

pub struct Runtime {
    sleepers: RefCell<Sleepers>,
    waker: Waker,
    poll: mio::Poll,
}

pub trait Task {
    type Output;

    fn start(self, runtime: &Runtime) -> impl Future<Output = Self::Output> + '_;
}

const BUTLER_WAKER_TOKEN: mio::Token = mio::Token(10_000_000_000);

pub fn run<T: Task>(task: T) -> io::Result<T::Output> {
    let mut poll = mio::Poll::new()?;
    let mut events = mio::Events::with_capacity(4096);

    let waker = Waker::from(Arc::new(ButlerWaker {
        waker: mio::Waker::new(poll.registry(), BUTLER_WAKER_TOKEN)?,
    }));

    let mut runtime = Runtime {
        sleepers: RefCell::new(Sleepers::new()),
        waker,
        poll,
    };

    let mut context = Context::from_waker(&runtime.waker);

    let future = task.start(&runtime);
    let mut future = pin!(future);

    loop {
        if let Poll::Ready(out) = future.as_mut().poll(&mut context) {
            break Ok(out);
        }

        let mut sleepers = runtime.sleepers.borrow_mut();
        let deadline = sleepers.get_next_wakeup(Instant::now());
        let timeout = deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));

        match runtime.poll.poll(&mut events, timeout) {
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }

        for event in events.iter() {}
    }
}

fn todo<T>() -> T {
    panic!()
}

struct ButlerWaker {
    waker: mio::Waker,
}

impl Wake for ButlerWaker {
    fn wake(self: Arc<Self>) {
        let _ = self.waker.wake();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let _ = self.waker.wake();
    }
}
