use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::BTreeMap;
use parking_lot::RwLock;
use memmap2::Mmap;
use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian};

use crate::core::format::{BDBLogEntry, EntryType, TableType, BDBFileHeader, BDBFileFooter, BDB_HEADER_SIZE, BDB_FOOTER_SIZE, BDB_BLOCK_SIZE};
use crate::core::heatmap::{BloomFilter, HeatTracker, QueryType};
use crate::core::wal::WALManager;
use crate::core::blob_log::{BlobLog, BlobPointer};

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
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
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
    pub block_checksums: Vec<u32>,
}

pub struct SSTableIterator<'a> {
    sstable: &'a SSTable,
    offset: usize,
    limit: usize,
}

impl<'a> Iterator for SSTableIterator<'a> {
    type Item = io::Result<KVEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.limit {
            return None;
        }

        // Verify block checksum before reading entry
        if let Err(e) = self.sstable.verify_blocks(self.offset, self.offset + 1) {
            return Some(Err(e));
        }

        let data = &self.sstable.mmap[self.offset..self.limit];
        let mut cursor = io::Cursor::new(data);

        match BDBLogEntry::read(&mut cursor) {
            Ok(log_entry) => {
                let size = cursor.position() as usize;
                self.offset += size;
                Some(Ok(KVEntry {
                    key: log_entry.key,
                    value: log_entry.value,
                    timestamp: log_entry.timestamp,
                    entry_type: log_entry.entry_type,
                    deleted: log_entry.entry_type == EntryType::Delete,
                }))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

impl SSTable {
    pub fn verify_blocks(&self, start_pos: usize, end_pos: usize) -> io::Result<()> {
        let first_block = (start_pos - BDB_HEADER_SIZE) / BDB_BLOCK_SIZE;
        let last_block = (end_pos - 1 - BDB_HEADER_SIZE) / BDB_BLOCK_SIZE;

        for i in first_block..=last_block {
            let block_start = BDB_HEADER_SIZE + i * BDB_BLOCK_SIZE;
            let block_end = (block_start + BDB_BLOCK_SIZE).min(self.mmap.len() - BDB_FOOTER_SIZE - self.block_checksums.len() * 4);

            if block_start >= block_end { continue; }

            let mut hasher = crc32fast::Hasher::new();
            hasher.update(&self.mmap[block_start..block_end]);
            let actual_crc = hasher.finalize();

            if let Some(&expected_crc) = self.block_checksums.get(i) {
                if actual_crc != expected_crc {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Block {} checksum mismatch", i)));
                }
            }
        }
        Ok(())
    }

    pub fn create(level: u8, entries: &BTreeMap<Vec<u8>, KVEntry>, base_path: &Path, table_type: TableType) -> io::Result<Self> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?.as_millis();
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

        let mut header = BDBFileHeader::new(table_type);
        header.write(&mut file)?;
        let header_size = BDB_HEADER_SIZE;

        // BTreeMap is already sorted by key
        
        let mut index = Vec::new();
        let mut offset = header_size as u64;
        let mut total_key_size = 0;
        let mut total_value_size = 0;
        let mut max_entry_size = 0;
        
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
            total_key_size += entry.key.len() as u64;
            total_value_size += entry.value.len() as u64;
            max_entry_size = max_entry_size.max(size as u32);
        }

        let data_end = offset;

        // Calculate Block Checksums
        file.sync_all()?;
        let mmap_for_crc = unsafe { Mmap::map(&file)? };
        let mut block_checksums = Vec::new();
        let mut curr = BDB_HEADER_SIZE;
        while curr < data_end as usize {
            let end = (curr + BDB_BLOCK_SIZE).min(data_end as usize);
            let mut hasher = crc32fast::Hasher::new();
            hasher.update(&mmap_for_crc[curr..end]);
            block_checksums.push(hasher.finalize());
            curr = end;
        }
        drop(mmap_for_crc);

        // Write Checksums to file
        let crc_offset = offset;
        for &crc in &block_checksums {
            file.write_u32::<LittleEndian>(crc)?;
            offset += 4;
        }

        let footer = BDBFileFooter {
            entry_count: entries.len() as u64,
            file_size: offset + BDB_FOOTER_SIZE as u64,
            data_offset: header_size as u64,
            block_crc_offset: crc_offset,
            max_entry_size,
            total_key_size,
            total_value_size,
            compression_ratio: 100,
            reserved: [0; 2],
            file_crc: 0,
        };
        footer.write(&mut file)?;
        
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
            block_checksums,
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

        // Verify block checksums
        if let Err(_) = self.verify_blocks(start, end) {
            return None;
        }

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
        let limit = self.mmap.len() - BDB_FOOTER_SIZE - self.block_checksums.len() * 4;
        SSTableIterator {
            sstable: self,
            offset: BDB_HEADER_SIZE,
            limit,
        }
    }

    pub fn open(file_path: PathBuf, level: u8) -> io::Result<Self> {
        let mut file = OpenOptions::new().read(true).open(&file_path)?;
        let _header = BDBFileHeader::read(&mut file)?;
        let mmap = unsafe { Mmap::map(&file)? };
        
        if mmap.len() < BDB_HEADER_SIZE + BDB_FOOTER_SIZE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "SSTable too small"));
        }

        let mut footer_cursor = io::Cursor::new(&mmap[mmap.len()-BDB_FOOTER_SIZE..]);
        let footer = BDBFileFooter::read(&mut footer_cursor)?;

        // Load block checksums
        let mut block_checksums = Vec::new();
        let num_blocks = (footer.block_crc_offset - footer.data_offset + BDB_BLOCK_SIZE as u64 - 1) / BDB_BLOCK_SIZE as u64;
        let mut crc_cursor = io::Cursor::new(&mmap[footer.block_crc_offset as usize..(mmap.len() - BDB_FOOTER_SIZE)]);
        for _ in 0..num_blocks {
            if let Ok(crc) = crc_cursor.read_u32::<LittleEndian>() {
                block_checksums.push(crc);
            }
        }

        let mut index = Vec::new();
        let mut offset = BDB_HEADER_SIZE;
        let data_end = footer.block_crc_offset as usize;
        
        while offset < data_end {
            let mut cursor = io::Cursor::new(&mmap[offset..data_end]);
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
            block_checksums,
        })
    }
}

