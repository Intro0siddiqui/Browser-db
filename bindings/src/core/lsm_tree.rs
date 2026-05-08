use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::BTreeMap;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce
};
use parking_lot::RwLock;
use memmap2::Mmap;

use crate::core::format::{BDBLogEntry, EntryType, TableType};
use crate::core::heatmap::{BloomFilter, HeatTracker, QueryType};
use crate::core::wal::WAL;

#[derive(Debug, Clone)]
pub struct KVEntry {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub timestamp: u64,
    pub entry_type: EntryType,
    pub deleted: bool,
}

impl KVEntry {
    pub fn size(&self) -> usize {
        self.key.len() + self.value.len() + 8 + 1 // timestamp + type
    }
}

pub struct MemTable {
    pub entries: BTreeMap<Vec<u8>, KVEntry>,
    pub max_size: usize,
    pub current_size: usize,
    pub entry_count: usize,
    pub table_type: TableType,
}

impl MemTable {
    pub fn new(max_size: usize, table_type: TableType) -> Self {
        Self {
            entries: BTreeMap::new(),
            max_size,
            current_size: 0,
            entry_count: 0,
            table_type,
        }
    }

    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>, entry_type: EntryType) {
        let entry = KVEntry {
            key: key.clone(),
            value,
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64,
            entry_type,
            deleted: entry_type == EntryType::Delete,
        };

        let new_size = entry.size();

        // Update size: subtract old entry size if exists, add new entry size
        if let Some(old_entry) = self.entries.insert(key, entry) {
            self.current_size -= old_entry.size();
        } else {
            self.entry_count += 1;
        }
        self.current_size += new_size;
    }

    pub fn get(&self, key: &[u8]) -> Option<KVEntry> {
        self.entries.get(key).cloned()
    }

    pub fn should_flush(&self) -> bool {
        self.current_size >= self.max_size
    }
    
    pub fn clear(&mut self) {
        self.entries.clear();
        self.current_size = 0;
        self.entry_count = 0;
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IndexEntry {
    pub key: Vec<u8>,
    pub position: u64,
    pub size: usize,
    pub timestamp: u64,
}

pub struct SSTable {
    pub level: u8,
    pub file_path: PathBuf,
    pub mmap: Mmap,
    pub index: Vec<IndexEntry>,
    pub bloom_filter: Option<BloomFilter>,
}

impl SSTable {
    pub fn create(level: u8, entries: &BTreeMap<Vec<u8>, KVEntry>, base_path: &Path, table_type: TableType, encryption_key: Option<&[u8; 32]>) -> io::Result<Self> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
        let filename = format!("{}_{}_{}_{}.sst", 
            match table_type {
                TableType::History => "history",
                TableType::Cookies => "cookies",
                TableType::Cache => "cache",
                TableType::LocalStore => "localstore",
                TableType::Settings => "settings",
            }, 
            level, timestamp, entries.len());
        let file_path = base_path.join(filename);
        
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&file_path)?;

        // BTreeMap is already sorted by key
        
        let mut index = Vec::new();
        let mut offset = 0u64;
        
        for entry in entries.values() {
            let mut bdb_entry = BDBLogEntry {
                entry_type: entry.entry_type,
                key: entry.key.clone(),
                value: entry.value.clone(),
                timestamp: entry.timestamp,
                entry_crc: 0,
            };
            
            let mut buffer = Vec::new();
            bdb_entry.write(&mut buffer)?;

            // Compress
            let compressed = lz4_flex::compress_prepend_size(&buffer);

            // Encrypt
            let final_data = if let Some(key) = encryption_key {
                let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| io::Error::new(io::ErrorKind::Other, "Invalid encryption key length"))?;
                let mut nonce_bytes = [0u8; 12];
                rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce_bytes);
                let nonce = Nonce::from_slice(&nonce_bytes);
                let ciphertext = cipher.encrypt(nonce, compressed.as_slice()).map_err(|_| io::Error::new(io::ErrorKind::Other, "Encryption failed"))?;
                let mut combined = nonce_bytes.to_vec();
                combined.extend_from_slice(&ciphertext);
                combined
            } else {
                compressed
            };

            file.write_all(&final_data)?;
            let size = final_data.len();
            
            index.push(IndexEntry {
                key: entry.key.clone(),
                position: offset,
                size: size as usize,
                timestamp: entry.timestamp,
            });
            
            offset += size as u64;
        }

        // Write index
        let index_offset = offset;
        let index_data = bincode::serialize(&index).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        file.write_all(&index_data)?;

        // Write footer
        file.write_all(&index_offset.to_le_bytes())?;
        file.write_all(b"BDB-SST")?;
        
        file.sync_all()?;
        
        let mmap = unsafe { Mmap::map(&file)? };
        
        // Build Bloom Filter
        let mut bloom = BloomFilter::new(index.len(), 0.01);
        for idx in &index {
            bloom.add(&idx.key);
        }

        Ok(Self {
            level,
            file_path,
            mmap,
            index,
            bloom_filter: Some(bloom),
        })
    }
    
    pub fn get(&self, key: &[u8], encryption_key: Option<&[u8; 32]>) -> Option<KVEntry> {
        // Check Bloom Filter
        if let Some(bf) = &self.bloom_filter {
            if !bf.might_contain(key) {
                return None;
            }
        }
        
        // Binary Search Index
        if let Ok(idx) = self.index.binary_search_by(|i| i.key.as_slice().cmp(key)) {
            let index_entry = &self.index[idx];
            let start = index_entry.position as usize;
            let end = start + index_entry.size;
            
            if end > self.mmap.len() { return None; }
            
            let data = &self.mmap[start..end];

            // Handle encryption
            let decrypted_data = if let Some(key) = encryption_key {
                if data.len() < 12 { return None; }
                let cipher = Aes256Gcm::new_from_slice(key).ok()?;
                let nonce = Nonce::from_slice(&data[..12]);
                cipher.decrypt(nonce, &data[12..]).ok()?
            } else {
                data.to_vec()
            };

            // Handle compression
            let decompressed_data = if let Ok(decompressed) = lz4_flex::decompress_size_prepended(&decrypted_data) {
                decompressed
            } else {
                decrypted_data
            };

            let mut cursor = io::Cursor::new(decompressed_data);
            
            if let Ok(log_entry) = BDBLogEntry::read(&mut cursor) {
                return Some(KVEntry {
                    key: log_entry.key,
                    value: log_entry.value,
                    timestamp: log_entry.timestamp,
                    entry_type: log_entry.entry_type,
                    deleted: log_entry.entry_type == EntryType::Delete,
                });
            }
        }
        
        None
    }

    pub fn open(file_path: PathBuf, level: u8) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).open(&file_path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let len = mmap.len();
        
        if len < 15 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "SSTable too small"));
        }

        // Read footer
        let magic = &mmap[len-7..];
        if magic != b"BDB-SST" {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid SSTable magic"));
        }

        let mut offset_bytes = [0u8; 8];
        offset_bytes.copy_from_slice(&mmap[len-15..len-7]);
        let index_offset = u64::from_le_bytes(offset_bytes);

        let index_data = &mmap[index_offset as usize..len-15];
        let index: Vec<IndexEntry> = bincode::deserialize(index_data)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        
        // Build Bloom Filter
        let mut bloom = BloomFilter::new(index.len(), 0.01);
        for idx in &index {
            bloom.add(&idx.key);
        }

        Ok(Self {
            level,
            file_path,
            mmap,
            index,
            bloom_filter: Some(bloom),
        })
    }
}

