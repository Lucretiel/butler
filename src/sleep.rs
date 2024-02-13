use std::{
    borrow::Borrow,
    collections::{binary_heap::PeekMut, BinaryHeap},
    future::Future,
    mem,
    pin::Pin,
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

use slab::Slab;

use crate::Runtime;

#[derive(Debug)]
struct Timer {
    deadline: Instant,
    key: usize,
}

impl PartialEq for Timer {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline
    }
}

impl Eq for Timer {}

impl PartialOrd for Timer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(Ord::cmp(self, other))
    }
}

impl Ord for Timer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        Ord::cmp(&other.deadline, &self.deadline)
    }
}

#[derive(Debug)]
enum EntryState {
    Pending(Waker),
    Done,
}

#[derive(Debug)]
struct Entry {
    deadline: Instant,
    state: EntryState,
}

#[derive(Debug)]
pub struct Sleepers {
    timers: BinaryHeap<Timer>,
    entries: Slab<Entry>,
}

impl Sleepers {
    pub fn new() -> Self {
        Self {
            timers: BinaryHeap::new(),
            entries: Slab::new(),
        }
    }

    /// For the event loop: get the next timer in the sequence. This function
    /// will handle a handful of things related to timers:
    ///
    /// - Find the next timer, soonest in the future
    /// - Any timers that are present or in the past will be triggered
    ///   immediately.
    /// - Any deleted timers in the near future will be cleaned up
    pub fn get_next_wakeup(&mut self, now: Instant) -> Option<Instant> {
        loop {
            let timer = self.timers.peek_mut()?;
            let key = timer.key;

            if let Some(entry) = self.entries.get_mut(key) {
                // If there is an entry, but the deadline is different. This
                // means that this entry is actually associated with a
                // different timer; this timer is dead, so we ignore it.
                if entry.deadline != timer.deadline {
                }
                // If there is an entry, and it's in the future, use it as the
                // next wakeup.
                else if timer.deadline > now {
                    break Some(timer.deadline);
                }
                // There is a valid entry in the past. Trigger it immediately
                // and move on to the next one.
                else if let EntryState::Pending(waker) =
                    mem::replace(&mut entry.state, EntryState::Done)
                {
                    waker.wake();
                }
            }

            PeekMut::pop(timer);
        }
    }
}

enum SleepState {
    Pending(Instant),
    Polled(usize),
}

pub struct Sleep<'a> {
    runtime: &'a Runtime,
    state: SleepState,
}

impl Sleep<'_> {
    pub fn deadline(&self) -> Instant {
        match self.state {
            SleepState::Pending(deadline) => deadline,
            SleepState::Polled(key) => {
                self.runtime
                    .state
                    .borrow()
                    .sleepers
                    .entries
                    .get(key)
                    .expect("entry shouldn't be removed while Sleep exists")
                    .deadline
            }
        }
    }
}

impl Future for Sleep<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let waker = cx.waker();
        let this = self.get_mut();
        let mut runtime = this.runtime.state.borrow_mut();
        let sleepers = &mut runtime.sleepers;

        match this.state {
            SleepState::Pending(deadline) => {
                let key = sleepers.entries.insert(Entry {
                    deadline,
                    state: EntryState::Pending(waker.clone()),
                });
                sleepers.timers.push(Timer { deadline, key });

                this.state = SleepState::Polled(key);
                Poll::Pending
            }

            SleepState::Polled(key) => {
                let entry = sleepers
                    .entries
                    .get_mut(key)
                    .expect("entry shouldn't be removed while Sleep exists");

                match entry.state {
                    EntryState::Pending(ref mut old_waker) => {
                        old_waker.clone_from(waker);
                        Poll::Pending
                    }

                    EntryState::Done => {
                        sleepers.entries.remove(key);
                        Poll::Ready(())
                    }
                }
            }
        }
    }
}

impl Drop for Sleep<'_> {
    fn drop(&mut self) {
        match self.state {
            SleepState::Pending(_) => {}
            SleepState::Polled(key) => {
                let mut runtime = self.runtime.state.borrow_mut();
                runtime.sleepers.entries.remove(key);
            }
        }
    }
}

impl Runtime {
    pub fn sleep_until(&self, deadline: Instant) -> Sleep<'_> {
        Sleep {
            runtime: self,
            state: SleepState::Pending(deadline),
        }
    }

    pub fn sleep(&self, duration: Duration) -> Sleep<'_> {
        self.sleep_until(Instant::now() + duration)
    }
}