pub struct Batch {
    pub entries: Vec<(Vec<u8>, Vec<u8>, EntryType)>,
}

impl Batch {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.entries.push((key, value, EntryType::Insert));
    }

    pub fn delete(&mut self, key: Vec<u8>) {
        self.entries.push((key, Vec::new(), EntryType::Delete));
    }
}

#[derive(Clone)]
pub struct LSMTree {
    pub inner: Arc<LSMTreeInner>,
}

pub struct LSMTreeInner {
    pub memtable: [RwLock<MemTable>; 16],
    pub levels: Vec<RwLock<Vec<Arc<SSTable>>>>, // 10 levels
    pub base_path: PathBuf,
    pub table_type: TableType,
    pub wal: RwLock<WALManager>,
    pub blob_log: Arc<BlobLog>,
    pub heat_tracker: HeatTracker,
    pub config: crate::core::config::BrowserDBConfig,
}

impl LSMTree {
    pub fn new(base_path: &Path, table_type: TableType, max_memtable_size: usize, config: crate::core::config::BrowserDBConfig) -> io::Result<Self> {
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

        let blob_path = base_path.join(format!("{}.blob", match table_type {
            TableType::History => "history",
            TableType::Cookies => "cookies",
            TableType::Cache => "cache",
            TableType::LocalStore => "localstore",
            TableType::Settings => "settings",
        }));
        let blob_log = Arc::new(BlobLog::open(&blob_path)?);

        let memtable: [RwLock<MemTable>; 16] = (0..16)
            .map(|_| RwLock::new(MemTable::new(max_memtable_size / 16, table_type)))
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Failed to initialize sharded memtable"))?;

        // Recover from WAL
        let entries = wal.read_all()?;
        let mut in_batch = false;
        let mut batch_entries: Vec<(Vec<u8>, Vec<u8>, EntryType)> = Vec::new();

        for entry in entries {
            match entry.entry_type {
                EntryType::BatchStart => {
                    in_batch = true;
                    batch_entries.clear();
                }
                EntryType::BatchEnd => {
                    if in_batch {
                        for (k, v, t) in batch_entries.drain(..) {
                            let shard = (k.first().cloned().unwrap_or(0) % 16) as usize;
                            memtable[shard].write().put(k, v, t);
                        }
                        in_batch = false;
                    }
                }
                _ => {
                    if in_batch {
                        batch_entries.push((entry.key, entry.value, entry.entry_type));
                    } else {
                        let shard = (entry.key.first().cloned().unwrap_or(0) % 16) as usize;
                        memtable[shard].write().put(entry.key, entry.value, entry.entry_type);
                    }
                }
            }
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
                memtable,
                levels,
                base_path: base_path.to_path_buf(),
                table_type,
                wal: RwLock::new(wal),
                blob_log,
                heat_tracker: HeatTracker::new(config.heatmap.max_entries),
                config,
            })
        })
    }
    
    pub fn put(&self, key: Vec<u8>, value: Vec<u8>) -> io::Result<()> {
        let (entry_type, stored_value) = if value.len() > 64 * 1024 {
            let ptr = self.inner.blob_log.put(&value)?;
            (EntryType::BlobIndex, ptr.encode())
        } else {
            (EntryType::Insert, value)
        };

        let mut wal_entry = BDBLogEntry::new(entry_type, key.clone(), stored_value.clone());
        self.inner.wal.write().log(&mut wal_entry)?;

        let shard = (key.first().cloned().unwrap_or(0) % 16) as usize;
        let mut mem = self.inner.memtable[shard].write();
        mem.put(key, stored_value, entry_type);
        
        if mem.should_flush() {
            drop(mem); // unlock
            self.flush()?;
        }
        Ok(())
    }

    pub fn apply_batch(&self, batch: Batch) -> io::Result<()> {
        let mut wal = self.inner.wal.write();
        wal.log(&mut BDBLogEntry::new(EntryType::BatchStart, Vec::new(), Vec::new()))?;
        for (k, v, t) in &batch.entries {
            wal.log(&mut BDBLogEntry::new(*t, k.clone(), v.clone()))?;
        }
        wal.log(&mut BDBLogEntry::new(EntryType::BatchEnd, Vec::new(), Vec::new()))?;
        drop(wal);

        let mut needs_flush = false;
        for (k, v, t) in batch.entries {
            let shard = (k.first().cloned().unwrap_or(0) % 16) as usize;
            let mut mem = self.inner.memtable[shard].write();
            mem.put(k, v, t);
            if mem.should_flush() {
                needs_flush = true;
            }
        }

        if needs_flush {
            self.flush()?;
        }
        Ok(())
    }

    pub fn clear(&self) -> io::Result<()> {
        for shard in &self.inner.memtable {
            shard.write().clear();
        }

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
        let shard = (key.first().cloned().unwrap_or(0) % 16) as usize;
        let entry_opt = self.inner.memtable[shard].read().get(key);

        if let Some(mut entry) = entry_opt {
            if entry.deleted {
                return None;
            }
            if entry.entry_type == EntryType::BlobIndex {
                if let Some(ptr) = BlobPointer::decode(&entry.value) {
                    if let Ok(val) = self.inner.blob_log.get(&ptr) {
                        entry.value = val;
                    }
                }
            }
            return Some(entry);
        }
        
        // 2. Levels (0 to 9)
        for level in &self.inner.levels {
            let sstables = level.read();
            // Search newest SSTables first (usually end of list)
            for sstable in sstables.iter().rev() {
                if let Some(mut entry) = sstable.get(key) {
                    if entry.deleted {
                        return None;
                    }
                    if entry.entry_type == EntryType::BlobIndex {
                        if let Some(ptr) = BlobPointer::decode(&entry.value) {
                            if let Ok(val) = self.inner.blob_log.get(&ptr) {
                                entry.value = val;
                            }
                        }
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

        let shard = (key.first().cloned().unwrap_or(0) % 16) as usize;
        let mut mem = self.inner.memtable[shard].write();
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
        for shard_lock in &self.inner.memtable {
            let mem = shard_lock.read();
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
        let mut all_entries = BTreeMap::new();
        for shard in &self.inner.memtable {
            let mut mem = shard.write();
            let entries = std::mem::take(&mut mem.entries);
            for (k, v) in entries {
                all_entries.insert(k, v);
            }
            mem.clear();
        }

        if all_entries.is_empty() { return Ok(()); }
        
        // Create SSTable (Level 0)
        let sstable = Arc::new(SSTable::create(0, &all_entries, &self.inner.base_path, self.inner.table_type)?);
        
        // Add to Level 0
        {
            let mut l0 = self.inner.levels[0].write();
            l0.push(sstable);
        }

        // Trigger cascading compaction starting from Level 0
        self.inner.clone().trigger_compaction(0);

        // Truncate WAL after successful flush
        self.inner.wal.write().truncate()?;
        
        Ok(())
    }

    pub fn merge_sstables(&self, level: u8, tables: Vec<Arc<SSTable>>) -> io::Result<Arc<SSTable>> {
        self.inner.merge_sstables(level, tables)
    }
}

impl LSMTreeInner {
    pub fn trigger_compaction(self: Arc<Self>, level: usize) {
        if level >= 9 { return; }

        let (should_compact, mut tables_to_compact) = {
            let levels = self.levels[level].read();
            if level == 0 {
                if levels.len() >= self.config.lsm_tree.max_level0_files {
                    (true, levels.clone())
                } else {
                    (false, vec![])
                }
            } else {
                let total_size: u64 = levels.iter().map(|s| s.mmap.len() as u64).sum();
                let threshold = self.config.lsm_tree.level_size_thresholds_mb.get(level - 1)
                    .cloned()
                    .unwrap_or(10 * 10usize.pow(level as u32 - 1)) as u64 * 1024 * 1024;

                if total_size > threshold {
                    (true, levels.clone())
                } else {
                    (false, vec![])
                }
            }
        };

        if should_compact {
            if level == 0 {
                // Use HeatTracker to influence compaction priority for L0
                let heat = &self.heat_tracker;
                tables_to_compact.sort_by_cached_key(|t| {
                    let mut total_heat = 0u64;
                    for idx in &t.index {
                        total_heat += heat.get_heat(&idx.key) as u64;
                    }
                    total_heat / t.index.len().max(1) as u64
                });
            }

            let inner = Arc::clone(&self);
            std::thread::spawn(move || {
                inner.run_compaction_cascade(level, tables_to_compact);
            });
        }
    }

    fn run_compaction_cascade(self: Arc<Self>, level: usize, tables_to_compact: Vec<Arc<SSTable>>) {
        // Record compaction access in heat tracker
        for table in &tables_to_compact {
            let ht = &self.heat_tracker;
            for idx in &table.index {
                ht.record_access(&idx.key, QueryType::Compact);
            }
        }

        if let Ok(new_sst) = self.merge_sstables((level + 1) as u8, tables_to_compact.clone()) {
            let next_level = level + 1;
            {
                let mut current_lvl = self.levels[level].write();
                let mut next_lvl = self.levels[next_level].write();

                current_lvl.retain(|t| !tables_to_compact.iter().any(|tc| tc.file_path == t.file_path));
                next_lvl.push(new_sst);
            }

            // Cascade to next level if threshold exceeded
            if next_level < 9 {
                let (should_compact_next, tables_next) = {
                    let levels = self.levels[next_level].read();
                    let total_size: u64 = levels.iter().map(|s| s.mmap.len() as u64).sum();
                    let threshold = self.config.lsm_tree.level_size_thresholds_mb.get(next_level - 1)
                        .cloned()
                        .unwrap_or(10 * 10usize.pow(next_level as u32 - 1)) as u64 * 1024 * 1024;

                    if total_size > threshold {
                        (true, levels.clone())
                    } else {
                        (false, vec![])
                    }
                };

                if should_compact_next {
                    self.run_compaction_cascade(next_level, tables_next);
                }
            }
        }
    }

    pub fn merge_sstables(&self, level: u8, tables: Vec<Arc<SSTable>>) -> io::Result<Arc<SSTable>> {
        let is_final_level = level == 9;

        // Multi-way merge sort
        // We handle Iterator items that are Results now. If an iterator yields an error, we bubble it up.
        // `peekable` allows looking ahead, but since we have `Result`, peek() returns `&Result`.

        let mut iterators: Vec<_> = tables.iter().map(|t| t.iter().peekable()).collect();
        let mut merged_entries: BTreeMap<Vec<u8>, KVEntry> = BTreeMap::new();

        loop {
            let mut min_key: Option<Vec<u8>> = None;

            for iter in iterators.iter_mut() {
                if let Some(entry_res) = iter.peek() {
                    match entry_res {
                        Ok(entry) => {
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
                        Err(e) => {
                            return Err(io::Error::new(e.kind(), format!("Corrupted SSTable encountered during merge: {}", e)));
                        }
                    }
                }
            }

            if let Some(key) = min_key {
                let mut best_entry: Option<KVEntry> = None;

                for iter in iterators.iter_mut() {
                    // Check if the next item belongs to `min_key`
                    let should_consume = if let Some(Ok(peeked)) = iter.peek() {
                        peeked.key == key
                    } else {
                        false
                    };

                    if should_consume {
                        if let Some(Ok(entry)) = iter.next() {
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
