//! Work that is ready, in order of arrival.

use std::collections::VecDeque;

pub(crate) struct ReadyQueue<T> {
    queue: VecDeque<T>,
}

impl<T> Default for ReadyQueue<T> {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }
}

impl<T> ReadyQueue<T> {
    pub fn push(&mut self, item: T) {
        self.queue.push_back(item);
    }

    pub fn pop(&mut self) -> Option<T> {
        self.queue.pop_front()
    }
}
