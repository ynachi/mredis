//! Sharded Map for caching
//! This file describe a shared concurrent hashmap used as the backend storage for the cache.
//! We do not store the state for eviction. Time-based eviction is used and we perform lazy eviction.
//! If an expired key is read, this key is deleted and no value is returned to the user.
//! ...
use std::collections::BinaryHeap;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;

struct Shard {
    storage: FxHashMap<String, String>,
    eviction_state: BinaryHeap<(Instant, String)>,
}

impl Shard {
    fn new() -> Self {
        Shard {
            storage: FxHashMap::default(),
            eviction_state: BinaryHeap::new(),
        }
    }

    fn get_value_by_key(&self, key: &str) -> Option<&String> {
        self.storage.get(key)
    }

    // Add_or_update_kv add a new entry if it does not exist. Update the entry and return the old
    // one if it already exists.
    fn add_or_update_kv(&mut self, key: &str, data: &str, expiry: Instant) -> Option<String> {
        self.eviction_state.push((expiry, key.to_string()));
        self.storage.insert(key.to_string(), data.to_string())
    }

    fn del_entry(&mut self, key: &str) -> usize {
        if self.storage.remove(key).is_some() {
            1
        } else {
            0
        }
    }

    fn latest_is_expired(&self) -> bool {
        if let Some((instant, _)) = self.eviction_state.peek() {
            if Instant::now() > *instant {
                return true;
            }
        }
        false
    }

    fn del_latest(&mut self) {
        if let Some((_, key)) = self.eviction_state.pop() {
            self.storage.remove(&key);
        }
    }
}

// We implement lazy eviction.
// When an item is expired, it is kept in the cache and removed either during get or set requests.
pub struct Storage {
    capacity: usize,
    // shard_count should be a power of two.
    shard_count: usize,
    shards: Vec<Arc<RwLock<Shard>>>,
    // we don't want to lock a mutex to get the size as it is a frequent operation.
    size: AtomicUsize,
}

impl Debug for Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Storage")
            .field("capacity", &self.capacity)
            .field("shard_count", &self.shard_count)
            .field("size", &self.size)
            .finish()
    }
}

impl Storage {
    /// new creates a new storage. shard_count must be a power of two or the function panics.
    pub fn new(capacity: usize, shard_count: usize) -> Self {
        assert!(
            shard_count.is_power_of_two(),
            "shard_count must be a power of two"
        );
        // Assuming shards are equally distributed
        let mut shards = Vec::with_capacity(shard_count);
        for _ in 0..shard_count {
            let shard = Arc::new(RwLock::new(Shard::new()));
            shards.push(shard);
        }
        Storage {
            capacity,
            shard_count,
            shards,
            size: Default::default(),
        }
    }
    fn get_shard(&self, key: &str) -> &Arc<RwLock<Shard>> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        let hash = hasher.finish();
        let shard_index = (hash as usize) & (self.shard_count - 1);
        &self.shards[shard_index]
    }

    pub fn set_kv(&self, key: &str, value: &str, ttl: Duration) -> Option<String> {
        let shard = self.get_shard(key);
        let mut shard = shard.write().unwrap();
        // lazy eviction, remove the latest key if it has expired
        if shard.latest_is_expired() {
            shard.del_latest();
        }
        shard.add_or_update_kv(key, value, Instant::now() + ttl)
    }

    pub fn get_v(&self, key: &str) -> Option<String> {
        let shard = self.get_shard(key);
        let shard = shard.read().unwrap();
        let maybe_entry = shard.get_value_by_key(key);
        maybe_entry.cloned()
        // lazy eviction, remove the last key if it has expired
    }

    // fn size(&self) -> usize {
    //     self.size.load(Ordering::Relaxed)
    // }

    pub(crate) fn del_entries(&self, keys: &Vec<String>) -> usize {
        let mut count = 0;
        for key in keys {
            let shard = self.get_shard(key);
            let mut bucket = shard.write().unwrap();
            count += bucket.del_entry(key);
        }
        self.size.fetch_sub(count, Ordering::Relaxed);
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_handler_test() {
        let storage = Storage::new(100, 8);

        // check set and get
        storage.set_kv("Key1", "V1", Duration::from_millis(300));
        let v = storage.get_v("Key1").unwrap();
        assert_eq!(v, "V1", "Value should exist and be V1");
        let v2 = storage.get_v("Key2");
        assert_eq!(v2, None, "There should be no value for key2");

        // check update
        let old_v = storage
            .set_kv("Key1", "UpdateV1", Duration::from_millis(300))
            .unwrap();
        assert_eq!(
            old_v, "V1",
            "Set kv on an existing key should return the old value"
        );
        let v1 = storage.get_v("Key1").unwrap();
        assert_eq!(
            v1, "UpdateV1",
            "Calling set on existing key should update value"
        );

        // check delete
        let num_deleted = storage.del_entries(&vec!["Key1".to_string()]);
        assert_eq!(num_deleted, 1, "should delete 1 key");
        let v2 = storage.get_v("Key1");
        assert_eq!(v2, None, "Key1 entry should have been deleted");
        storage.set_kv("Key1", "V1", Duration::from_millis(300));
        storage.set_kv("Key2", "V1", Duration::from_millis(300));
        let num_deleted = storage.del_entries(&vec!["Key1".to_string(), "Key2".to_string()]);
        assert_eq!(num_deleted, 2, "should delete 2 key");

        // check ordering
        storage.set_kv("ent1", "V1", Duration::from_millis(180));
        storage.set_kv("ent2", "V1", Duration::from_millis(300));
        storage.set_kv("ent3", "V1", Duration::from_millis(100));
        // let ent2 = storage.get_oldest().unwrap();
        // assert_eq!(ent2, "ent2", "Oldest value should be ent2");
    }
}
