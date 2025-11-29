use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

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
    last_decay_time: u64,
    heat_entries: HashMap<Vec<u8>, HeatEntry>,
}

impl HeatTracker {
    pub fn new(max_entries: usize) -> Self {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        Self {
            max_entries,
            decay_factor: 0.95,
            hot_threshold: 10,
            last_decay_time: now,
            heat_entries: HashMap::new(),
        }
    }

    pub fn record_access(&mut self, key: &[u8], query_type: QueryType) {
        self.apply_decay();
        
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        
        let entry = self.heat_entries.entry(key.to_vec()).or_insert(HeatEntry {
            heat: 0,
            access_count: 0,
            last_access: now,
            created_at: now,
        });
        
        let increment = match query_type {
            QueryType::Read => 1,
            QueryType::Write => 2,
            QueryType::Delete => 3,
            QueryType::Compact => 4,
        };
        
        entry.heat = entry.heat.saturating_add(increment);
        entry.access_count += 1;
        entry.last_access = now;
    }
    
    pub fn get_heat(&self, key: &[u8]) -> u32 {
        if let Some(entry) = self.heat_entries.get(key) {
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
    
    fn apply_decay(&mut self) {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        if now - self.last_decay_time < 60 {
            return;
        }
        self.last_decay_time = now;
        
        // Apply decay to all and remove cold
        let mut keys_to_remove = Vec::new();
        
        for (key, entry) in self.heat_entries.iter_mut() {
            if entry.heat < 1 {
                keys_to_remove.push(key.clone());
            }
        }
        
        for key in keys_to_remove {
            self.heat_entries.remove(&key);
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
