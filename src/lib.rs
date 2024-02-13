mod sleep;
//mod tasks;
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

use crate::sleep::Sleepers;

// Parts of the waker that need to live in a refcell
#[derive(Debug)]
struct RuntimeState {
    sleepers: Sleepers,
    poll: mio::Poll,
    io_wakers: Slab<Waker>,
}

#[derive(Debug)]
pub struct Runtime {
    state: RefCell<RuntimeState>,
    waker: Waker,
}

pub trait Task {
    type Output;

    fn start(self, runtime: &Runtime) -> impl Future<Output = Self::Output> + '_;
}

const BUTLER_WAKER_TOKEN: mio::Token = mio::Token(0);
const EXTERNAL_WAKER_BASE: usize = 1;

pub fn run<T: Task>(task: T) -> io::Result<T::Output> {
    let mut poll = mio::Poll::new()?;
    let mut events = mio::Events::with_capacity(4096);

    let waker = Waker::from(Arc::new(ButlerWaker {
        waker: mio::Waker::new(poll.registry(), BUTLER_WAKER_TOKEN)?,
    }));

    let mut runtime = Runtime {
        state: RefCell::new(RuntimeState {
            sleepers: Sleepers::new(),
            io_wakers: Slab::new(),
            poll,
        }),
        waker,
    };

    let mut context = Context::from_waker(&runtime.waker);

    let future = task.start(&runtime);
    let mut future = pin!(future);

    loop {
        if let Poll::Ready(out) = future.as_mut().poll(&mut context) {
            break Ok(out);
        }

        let mut state = runtime.state.borrow_mut();
        let mut idle = true;

        while idle {
            let now = Instant::now();
            let deadline = state.sleepers.get_next_wakeup(Instant::now());
            let timeout =
                deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));

            match state.poll.poll(&mut events, timeout) {
                Ok(_) => {}
                Err(err) if err.kind() == io::ErrorKind::TimedOut => {}
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) => return Err(err),
            }

            // `Poll::poll` doesn't provide any convenient ways to detect if a
            // timeout occurred. We could manually check now, but we instead assume
            // that if a timeout occurred, no events will be enqueued and
            // `idle` will remain true, so we'll immediately be looping back to
            // `get_next_wakeup`, which will trigger any needed timeouts.

            events.iter().for_each(|event| {
                let token = event.token();

                if token == BUTLER_WAKER_TOKEN {
                    idle = true;
                } else {
                    let idx = token.0 - EXTERNAL_WAKER_BASE;

                    if let Some(waker) = state.io_wakers.get(idx) {
                        waker.wake_by_ref();
                    } else {
                        // TODO: warning
                    }
                }
            });
        }
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
