//! Idle-task queue for deferred GUI work.
//!
//! Extracted from `lib.rs` (GUI-LIB-SPLIT-65 Slice 2, file-size ratchet).
//! Pure data structure: bounded priority queue with stable cancellation handles.

use std::path::PathBuf;

/// Work which may be performed when the GUI has no interactive input.
///
/// Queueing is deliberately separate from mask status: a missing/pending mask
/// is never inserted here implicitly.  The caller must enqueue it as the
/// result of an explicit user action (or a future CLI/GUI command).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdleTask {
    MaskInference { mask_id: String },
    Thumbnail { source: PathBuf, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QueuedIdleTask {
    id: u64,
    priority: u8,
    task: IdleTask,
}

/// Small, bounded priority queue for work that is safe to defer until idle.
#[derive(Debug, Clone)]
pub struct IdleQueue {
    capacity: usize,
    next_id: u64,
    tasks: Vec<QueuedIdleTask>,
}

impl IdleQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            next_id: 0,
            tasks: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Maximum number of queued tasks before [`enqueue`](Self::enqueue) starts
    /// dropping jobs.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Enqueue only explicit requests. Returns a stable cancellation handle.
    pub fn enqueue(&mut self, task: IdleTask, priority: u8) -> Option<u64> {
        if self.tasks.len() >= self.capacity {
            return None;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.tasks.push(QueuedIdleTask { id, priority, task });
        Some(id)
    }

    pub fn cancel(&mut self, id: u64) -> bool {
        let before = self.tasks.len();
        self.tasks.retain(|task| task.id != id);
        self.tasks.len() != before
    }

    /// Takes the highest-priority task. Equal priorities retain FIFO order
    /// (REVIEW-GUI-N4): ties are broken by the monotonically increasing
    /// enqueue id, so the *first*-enqueued task of a priority class wins.
    /// (`Iterator::max_by_key` alone would pick the *last* maximum — LIFO.)
    pub fn pop_next(&mut self) -> Option<(u64, IdleTask)> {
        let index = self
            .tasks
            .iter()
            .enumerate()
            .min_by_key(|(_, task)| (std::cmp::Reverse(task.priority), task.id))?
            .0;
        let task = self.tasks.remove(index);
        Some((task.id, task.task))
    }
}
