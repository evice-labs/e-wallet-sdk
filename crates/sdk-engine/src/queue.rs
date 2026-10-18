//! Transaction queue.
//!
//! The [`TxQueue`] serializes all transaction submissions from the engine.
//! No two transactions execute concurrently — this prevents nonce conflicts
//! and state races across multiple dApp sessions.

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};

use crate::request::RequestId;

/// A queued transaction entry ready for submission.
#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub request_id: RequestId,
    pub priority: u8,
}

/// Transaction queue — serializes submissions to avoid nonce/state conflicts.
///
/// Internally backed by a FIFO queue with a single consumer.
/// The queue holds entries; the actual execution is performed by the engine
/// when it dequeues each entry.
#[derive(Debug)]
pub struct TxQueue {
    queue: Arc<Mutex<VecDeque<QueueEntry>>>,
    notify: Arc<Notify>,
}

impl TxQueue {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Enqueue a transaction for submission.
    pub async fn enqueue(&self, entry: QueueEntry) {
        let mut q = self.queue.lock().await;
        q.push_back(entry);
        self.notify.notify_one();
    }

    /// Dequeue the next entry, waiting if the queue is empty.
    pub async fn dequeue(&self) -> QueueEntry {
        loop {
            {
                let mut q = self.queue.lock().await;
                if let Some(entry) = q.pop_front() {
                    return entry;
                }
            }
            self.notify.notified().await;
        }
    }

    /// Check how many entries are in the queue.
    pub async fn len(&self) -> usize {
        let q = self.queue.lock().await;
        q.len()
    }

    /// Check whether the queue is empty.
    pub async fn is_empty(&self) -> bool {
        let q = self.queue.lock().await;
        q.is_empty()
    }
}

impl Default for TxQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn enqueue_dequeue_fifo() {
        let queue = TxQueue::new();
        let e1 = QueueEntry {
            request_id: RequestId("r1".into()),
            priority: 0,
        };
        let e2 = QueueEntry {
            request_id: RequestId("r2".into()),
            priority: 0,
        };

        queue.enqueue(e1).await;
        queue.enqueue(e2).await;
        assert_eq!(queue.len().await, 2);

        let first = queue.dequeue().await;
        assert_eq!(first.request_id.0, "r1");
        let second = queue.dequeue().await;
        assert_eq!(second.request_id.0, "r2");
        assert!(queue.is_empty().await);
    }
}
