use std::{collections::HashMap, hash::Hash, sync::Arc};

/// Least-recently-used cache bounded by the bytes it holds.
pub struct ByteCache<K> {
    entries: HashMap<K, Entry>,
    bytes: usize,
    max_bytes: usize,
    clock: u64,
}

struct Entry {
    data: Arc<[u8]>,
    used_at: u64,
}

impl<K: Hash + Eq + Clone> ByteCache<K> {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            max_bytes,
            clock: 0,
        }
    }

    pub fn get(&mut self, key: &K) -> Option<Arc<[u8]>> {
        self.clock += 1;
        let entry = self.entries.get_mut(key)?;
        entry.used_at = self.clock;
        Some(entry.data.clone())
    }

    pub fn insert(&mut self, key: K, data: Arc<[u8]>) {
        self.clock += 1;
        self.bytes += data.len();
        let entry = Entry {
            data,
            used_at: self.clock,
        };
        if let Some(replaced) = self.entries.insert(key, entry) {
            self.bytes -= replaced.data.len();
        }
        while self.bytes > self.max_bytes && self.entries.len() > 1 {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used_at)
                .map(|(key, _)| key.clone())
                .expect("the cache holds entries");
            self.bytes -= self.entries.remove(&oldest).expect("key exists").data.len();
        }
    }
}
