//! Fixed-capacity overwrite-oldest ring buffer.

/// A fixed-capacity, overwrite-oldest ring buffer backed by a preallocated [`Vec`].
#[derive(Debug, Clone)]
pub struct RingBuffer<T: Copy> {
    buffer: Vec<T>,
    capacity: usize,
    head: usize,
}

impl<T: Copy> RingBuffer<T> {
    /// Creates a new `RingBuffer` with the specified fixed capacity.
    ///
    /// The backing [`Vec`] is allocated once up front and never reallocated.
    ///
    /// # Panics
    ///
    /// Panics if `capacity == 0`.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "capacity must be greater than zero");
        Self {
            buffer: Vec::with_capacity(capacity),
            capacity,
            head: 0,
        }
    }

    /// Pushes an item into the ring buffer.
    ///
    /// If the buffer is full, overwrites the oldest item and advances the head.
    pub fn push(&mut self, item: T) {
        if self.buffer.len() < self.capacity {
            self.buffer.push(item);
        } else {
            self.buffer[self.head] = item;
            self.head = (self.head + 1) % self.capacity;
        }
    }

    /// Extends the buffer with elements from a slice.
    ///
    /// If `items.len()` exceeds the capacity, only the last `capacity` items are retained.
    pub fn extend_from_slice(&mut self, items: &[T]) {
        let slice = if items.len() > self.capacity {
            &items[items.len() - self.capacity..]
        } else {
            items
        };
        for &item in slice {
            self.push(item);
        }
    }

    /// Returns the number of elements currently stored in the buffer.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns `true` if the buffer contains no elements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Returns the maximum capacity of the buffer.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns `true` if the buffer has reached its capacity.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.buffer.len() == self.capacity
    }

    /// Clears the buffer, removing all elements while retaining allocated capacity.
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.head = 0;
    }

    /// Returns the most recently pushed item, or `None` if the buffer is empty.
    #[must_use]
    pub fn latest(&self) -> Option<T> {
        if self.is_empty() {
            None
        } else {
            self.get(self.len() - 1)
        }
    }

    /// Returns the oldest item in the buffer, or `None` if the buffer is empty.
    #[must_use]
    pub fn oldest(&self) -> Option<T> {
        self.get(0)
    }

    /// Returns an element at the given logical index, where `0` is the oldest element.
    #[must_use]
    pub fn get(&self, i: usize) -> Option<T> {
        if i >= self.len() {
            None
        } else {
            let index = (self.head + i) % self.capacity;
            self.buffer.get(index).copied()
        }
    }

    /// Returns an iterator over items from oldest to newest.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = T> + ExactSizeIterator + '_ {
        Iter {
            buffer: self,
            front: 0,
            back: self.len(),
        }
    }

    /// Copies the stored items into a [`Vec`] ordered from oldest to newest.
    #[must_use]
    pub fn to_vec(&self) -> Vec<T> {
        self.iter().collect()
    }

    /// Returns an iterator yielding the last `n` items oldest-first.
    ///
    /// If `n >= self.len()`, all items are yielded. If `n == 0`, no items are yielded.
    pub fn iter_newest(&self, n: usize) -> impl Iterator<Item = T> + '_ {
        let skip = self.len().saturating_sub(n);
        self.iter().skip(skip)
    }
}

/// An iterator over the items of a [`RingBuffer`] from oldest to newest.
#[derive(Debug, Clone)]
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct Iter<'a, T: Copy> {
    buffer: &'a RingBuffer<T>,
    front: usize,
    back: usize,
}

impl<T: Copy> Iterator for Iter<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.front < self.back {
            let item = self.buffer.get(self.front);
            self.front += 1;
            item
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.back - self.front;
        (remaining, Some(remaining))
    }
}

impl<T: Copy> DoubleEndedIterator for Iter<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front < self.back {
            self.back -= 1;
            self.buffer.get(self.back)
        } else {
            None
        }
    }
}

impl<T: Copy> ExactSizeIterator for Iter<'_, T> {}

impl<T: Copy> std::iter::FusedIterator for Iter<'_, T> {}

impl<'a, T: Copy> IntoIterator for &'a RingBuffer<T> {
    type Item = T;
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            buffer: self,
            front: 0,
            back: self.len(),
        }
    }
}

