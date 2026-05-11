use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::BTreeMap;
use parking_lot::RwLock;
use memmap2::Mmap;

use crate::core::format::{BDBLogEntry, EntryType, TableType};
use crate::core::heatmap::{BloomFilter, HeatTracker, QueryType};
use crate::core::wal::WALManager;

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

pub struct SSTableIterator<'a> {
    sstable: &'a SSTable,
    offset: usize,
}

impl<'a> Iterator for SSTableIterator<'a> {
    type Item = KVEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.sstable.mmap.len() {
            return None;
        }

        let data = &self.sstable.mmap[self.offset..];
        let mut cursor = io::Cursor::new(data);

        match BDBLogEntry::read(&mut cursor) {
            Ok(log_entry) => {
                let size = cursor.position() as usize;
                self.offset += size;
                Some(KVEntry {
                    key: log_entry.key,
                    value: log_entry.value,
                    timestamp: log_entry.timestamp,
                    entry_type: log_entry.entry_type,
                    deleted: log_entry.entry_type == EntryType::Delete,
                })
            }
            Err(_) => None,
        }
    }
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
            return self.get_at_index(&self.index[idx]);
        }
        
        None
    }

    pub fn get_at_index(&self, index_entry: &IndexEntry) -> Option<KVEntry> {
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
        None
    }

    pub fn iter(&self) -> SSTableIterator<'_> {
        SSTableIterator {
            sstable: self,
            offset: 0,
        }
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

#[derive(Clone)]
pub struct LSMTree {
    pub inner: Arc<LSMTreeInner>,
}

pub struct LSMTreeInner {
    pub memtable: RwLock<MemTable>,
    pub levels: Vec<RwLock<Vec<Arc<SSTable>>>>, // 10 levels
    pub base_path: PathBuf,
    pub table_type: TableType,
    pub wal: RwLock<WALManager>,
    pub heat_tracker: HeatTracker,
}

impl LSMTree {
    pub fn new(base_path: &Path, table_type: TableType, max_memtable_size: usize) -> io::Result<Self> {
        let mut levels = Vec::with_capacity(10);
        for _ in 0..10 {
            levels.push(RwLock::new(Vec::new()));
        }

        let wal_path = base_path.join(format!("{}.wal", match table_type {
            TableType::History => "history",
            TableType::Cookies => "cookies",
            TableType::Cache => "cache",
            TableType::LocalStore => "localstore",
            TableType::Settings => "settings",
        }));
        let wal = WALManager::new(&wal_path)?;

        let mut memtable = MemTable::new(max_memtable_size, table_type);

        // Recover from WAL
        let entries = wal.read_all()?;
        for entry in entries {
            memtable.put(entry.key, entry.value, entry.entry_type);
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
        }
        
        Ok(Self {
            inner: Arc::new(LSMTreeInner {
                memtable: RwLock::new(memtable),
                levels,
                base_path: base_path.to_path_buf(),
                table_type,
                wal: RwLock::new(wal),
                heat_tracker: HeatTracker::new(10000),
            })
        })
    }
    
    pub fn put(&self, key: Vec<u8>, value: Vec<u8>) -> io::Result<()> {
        let mut wal_entry = BDBLogEntry::new(EntryType::Insert, key.clone(), value.clone());
        self.inner.wal.write().log(&mut wal_entry)?;

        let mut mem = self.inner.memtable.write();
        mem.put(key, value, EntryType::Insert);
        
        if mem.should_flush() {
            drop(mem); // unlock
            self.flush()?;
        }
        Ok(())
    }