pub struct LSMTree {
    pub memtable: RwLock<MemTable>,
    pub levels: Vec<RwLock<Vec<Arc<SSTable>>>>, // 10 levels
    pub base_path: PathBuf,
    pub table_type: TableType,
    pub wal: RwLock<WAL>,
    pub heat_tracker: RwLock<HeatTracker>,
    pub hot_cache: RwLock<BTreeMap<Vec<u8>, KVEntry>>,
    pub encryption_key: Option<[u8; 32]>,
}

impl LSMTree {
    pub fn new(base_path: &Path, table_type: TableType, max_memtable_size: usize) -> Self {
        Self::new_with_encryption(base_path, table_type, max_memtable_size, None)
    }

    pub fn new_with_encryption(base_path: &Path, table_type: TableType, max_memtable_size: usize, encryption_key: Option<[u8; 32]>) -> Self {
        let mut levels = Vec::with_capacity(10);
        for _ in 0..10 {
            levels.push(RwLock::new(Vec::new()));
        }

        let table_name = match table_type {
            TableType::History => "history",
            TableType::Cookies => "cookies",
            TableType::Cache => "cache",
            TableType::LocalStore => "localstore",
            TableType::Settings => "settings",
        };

        let wal_path = base_path.join(format!("{}.wal", table_name));
        let mut memtable = MemTable::new(max_memtable_size, table_type);

        // Replay WAL
        if let Ok(entries) = WAL::recover(&wal_path, encryption_key) {
            for entry in entries {
                memtable.put(entry.key, entry.value, entry.entry_type);
            }
        }

        let wal = WAL::new_with_encryption(&wal_path, encryption_key).expect("Failed to create WAL");
        let heat_tracker = HeatTracker::new(10000); // Track up to 10k hot keys

        // Recover existing SSTables
        if let Ok(entries) = fs::read_dir(base_path) {
            let prefix = match table_type {
                TableType::History => "history",
                TableType::Cookies => "cookies",
                TableType::Cache => "cache",
                TableType::LocalStore => "localstore",
                TableType::Settings => "settings",
            };
            
            let mut loaded_sstables: Vec<(u8, Arc<SSTable>)> = Vec::new();

            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "sst") {
                    if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                        if fname.starts_with(prefix) {
                            // Parse level from filename: prefix_level_timestamp_count.sst
                            let parts: Vec<&str> = fname.split('_').collect();
                            if parts.len() >= 2 {
                                if let Ok(level) = parts[1].parse::<u8>() {
                                    if level < 10 {
                                        if let Ok(sst) = SSTable::open(path, level) {
                                            loaded_sstables.push((level, Arc::new(sst)));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            
            // Add to levels
            for (level, sst) in loaded_sstables {
                levels[level as usize].write().push(sst);
            }
            
            // Sort each level by timestamp (newest first)? 
            // For now, append order is likely file system order (random).
            // Ideally we should sort. But for get(), we iterate rev().
            // If we want "newest sstable first", we should sort by timestamp (descending).
            // Filename has timestamp.
        }
        
        Self {
            memtable: RwLock::new(memtable),
            levels,
            base_path: base_path.to_path_buf(),
            table_type,
            wal: RwLock::new(wal),
            heat_tracker: RwLock::new(heat_tracker),
            hot_cache: RwLock::new(BTreeMap::new()),
            encryption_key,
        }
    }
    
    pub fn put(&self, key: Vec<u8>, value: Vec<u8>) -> io::Result<()> {
        self.heat_tracker.write().record_access(&key, QueryType::Write);
        self.wal.write().append(EntryType::Insert, &key, &value)?;

        let mut mem = self.memtable.write();
        mem.put(key, value, EntryType::Insert);
        
        if mem.should_flush() {
            drop(mem); // unlock
            self.flush()?;
        }
        Ok(())
    }

    pub fn clear(&self) -> io::Result<()> {
        let mut mem = self.memtable.write();
        mem.clear();
        drop(mem);

        let mut levels = Vec::new();
        for l in &self.levels {
            levels.push(l.write());
        }

        for mut level in levels {
            for sstable in level.drain(..) {
                let _ = fs::remove_file(&sstable.file_path);
            }
        }

        Ok(())
    }
    
    pub fn get(&self, key: &[u8]) -> Option<KVEntry> {
        self.heat_tracker.write().record_access(key, QueryType::Read);

        // 1. Hot Cache
        if let Some(entry) = self.hot_cache.read().get(key) {
            if entry.deleted { return None; }
            return Some(entry.clone());
        }

        // 2. MemTable
        if let Some(entry) = self.memtable.read().get(key) {
            if entry.deleted {
                return None;
            }
            // Update hot cache if heat is high enough
            self.maybe_promote_to_hot_cache(key, &entry);
            return Some(entry);
        }
        
        // 3. Levels (0 to 9)
        for level in &self.levels {
            let sstables = level.read();
            // Search newest SSTables first (usually end of list)
            for sstable in sstables.iter().rev() {
                if let Some(entry) = sstable.get(key, self.encryption_key.as_ref()) {
                    if entry.deleted {
                        return None;
                    }
                    self.maybe_promote_to_hot_cache(key, &entry);
                    return Some(entry);
                }
            }
        }
        
        None
    }

    fn maybe_promote_to_hot_cache(&self, key: &[u8], entry: &KVEntry) {
        let heat = self.heat_tracker.read().get_heat(key);
        if heat > 5 { // Arbitrary threshold for "hot"
            let mut cache = self.hot_cache.write();
            if cache.len() < 1000 { // Max 1000 items in hot cache
                cache.insert(key.to_vec(), entry.clone());
            } else if !cache.contains_key(key) {
                // Simplified LRU or just skip if full for now
            }
        }
    }

    pub fn delete(&self, key: Vec<u8>) -> io::Result<()> {
        self.heat_tracker.write().record_access(&key, QueryType::Delete);
        self.wal.write().append(EntryType::Delete, &key, &[])?;

        let mut mem = self.memtable.write();
        mem.put(key, Vec::new(), EntryType::Delete);

        if mem.should_flush() {
            drop(mem);
            self.flush()?;
        }
        Ok(())
    }

    pub fn apply_batch(&self, ops: Vec<(EntryType, Vec<u8>, Vec<u8>)>) -> io::Result<()> {
        let mut wal = self.wal.write();
        let mut mem = self.memtable.write();

        wal.append(EntryType::BatchStart, &[], &[])?;
        for (entry_type, key, value) in &ops {
            wal.append(*entry_type, key, value)?;
            mem.put(key.clone(), value.clone(), *entry_type);
        }
        wal.append(EntryType::BatchEnd, &[], &[])?;

        if mem.should_flush() {
            drop(mem);
            drop(wal);
            self.flush()?;
        }
        Ok(())
    }

    pub fn all_entries(&self) -> Vec<KVEntry> {
        self.scan_prefix(&[])
    }

    pub fn scan_prefix(&self, prefix: &[u8]) -> Vec<KVEntry> {
        let mut results: BTreeMap<Vec<u8>, KVEntry> = BTreeMap::new();

        // 1. MemTable
        let mem = self.memtable.read();
        let range = if prefix.is_empty() {
            mem.entries.range::<Vec<u8>, _>(..)
        } else {
            mem.entries.range(prefix.to_vec()..)
        };

        for (key, entry) in range {
            if !prefix.is_empty() && !key.starts_with(prefix) { break; }
            results.insert(key.clone(), entry.clone());
        }

        // 2. Levels (newest SSTables first)
        for level in &self.levels {
            let sstables = level.read();
            for sstable in sstables.iter().rev() {
                let start_idx = if prefix.is_empty() {
                    0
                } else {
                    match sstable.index.binary_search_by(|idx| idx.key.as_slice().cmp(prefix)) {
                        Ok(i) => i,
                        Err(i) => i,
                    }
                };

                for idx in &sstable.index[start_idx..] {
                    if !prefix.is_empty() && !idx.key.starts_with(prefix) { break; }
                    if !results.contains_key(&idx.key) {
                        // Directly read from mmap
                        let start = idx.position as usize;
                        let end = start + idx.size;
                        if end <= sstable.mmap.len() {
                            let data = &sstable.mmap[start..end];
                            let mut cursor = io::Cursor::new(data);
                            if let Ok(log_entry) = BDBLogEntry::read(&mut cursor) {
                                results.insert(idx.key.clone(), KVEntry {
                                    key: log_entry.key,
                                    value: log_entry.value,
                                    timestamp: log_entry.timestamp,
                                    entry_type: log_entry.entry_type,
                                    deleted: log_entry.entry_type == EntryType::Delete,
                                });
                            }
                        }
                    }
                }
            }
        }

        results.into_values().filter(|e| !e.deleted).collect()
    }
    
    pub fn flush(&self) -> io::Result<()> {
        let mut mem = self.memtable.write();
        if mem.entries.is_empty() { return Ok(()); }
        
        // Clone entries for flushing (BTreeMap matches SSTable creation signature now)
        let entries = mem.entries.clone();
        mem.clear();
        drop(mem); // Unlock MemTable
        
        // Create SSTable (Level 0)
        let sstable = Arc::new(SSTable::create(0, &entries, &self.base_path, self.table_type, self.encryption_key.as_ref())?);
        
        // Add to Level 0
        self.levels[0].write().push(sstable);
        
        // Clear WAL
        self.wal.write().clear()?;
        let table_name = match self.table_type {
            TableType::History => "history",
            TableType::Cookies => "cookies",
            TableType::Cache => "cache",
            TableType::LocalStore => "localstore",
            TableType::Settings => "settings",
        };
        let wal_path = self.base_path.join(format!("{}.wal", table_name));
        *self.wal.write() = WAL::new_with_encryption(&wal_path, self.encryption_key)?;

        // Trigger compaction (Simplified: just check counts)
        // self.compact();
        
        Ok(())
    }
}

impl Drop for LSMTree {
    fn drop(&mut self) {
        // Attempt to flush on drop
        if let Err(e) = self.flush() {
            eprintln!("Failed to flush LSMTree on drop: {}", e);
        }
    }
}
