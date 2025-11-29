use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::BTreeMap;
use parking_lot::RwLock;
use memmap2::Mmap;

use crate::core::format::{BDBLogEntry, EntryType, TableType};
use crate::core::heatmap::BloomFilter;

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
    pub table_type: TableType,
}

impl MemTable {
    pub fn new(max_size: usize, table_type: TableType) -> Self {
        Self {
            entries: BTreeMap::new(),
            max_size,
            current_size: 0,
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
    }
}

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
    pub fn create(level: u8, entries: &BTreeMap<Vec<u8>, KVEntry>, base_path: &Path, table_type: TableType) -> io::Result<Self> {
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
            if entry.deleted { continue; }
            
            let mut bdb_entry = BDBLogEntry {
                entry_type: entry.entry_type,
                key: entry.key.clone(),
                value: entry.value.clone(),
                timestamp: entry.timestamp,
                entry_crc: 0,
            };
            
            let size = bdb_entry.write(&mut file)?;
            
            index.push(IndexEntry {
                key: entry.key.clone(),
                position: offset,
                size,
                timestamp: entry.timestamp,
            });
            
            offset += size as u64;
        }
        
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
    
    pub fn get(&self, key: &[u8]) -> Option<KVEntry> {
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
            let mut cursor = io::Cursor::new(data);
            
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
        
        let mut index = Vec::new();
        let mut offset = 0;
        let len = mmap.len();
        
        while offset < len {
            let mut cursor = io::Cursor::new(&mmap[offset..]);
            match BDBLogEntry::read(&mut cursor) {
                Ok(entry) => {
                    let size = cursor.position() as usize;
                    index.push(IndexEntry {
                        key: entry.key.clone(),
                        position: offset as u64,
                        size,
                        timestamp: entry.timestamp,
                    });
                    offset += size;
                }
                Err(_) => break, // Stop on error or EOF
            }
        }
        
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
}

impl LSMTree {
    pub fn new(base_path: &Path, table_type: TableType, max_memtable_size: usize) -> Self {
        let mut levels = Vec::with_capacity(10);
        for _ in 0..10 {
            levels.push(RwLock::new(Vec::new()));
        }

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
            memtable: RwLock::new(MemTable::new(max_memtable_size, table_type)),
            levels,
            base_path: base_path.to_path_buf(),
            table_type,
        }
    }
    
    pub fn put(&self, key: Vec<u8>, value: Vec<u8>) -> io::Result<()> {
        let mut mem = self.memtable.write();
        mem.put(key, value, EntryType::Insert);
        
        if mem.should_flush() {
            drop(mem); // unlock
            self.flush()?;
        }
        Ok(())
    }
    
    pub fn get(&self, key: &[u8]) -> Option<KVEntry> {
        // 1. MemTable
        if let Some(entry) = self.memtable.read().get(key) {
            return Some(entry);
        }
        
        // 2. Levels (0 to 9)
        for level in &self.levels {
            let sstables = level.read();
            // Search newest SSTables first (usually end of list)
            for sstable in sstables.iter().rev() {
                if let Some(entry) = sstable.get(key) {
                    return Some(entry);
                }
            }
        }
        
        None
    }
    
    pub fn flush(&self) -> io::Result<()> {
        let mut mem = self.memtable.write();
        if mem.entries.is_empty() { return Ok(()); }
        
        // Clone entries for flushing (BTreeMap matches SSTable creation signature now)
        let entries = mem.entries.clone();
        mem.clear();
        drop(mem); // Unlock MemTable
        
        // Create SSTable (Level 0)
        let sstable = Arc::new(SSTable::create(0, &entries, &self.base_path, self.table_type)?);
        
        // Add to Level 0
        self.levels[0].write().push(sstable);
        
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

