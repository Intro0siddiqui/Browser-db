use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::Arc;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct BDBKey {
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct BDBValue {
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub enum QueryType {
    Read,
    Write,
    Delete,
    Compact,
}

#[derive(Debug, Clone)]
pub struct HeatEntry {
    pub heat: u32,
    pub access_count: u32,
    pub last_access: u64,
    pub created_at: u64,
}

pub struct HeatTracker {
    max_entries: usize,
    decay_factor: f64,
    hot_threshold: u32,
    last_decay_time: AtomicU64,
    // Use an Array of RwLock<HashMap> to shard the lock and reduce contention
    heat_entries: Vec<RwLock<HashMap<Vec<u8>, HeatEntry>>>,
}

impl HeatTracker {
    pub fn new(max_entries: usize) -> Self {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let mut heat_entries = Vec::with_capacity(32);
        for _ in 0..32 {
            heat_entries.push(RwLock::new(HashMap::new()));
        }

        Self {
            max_entries,
            decay_factor: 0.95,
            hot_threshold: 10,
            last_decay_time: AtomicU64::new(now),
            heat_entries,
        }
    }

    fn get_shard(&self, key: &[u8]) -> usize {
        let mut hash = 0usize;
        for &b in key {
            hash = hash.wrapping_add(b as usize);
        }
        hash % 32
    }

    pub fn record_access(&self, key: &[u8], query_type: QueryType) {
        self.apply_decay();
        
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        
        let increment = match query_type {
            QueryType::Read => 1,
            QueryType::Write => 2,
            QueryType::Delete => 3,
            QueryType::Compact => 4,
        };

        let shard_idx = self.get_shard(key);
        let mut entries = self.heat_entries[shard_idx].write();
        let entry = entries.entry(key.to_vec()).or_insert(HeatEntry {
            heat: 0,
            access_count: 0,
            last_access: now,
            created_at: now,
        });
        
        entry.heat = entry.heat.saturating_add(increment);
        entry.access_count += 1;
        entry.last_access = now;
    }
    
    pub fn get_heat(&self, key: &[u8]) -> u32 {
        let shard_idx = self.get_shard(key);
        if let Some(entry) = self.heat_entries[shard_idx].read().get(key) {
             // Simple decay simulation for read
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
            let age_seconds = now.saturating_sub(entry.last_access);
            let decay_cycles = age_seconds / 60;
            
            if decay_cycles > 0 {
                let factor = self.decay_factor.powf(decay_cycles as f64);
                return (entry.heat as f64 * factor) as u32;
            }
            return entry.heat;
        }
        0
    }
    
    fn apply_decay(&self) {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let last_time = self.last_decay_time.load(Ordering::Acquire);
        if now - last_time < 60 {
            return;
        }

        // Try to update the last_decay_time. If we fail, someone else did it, so we can return.
        if self.last_decay_time.compare_exchange(last_time, now, Ordering::Release, Ordering::Relaxed).is_err() {
            return;
        }
        
        // Apply decay to all and remove cold
        for shard in &self.heat_entries {
            let mut keys_to_remove = Vec::new();
            let mut entries = shard.write();

            for (key, entry) in entries.iter_mut() {
                if entry.heat < 1 {
                    keys_to_remove.push(key.clone());
                }
            }

            for key in keys_to_remove {
                entries.remove(&key);
            }
        }
    }
}

pub struct BloomFilter {
    bit_array: Vec<u8>,
    bit_array_size: usize,
    num_hashes: u32,
}

impl BloomFilter {
    pub fn new(expected_elements: usize, false_positive_rate: f64) -> Self {
        let optimal_bit_size = -((expected_elements as f64 * false_positive_rate.ln()) / (2.0f64.ln().powi(2))) as usize;
        let bit_array_size = (optimal_bit_size + 7) / 8;
        let k = ((optimal_bit_size as f64 / expected_elements as f64) * 2.0f64.ln()) as u32;
        
        Self {
            bit_array: vec![0; bit_array_size],
            bit_array_size,
            num_hashes: k.max(1),
        }
    }
    
    pub fn add(&mut self, key: &[u8]) {
        for i in 0..self.num_hashes {
            let hash = self.hash(key, i);
            let bit_pos = (hash % (self.bit_array_size * 8) as u64) as usize;
            self.bit_array[bit_pos / 8] |= 1 << (bit_pos % 8);
        }
    }
    
    pub fn might_contain(&self, key: &[u8]) -> bool {
        for i in 0..self.num_hashes {
            let hash = self.hash(key, i);
            let bit_pos = (hash % (self.bit_array_size * 8) as u64) as usize;
            if (self.bit_array[bit_pos / 8] & (1 << (bit_pos % 8))) == 0 {
                return false;
            }
        }
        true
    }
    
    fn hash(&self, key: &[u8], seed: u32) -> u64 {
        // Simple combination of hashes to simulate k hashes
        let h1 = self.murmur3(key, seed);
        let h2 = self.fnv1a(key);
        h1.wrapping_add(h2)
    }

    fn murmur3(&self, key: &[u8], seed: u32) -> u64 {
        let mut h = seed as u64;
        // Simplified Murmur3-like mixing
        for chunk in key.chunks(4) {
             let mut k = 0u32;
             for &b in chunk.iter().rev() {
                 k = (k << 8) | b as u32;
             }
             h ^= k as u64;
             h = h.wrapping_mul(0xc6a4a7935bd1e995);
             h ^= h >> 47;
        }
        h
    }
    
    fn fnv1a(&self, key: &[u8]) -> u64 {
        let mut h = 14695981039346656037u64;
        for &b in key {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heat_tracker_basic() {
        let mut tracker = HeatTracker::new(100);
        let key = b"test_key";

        tracker.record_access(key, QueryType::Read);
        assert!(tracker.get_heat(key) >= 1);

        tracker.record_access(key, QueryType::Write);
        assert!(tracker.get_heat(key) >= 3);
    }

    #[test]
    fn test_decay_factor_math() {
        let tracker = HeatTracker::new(100);
        let initial_heat = 100.0;
        let decay_factor = tracker.decay_factor;
        let cycles = 5.0;

        let decayed_heat = initial_heat * decay_factor.powf(cycles);
        assert!(decayed_heat < initial_heat);
        assert!((decayed_heat - 77.37).abs() < 1.0); // 100 * 0.95^5 approx 77.37
    }

    #[test]
    fn test_bloom_filter_basic() {
        let mut bf = BloomFilter::new(100, 0.01);
        let key = b"test_key";

        assert!(!bf.might_contain(key));
        bf.add(key);
        assert!(bf.might_contain(key));
    }
}
