//! `LruCache` of `bufferCache.ts`: `get` refreshes an entry, `set` evicts the least recently used
//! entries beyond `capacity`. Small capacities only (linear scans).

#[derive(Debug, Clone)]
pub struct LruCache<K, V> {
    capacity: usize,
    /// Oldest first.
    entries: Vec<(K, V)>,
}

impl<K: PartialEq + Clone, V: Clone> LruCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: Vec::with_capacity(capacity + 1),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn get(&mut self, key: &K) -> Option<V> {
        let i = self.entries.iter().position(|(k, _)| k == key)?;
        let e = self.entries.remove(i);
        let v = e.1.clone();
        self.entries.push(e);
        Some(v)
    }

    pub fn set(&mut self, key: K, value: V) {
        self.delete(&key);
        self.entries.push((key, value));
        while self.entries.len() > self.capacity {
            self.entries.remove(0);
        }
    }

    pub fn delete(&mut self, key: &K) {
        self.entries.retain(|(k, _)| k != key);
    }

    pub fn has(&self, key: &K) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    /// Keys, least recently used first.
    pub fn keys(&self) -> Vec<K> {
        self.entries.iter().map(|(k, _)| k.clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
