#![allow(dead_code)]

use futures::stream::Stream;
use std::sync::Mutex;
use std::{
    collections::VecDeque,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll, Waker},
};

// handle and status for a stream receiver task
#[derive(Debug)]
struct ReceiverWaker {
    handle: Waker,
    awake: Arc<AtomicBool>,
}

// holds inner VecDeque as well as the notification buffer
// letting streams know when polling is ready
struct RawDeque<T> {
    values: VecDeque<T>,
    rx_wakers: VecDeque<ReceiverWaker>,
}

impl<T> RawDeque<T> {
    fn new() -> Self {
        Self {
            values: VecDeque::new(),
            rx_wakers: VecDeque::new(),
        }
    }
}

impl<T> RawDeque<T> {
    // waker first receiver to poll for values
    fn notify_rx(&mut self) {
        if let Some(w) = self.rx_wakers.pop_front() {
            w.handle.wake();
            w.awake.store(true, Ordering::Relaxed);
        }
    }
}

/// This type acts similarly to std::collections::VecDeque but
/// modifying queue is async
pub struct StreamableDeque<T> {
    inner: Mutex<RawDeque<T>>,
}

impl<T> Default for StreamableDeque<T> {
    fn default() -> Self {
        Self {
            inner: Mutex::new(RawDeque::new()),
        }
    }
}

impl<T> StreamableDeque<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push an item into the queue and notify first receiver
    pub fn push_front(&self, item: T) {
        let mut inner = self.inner.lock().unwrap();
        inner.values.push_front(item);
        // Notify first receiver in queue
        inner.notify_rx();
    }

    /// Push an item into the back of the queue and notify first receiver
    pub fn push_back(&self, item: T) {
        let mut inner = self.inner.lock().unwrap();
        inner.values.push_back(item);
        // Notify first receiver in queue
        inner.notify_rx();
    }

    pub fn pop_front(&self) -> Option<T> {
        self.inner.lock().unwrap().values.pop_front()
    }

    pub async fn pop_back(&self) -> Option<T> {
        self.inner.lock().unwrap().values.pop_back()
    }

    /// Returns a stream of items using pop_front()
    /// This opens us up to handle a back_stream() as well
    pub fn stream(&self) -> StreamReceiver<T> {
        StreamReceiver {
            queue: self,
            awake: None,
        }
    }
}

/// A stream of items removed from the priority queue.
pub struct StreamReceiver<'a, T> {
    queue: &'a StreamableDeque<T>,
    awake: Option<Arc<AtomicBool>>,
}

impl<'a, T> Stream for StreamReceiver<'a, T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut inner = self.queue.inner.lock().unwrap();

        match inner.values.pop_front() {
            Some(entry) => {
                self.awake = None;
                Poll::Ready(Some(entry))
            }
            // if queue has no entries
            None => {
                // TODO avoid allocation of a new AtomicBool if possible
                let awake = Arc::new(AtomicBool::new(false));
                // push stream's waker onto buffer
                inner.rx_wakers.push_back(ReceiverWaker {
                    handle: cx.waker().clone(),
                    awake: awake.clone(),
                });
                self.awake = Some(awake);
                Poll::Pending
            }
        }
    }
}

impl<'a, T> Drop for StreamReceiver<'a, T> {
    // if a stream gets dropped, remove itself from the notification queue
    fn drop(&mut self) {
        let awake = self
            .awake
            .take()
            .map_or(false, |w| w.load(Ordering::Relaxed));

        if awake {
            let mut queue_wakers = self.queue.inner.lock().unwrap();
            // StreamReceiver was woken by a None, notify another
            if let Some(w) = queue_wakers.rx_wakers.pop_front() {
                w.awake.store(true, Ordering::Relaxed);
                w.handle.wake();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use futures::stream::StreamExt;

    #[tokio::test]
    async fn priority() {
        let queue = Arc::new(StreamableDeque::<i32>::new());

        let pos_queue = queue.clone();
        tokio::spawn(async move {
            for i in 0..=1000 {
                pos_queue.push_back(i);
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });

        let neg_queue = queue.clone();
        tokio::spawn(async move {
            for i in (-1000..=-1).rev() {
                neg_queue.push_back(i);
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });

        let mut rx_vec = vec![];

        let mut stream = queue.stream().enumerate();
        loop {
            if let Some((i, v)) = stream.next().await {
                rx_vec.push(v);
                if i >= 100 {
                    break;
                }
            }
        }

        // we should guarantee that positive and negative numbers have been pushed out of order
        // but push_front and push_back should guarantee that they are sorted
        let expected_vec: Vec<i32> = (-10..=10).collect();
        assert_eq!(expected_vec, rx_vec);
    }
}

fn main() {}

