use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    ptr::{self, NonNull},
    sync::{
        atomic::{AtomicPtr, AtomicU8, Ordering},
        Arc, Weak,
    },
    task::{Context, Poll, Waker},
};

use cooked_waker::IntoWaker;
use futures_core::{FusedFuture, Stream};
use futures_util::{future::Fuse, task::AtomicWaker, FutureExt};
use pin_project::pin_project;

const IDLE: u8 = 0;
const READY: u8 = 1;

enum UntypedNode {}

struct NodeMeta {
    node: AtomicPtr<UntypedNode>,
    ready_queue_head: Arc<ReadyQueueHead>,
    next_ready: AtomicPtr<NodeMeta>,
    waker: AtomicWaker,
}

impl NodeMeta {
    fn set_node_ptr<F>(&mut self, node: NonNull<Node<F>>) {
        *self.node.get_mut() = node.cast().as_ptr();
    }

    fn clear_node_ptr(&self) {
        self.node.store(ptr::null_mut(), Ordering::Relaxed);
    }
}

unsafe impl Send for NodeMeta {}
unsafe impl Sync for NodeMeta {}

impl cooked_waker::WakeRef for NodeMeta {
    fn wake_by_ref(&self) {
        let Some(waker) = self.waker.take() else {
            return;
        };

        self.ready_queue_head.enqueue_node(self);
        waker.wake();
    }
}

struct Node<F> {
    future: F,
    meta: Arc<NodeMeta>,
    next_node: Option<Box<Node<F>>>,
    prev_node: Option<NonNull<Node<F>>>,
}

impl<F> Node<F> {
    fn make_waker(&self) -> Waker {
        Arc::downgrade(&self.meta).into_waker()
    }
}

struct ReadyQueueHead {
    next_ready: AtomicPtr<NodeMeta>,
}

impl ReadyQueueHead {
    fn enqueue_node(&self, node: &NodeMeta) {
        let node_ptr = NonNull::from(node);
        let mut current_head = self.next_ready.load(Ordering::Acquire);

        loop {
            node.next_ready.store(current_head, Ordering::Relaxed);

            match self.next_ready.compare_exchange_weak(
                current_head,
                node_ptr.as_ptr(),
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(..) => break,
                Err(c) => current_head = c,
            };
        }
    }
}

pub struct FutureSet<F> {
    live_queue: Option<NonNull<NodeMeta>>,
    ready_queue_head: Arc<ReadyQueueHead>,

    node_list: Option<Box<Node<F>>>,
}

impl<F: Future> FutureSet<F> {
    pub fn insert(&mut self, future: F) {
        let next_node = self.node_list.take();

        let meta = Arc::new(NodeMeta {
            node: AtomicPtr::new(ptr::null_mut()),
            ready_queue_head: self.ready_queue_head.clone(),
            next_ready: AtomicPtr::new(ptr::null_mut()),
            waker: AtomicWaker::new(),
        });

        let mut node = Box::new(Node {
            future,
            meta,
            next_node,
            prev_node: None,
        });

        let node_ptr = NonNull::new(&mut *node).expect("reference is never null");

        // Create the reverse pointer from meta to node
        let meta_node_ptr = Arc::get_mut(&mut node.meta)
            .expect("New meta has no shared references")
            .node
            .get_mut();

        *meta_node_ptr = node_ptr.cast().as_ptr();

        // Create the doubly-linked list linkage
        if let Some(mut next_node) = next_node {
            let next_node = unsafe { next_node.as_mut() };
            next_node.prev_node = Some(node_ptr);
        }

        // Add this node to the list
        self.node_list = Some(node);

        self.ready_queue_head.enqueue_node(&meta);
    }

    pub fn poll_tasks(&mut self, cx: &mut Context<'_>) -> Poll<Option<F::Output>> {
        if self.live_queue.is_none() {
            // Take a snapshot of the ready_queue. This ensures that any new
            // readiness that occurs while we're polling tasks can't cause an
            // infinite loop.
            let ready_queue = self
                .ready_queue_head
                .next_ready
                .swap(ptr::null_mut(), Ordering::Acquire);

            self.live_queue = NonNull::new(ready_queue);
        } else if abc {
        };

        let mut needs_ready_queue = true;

        loop {
            if let Some(meta_ptr) = self.live_queue.take() {
                let meta: &NodeMeta = unsafe { meta_ptr.as_ref() };
                let next_meta = meta.next_ready.swap(ptr::null_mut(), Ordering::Acquire);
                self.live_queue = NonNull::new(next_meta);

                // It is now safe to re-enqueue this node if it gets awoken while
                // we're working on it.
                meta.waker.register(cx.waker());

                // Now that the node has been extracted, actually do the work of
                // polling it
                let node_ptr = meta.node;
                let node_ptr: NonNull<Node<F>> = node_ptr.cast();
                let node: &mut Node<F> = unsafe { node_ptr.as_mut() };

                let node_waker = node.make_waker();
                let mut context = Context::from_waker(&node_waker);

                let future = unsafe { Pin::new_unchecked(&mut node.future) };

                if let Poll::Ready(value) = future.poll(&mut context) {
                    meta.node.store(ptr::null_mut(), Ordering::Relaxed);

                    let next_node = node.next_node.take();
                    let prev_node = node.prev_node.take();
                };
            }
        }

        if !self
            .ready_queue_head
            .next_ready
            .load(Ordering::Relaxed)
            .is_null()
        {
            cx.waker().wake_by_ref();
        }

        Poll::Pending
    }
}

impl<F: Future> Stream for FutureSet<F> {
    type Item = F::Output;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        todo!()
    }
}