    pub fn clear(&self) -> io::Result<()> {
        let mut mem = self.inner.memtable.write();
        mem.clear();
        drop(mem);

        let mut levels = Vec::new();
        for l in &self.inner.levels {
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
        self.inner.heat_tracker.record_access(key, QueryType::Read);

        // 1. MemTable
        if let Some(entry) = self.inner.memtable.read().get(key) {
            if entry.deleted {
                return None;
            }
            return Some(entry);
        }
        
        // 2. Levels (0 to 9)
        for level in &self.inner.levels {
            let sstables = level.read();
            // Search newest SSTables first (usually end of list)
            for sstable in sstables.iter().rev() {
                if let Some(entry) = sstable.get(key) {
                    if entry.deleted {
                        return None;
                    }
                    return Some(entry);
                }
            }
        }
        
        None
    }

    pub fn delete(&self, key: Vec<u8>) -> io::Result<()> {
        let mut wal_entry = BDBLogEntry::new(EntryType::Delete, key.clone(), Vec::new());
        self.inner.wal.write().log(&mut wal_entry)?;

        let mut mem = self.inner.memtable.write();
        mem.put(key, Vec::new(), EntryType::Delete);

        if mem.should_flush() {
            drop(mem);
            self.flush()?;
        }
        Ok(())
    }

    pub fn all_entries(&self) -> Vec<KVEntry> {
        self.scan_prefix(&[])
    }

    pub fn scan_prefix(&self, prefix: &[u8]) -> Vec<KVEntry> {
        self.scan_with_predicate(prefix, |_| true)
    }

    pub fn scan_with_predicate<F>(&self, prefix: &[u8], predicate: F) -> Vec<KVEntry>
    where F: Fn(&KVEntry) -> bool {
        let mut results: BTreeMap<Vec<u8>, Option<KVEntry>> = BTreeMap::new();

        // 1. MemTable
        {
            let mem = self.inner.memtable.read();
            let range = if prefix.is_empty() {
                mem.entries.range::<Vec<u8>, _>(..)
            } else {
                mem.entries.range(prefix.to_vec()..)
            };

            for (key, entry) in range {
                if !prefix.is_empty() && !key.starts_with(prefix) { break; }
                if !entry.deleted && predicate(entry) {
                    results.insert(key.clone(), Some(entry.clone()));
                } else {
                    results.insert(key.clone(), None);
                }
            }
        }

        // 2. Levels (newest SSTables first)
        for level in &self.inner.levels {
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
                        if let Some(kv) = sstable.get_at_index(idx) {
                            if !kv.deleted && predicate(&kv) {
                                results.insert(idx.key.clone(), Some(kv));
                            } else {
                                results.insert(idx.key.clone(), None);
                            }
                        }
                    }
                }
            }
        }

        results.into_values().flatten().collect()
    }
    
    pub fn flush(&self) -> io::Result<()> {
        let mut mem = self.inner.memtable.write();
        if mem.entries.is_empty() { return Ok(()); }
        
        // Clone entries for flushing
        let entries = mem.entries.clone();
        mem.clear();
        drop(mem); // Unlock MemTable
        
        // Create SSTable (Level 0)
        let sstable = Arc::new(SSTable::create(0, &entries, &self.inner.base_path, self.inner.table_type)?);
        
        // Add to Level 0
        {
            let mut l0 = self.inner.levels[0].write();
            l0.push(sstable);

            // Check for compaction
            // In a real scenario, we'd load this from browserdb.toml
            let max_l0_files = 4;

            if l0.len() >= max_l0_files {
                let mut tables_to_compact = l0.clone();
                drop(l0); // Release the write lock on Level 0

                // Use HeatTracker to influence compaction priority outside the lock
                {
                    let heat = &self.inner.heat_tracker;
                    tables_to_compact.sort_by_cached_key(|t| {
                        let mut total_heat = 0u64;
                        for idx in &t.index {
                            total_heat += heat.get_heat(&idx.key) as u64;
                        }
                        total_heat / t.index.len().max(1) as u64
                    });
                }

                let inner = Arc::clone(&self.inner);
                std::thread::spawn(move || {
                    // Record compaction access in heat tracker
                    for table in &tables_to_compact {
                        let ht = &inner.heat_tracker;
                        for idx in &table.index {
                            ht.record_access(&idx.key, QueryType::Compact);
                        }
                    }

                    if let Ok(new_sst) = inner.merge_sstables(1, tables_to_compact.clone()) {
                        let mut l0 = inner.levels[0].write();
                        let mut l1 = inner.levels[1].write();

                        l0.retain(|t| !tables_to_compact.iter().any(|tc| tc.file_path == t.file_path));
                        l1.push(new_sst);
                    }
                });
            }
        }

        // Truncate WAL after successful flush
        self.inner.wal.write().truncate()?;
        
        Ok(())
    }

    pub fn merge_sstables(&self, level: u8, tables: Vec<Arc<SSTable>>) -> io::Result<Arc<SSTable>> {
        self.inner.merge_sstables(level, tables)
    }
}

impl LSMTreeInner {
    pub fn merge_sstables(&self, level: u8, tables: Vec<Arc<SSTable>>) -> io::Result<Arc<SSTable>> {
        let is_final_level = level == 9;

        // Multi-way merge sort
        let mut iterators: Vec<_> = tables.iter().map(|t| t.iter().peekable()).collect();
        let mut merged_entries: BTreeMap<Vec<u8>, KVEntry> = BTreeMap::new();

        loop {
            let mut min_key: Option<Vec<u8>> = None;

            for iter in iterators.iter_mut() {
                if let Some(entry) = iter.peek() {
                    match &min_key {
                        None => {
                            min_key = Some(entry.key.clone());
                        }
                        Some(key) => {
                            if entry.key < *key {
                                min_key = Some(entry.key.clone());
                            }
                        }
                    }
                }
            }

            if let Some(key) = min_key {
                let mut best_entry: Option<KVEntry> = None;

                for iter in iterators.iter_mut() {
                    if let Some(peeked) = iter.peek() {
                        if peeked.key == key {
                            let entry = iter.next().unwrap();
                            if best_entry.is_none() || entry.timestamp > best_entry.as_ref().unwrap().timestamp {
                                best_entry = Some(entry);
                            }
                        }
                    }
                }

                if let Some(entry) = best_entry {
                    if !is_final_level || !entry.deleted {
                        merged_entries.insert(key, entry);
                    }
                }
            } else {
                break; // No more entries
            }
        }

        let new_sstable = Arc::new(SSTable::create(level, &merged_entries, &self.base_path, self.table_type)?);

        // Delete old sstables
        for table in tables {
            let _ = fs::remove_file(&table.file_path);
        }

        Ok(new_sstable)
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