impl<T: Copy + PartialEq> PartialEq for RingBuffer<T> {
    fn eq(&self, other: &Self) -> bool {
        self.capacity == other.capacity && self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl<T: Copy + Eq> Eq for RingBuffer<T> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "capacity must be greater than zero")]
    fn capacity_zero_panics() {
        let _ = RingBuffer::<i32>::with_capacity(0);
    }

    #[test]
    fn empty_buffer() {
        let rb = RingBuffer::<i32>::with_capacity(3);
        assert_eq!(rb.len(), 0);
        assert!(rb.is_empty());
        assert!(!rb.is_full());
        assert_eq!(rb.capacity(), 3);
        assert_eq!(rb.oldest(), None);
        assert_eq!(rb.latest(), None);
        assert_eq!(rb.get(0), None);
        assert_eq!(rb.to_vec(), Vec::<i32>::new());
        assert_eq!(rb.iter().collect::<Vec<_>>(), Vec::<i32>::new());
    }

    #[test]
    fn partial_fill() {
        let mut rb = RingBuffer::with_capacity(5);
        rb.push(10);
        rb.push(20);

        assert_eq!(rb.len(), 2);
        assert!(!rb.is_empty());
        assert!(!rb.is_full());
        assert_eq!(rb.oldest(), Some(10));
        assert_eq!(rb.latest(), Some(20));
        assert_eq!(rb.get(0), Some(10));
        assert_eq!(rb.get(1), Some(20));
        assert_eq!(rb.get(2), None);
        assert_eq!(rb.to_vec(), vec![10, 20]);
    }

    #[test]
    fn exact_fill() {
        let mut rb = RingBuffer::with_capacity(3);
        rb.push(1);
        rb.push(2);
        rb.push(3);

        assert_eq!(rb.len(), 3);
        assert!(rb.is_full());
        assert_eq!(rb.oldest(), Some(1));
        assert_eq!(rb.latest(), Some(3));
        assert_eq!(rb.to_vec(), vec![1, 2, 3]);
    }

    #[test]
    fn wraparound_several_times() {
        let mut rb = RingBuffer::with_capacity(3);
        for i in 1..=10 {
            rb.push(i);
        }

        assert_eq!(rb.len(), 3);
        assert!(rb.is_full());
        // Last 3 elements pushed are 8, 9, 10
        assert_eq!(rb.oldest(), Some(8));
        assert_eq!(rb.latest(), Some(10));
        assert_eq!(rb.get(0), Some(8));
        assert_eq!(rb.get(1), Some(9));
        assert_eq!(rb.get(2), Some(10));
        assert_eq!(rb.get(3), None);
        assert_eq!(rb.to_vec(), vec![8, 9, 10]);
    }

    #[test]
    fn extend_from_slice_larger_than_capacity() {
        let mut rb = RingBuffer::with_capacity(3);
        rb.push(100);
        rb.extend_from_slice(&[1, 2, 3, 4, 5]);

        assert_eq!(rb.len(), 3);
        assert_eq!(rb.to_vec(), vec![3, 4, 5]);
        assert_eq!(rb.oldest(), Some(3));
        assert_eq!(rb.latest(), Some(5));
    }

    #[test]
    fn clear_resets_buffer() {
        let mut rb = RingBuffer::with_capacity(3);
        rb.push(1);
        rb.push(2);
        rb.push(3);
        rb.push(4); // wrapped
        assert_eq!(rb.len(), 3);

        rb.clear();
        assert_eq!(rb.len(), 0);
        assert!(rb.is_empty());
        assert_eq!(rb.capacity(), 3);
        assert_eq!(rb.oldest(), None);
        assert_eq!(rb.latest(), None);

        // Can push again
        rb.push(42);
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.oldest(), Some(42));
        assert_eq!(rb.latest(), Some(42));
    }

    #[test]
    fn get_out_of_range() {
        let mut rb = RingBuffer::with_capacity(4);
        rb.push(10);
        rb.push(20);
        assert_eq!(rb.get(0), Some(10));
        assert_eq!(rb.get(1), Some(20));
        assert_eq!(rb.get(2), None);
        assert_eq!(rb.get(100), None);
    }

    #[test]
    fn iter_rev() {
        let mut rb = RingBuffer::with_capacity(4);
        rb.push(1);
        rb.push(2);
        rb.push(3);
        rb.push(4);
        rb.push(5); // [2, 3, 4, 5]

        let forward: Vec<_> = rb.iter().collect();
        assert_eq!(forward, vec![2, 3, 4, 5]);

        let backward: Vec<_> = rb.iter().rev().collect();
        assert_eq!(backward, vec![5, 4, 3, 2]);

        let mut it = rb.iter();
        assert_eq!(it.len(), 4);
        assert_eq!(it.next(), Some(2));
        assert_eq!(it.len(), 3);
        assert_eq!(it.next_back(), Some(5));
        assert_eq!(it.len(), 2);
        assert_eq!(it.next(), Some(3));
        assert_eq!(it.len(), 1);
        assert_eq!(it.next_back(), Some(4));
        assert_eq!(it.len(), 0);
        assert_eq!(it.next(), None);
        assert_eq!(it.next_back(), None);
    }

    #[test]
    fn iter_newest_test() {
        let mut rb = RingBuffer::with_capacity(5);
        for i in 1..=5 {
            rb.push(i);
        }
        // Last 3 oldest-first should be [3, 4, 5]
        let newest_3: Vec<_> = rb.iter_newest(3).collect();
        assert_eq!(newest_3, vec![3, 4, 5]);

        // n == 0
        let newest_0: Vec<_> = rb.iter_newest(0).collect();
        assert_eq!(newest_0, Vec::<i32>::new());

        // n >= len
        let newest_all: Vec<_> = rb.iter_newest(10).collect();
        assert_eq!(newest_all, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn capacity_one() {
        let mut rb = RingBuffer::with_capacity(1);
        assert_eq!(rb.len(), 0);
        assert!(rb.is_empty());
        assert!(!rb.is_full());

        rb.push(10);
        assert_eq!(rb.len(), 1);
        assert!(rb.is_full());
        assert_eq!(rb.oldest(), Some(10));
        assert_eq!(rb.latest(), Some(10));
        assert_eq!(rb.get(0), Some(10));
        assert_eq!(rb.get(1), None);
        assert_eq!(rb.to_vec(), vec![10]);

        rb.push(20);
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.oldest(), Some(20));
        assert_eq!(rb.latest(), Some(20));
        assert_eq!(rb.get(0), Some(20));
        assert_eq!(rb.to_vec(), vec![20]);

        rb.extend_from_slice(&[30, 40]);
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.oldest(), Some(40));
        assert_eq!(rb.latest(), Some(40));
    }
}
