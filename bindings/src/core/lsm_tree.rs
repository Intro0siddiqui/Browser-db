use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::{BTreeMap, BinaryHeap};
use std::cmp::Ordering;
use parking_lot::RwLock;
use memmap2::Mmap;
use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian};

use crate::core::format::{BDBLogEntry, EntryType, TableType, BDBFileHeader, BDBFileFooter, BDB_HEADER_SIZE, BDB_FOOTER_SIZE, BDB_BLOCK_SIZE, BDB_RESTART_INTERVAL};
use crate::core::heatmap::{BloomFilter, HeatTracker, QueryType};
use crate::core::wal::WALManager;
use crate::core::blob_log::{BlobLog, BlobPointer, BlobLogIterator};

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
    last_key: Vec<u8>,
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

        match read_compressed_entry(&mut cursor, &self.last_key) {
            Ok(log_entry) => {
                let size = cursor.position() as usize;
                self.offset += size;
                self.last_key = log_entry.key.clone();
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

fn write_compressed_entry<W: io::Write>(writer: &mut W, entry: &BDBLogEntry, shared: usize) -> io::Result<usize> {
    let mut bytes_written = 0;

    writer.write_u8(entry.entry_type as u8)?;
    bytes_written += 1;

    let non_shared = entry.key.len() - shared;
    bytes_written += crate::core::format::write_varint(writer, shared as u64)?;
    bytes_written += crate::core::format::write_varint(writer, non_shared as u64)?;
    bytes_written += crate::core::format::write_varint(writer, entry.value.len() as u64)?;

    writer.write_all(&entry.key[shared..])?;
    bytes_written += non_shared;

    writer.write_all(&entry.value)?;
    bytes_written += entry.value.len();

    writer.write_u64::<LittleEndian>(entry.timestamp)?;
    bytes_written += 8;

    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&[entry.entry_type as u8]);

    // Use consistent varint hashing for shared length
    let mut shared_buf = Vec::new();
    crate::core::format::write_varint(&mut shared_buf, shared as u64)?;
    hasher.update(&shared_buf);

    hasher.update(&entry.key[shared..]);
    hasher.update(&entry.value);
    hasher.update(&entry.timestamp.to_le_bytes());
    let crc = hasher.finalize();

    writer.write_u32::<LittleEndian>(crc)?;
    bytes_written += 4;

    Ok(bytes_written)
}

fn read_compressed_entry<R: io::Read>(reader: &mut R, full_key: &[u8]) -> io::Result<BDBLogEntry> {
    let entry_type_res = reader.read_u8();
    if let Err(ref e) = entry_type_res {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"));
        }
    }
    let entry_type = entry_type_res?.into();
    let shared = crate::core::format::read_varint(reader)? as usize;
    let non_shared = crate::core::format::read_varint(reader)? as usize;
    let value_len = crate::core::format::read_varint(reader)? as usize;

    let mut key_suffix = vec![0u8; non_shared];
    reader.read_exact(&mut key_suffix)?;

    let mut key = Vec::with_capacity(shared + non_shared);
    if shared > 0 {
        let len = full_key.len().min(shared);
        key.extend_from_slice(&full_key[..len]);
    }
    key.extend_from_slice(&key_suffix);

    let mut value = vec![0u8; value_len];
    reader.read_exact(&mut value)?;

    let timestamp = reader.read_u64::<LittleEndian>()?;
    let _crc = reader.read_u32::<LittleEndian>()?;

    Ok(BDBLogEntry {
        entry_type,
        key,
        value,
        timestamp,
        entry_crc: _crc,
    })
}

pub struct SourceIterator<'a> {
    iter: Box<dyn Iterator<Item = io::Result<KVEntry>> + 'a>,
    source_id: usize, // used for stable tie-breaking
}

struct HeapNode<'a> {
    entry: KVEntry,
    source_id: usize,
    iter: Box<dyn Iterator<Item = io::Result<KVEntry>> + 'a>,
}

impl<'a> PartialEq for HeapNode<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.entry.key == other.entry.key && self.entry.timestamp == other.entry.timestamp
    }
}

impl<'a> Eq for HeapNode<'a> {}

impl<'a> PartialOrd for HeapNode<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a> Ord for HeapNode<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        // Min-heap based on key (alphabetical)
        let mut ord = other.entry.key.cmp(&self.entry.key);
        if ord == Ordering::Equal {
            // Newest timestamp first for same key
            ord = self.entry.timestamp.cmp(&other.entry.timestamp);
        }
        if ord == Ordering::Equal {
            // Source ID as last resort for stability
            ord = other.source_id.cmp(&self.source_id);
        }
        ord
    }
}

struct SSTableStreamWrapper {
    _sst: Arc<SSTable>,
    iter: SSTableIterator<'static>,
}

impl Iterator for SSTableStreamWrapper {
    type Item = io::Result<KVEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

pub struct MergeIterator<'a> {
    heap: BinaryHeap<HeapNode<'a>>,
    last_yielded_key: Vec<u8>,
    prefix: Vec<u8>,
}

impl<'a> MergeIterator<'a> {
    pub fn new(iters: Vec<SourceIterator<'a>>, prefix: Vec<u8>) -> Self {
        let mut heap = BinaryHeap::new();
        for mut src in iters {
            if let Some(Ok(entry)) = src.iter.next() {
                heap.push(HeapNode {
                    entry,
                    source_id: src.source_id,
                    iter: src.iter,
                });
            }
        }
        Self {
            heap,
            last_yielded_key: Vec::new(),
            prefix,
        }
    }
}

impl<'a> Iterator for MergeIterator<'a> {
    type Item = io::Result<KVEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(mut node) = self.heap.pop() {
            let key = node.entry.key.clone();

            // Re-fill heap from the source iterator
            if let Some(res) = node.iter.next() {
                match res {
                    Ok(next_entry) => {
                        self.heap.push(HeapNode {
                            entry: next_entry,
                            source_id: node.source_id,
                            iter: node.iter,
                        });
                    }
                    Err(e) => return Some(Err(e)),
                }
            }

            // Prefix check
            if !self.prefix.is_empty() && !key.starts_with(&self.prefix) {
                continue;
            }

            // Deduplication and deletion check
            if !self.last_yielded_key.is_empty() && self.last_yielded_key == key {
                continue; // Skip older version
            }

            self.last_yielded_key = key;

            if !node.entry.deleted {
                return Some(Ok(node.entry));
            }
            // If deleted, we continue loop to find next unique key
        }
        None
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
        
        let mut last_key: Vec<u8> = Vec::new();
        let mut count = 0;

        for entry in entries.values() {
            let bdb_entry = BDBLogEntry {
                entry_type: entry.entry_type,
                key: entry.key.clone(),
                value: entry.value.clone(),
                timestamp: entry.timestamp,
                entry_crc: 0,
            };
            
            let pos = offset;

            // Prefix Compression Logic
            let shared = if count % BDB_RESTART_INTERVAL == 0 {
                0
            } else {
                let mut shared = 0;
                while shared < last_key.len() && shared < entry.key.len() && last_key[shared] == entry.key[shared] {
                    shared += 1;
                }
                shared
            };

            // For now, we still use BDBLogEntry which writes the full key.
            // To support true prefix compression in the file, we'd need to modify BDBLogEntry::write.
            // Let's implement a "Compressed" write path.

            let size = self::write_compressed_entry(&mut file, &bdb_entry, shared)?;
            
            index.push(IndexEntry {
                key: entry.key.clone(),
                position: pos,
                size,
                timestamp: entry.timestamp,
            });
            
            offset += size as u64;
            total_key_size += entry.key.len() as u64;
            total_value_size += entry.value.len() as u64;
            max_entry_size = max_entry_size.max(size as u32);
            last_key = entry.key.clone();
            count += 1;
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

        // Since we use variable shared length, we need to know the shared length.
        // But for random access, we must know the shared length.
        // In our simple implementation, every Nth entry is a restart point (shared=0).
        // For random access via index, we fortunately stored the full key in IndexEntry,
        // and we can reconstruct if we read carefully.

        if let Ok(log_entry) = read_compressed_entry(&mut cursor, &index_entry.key) {
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
            last_key: Vec::new(),
        }
    }

    pub fn seek_prefix(&self, prefix: &[u8]) -> SSTableIterator<'_> {
        let limit = self.mmap.len() - BDB_FOOTER_SIZE - self.block_checksums.len() * 4;

        // Find the first entry that might match the prefix
        let idx = match self.index.binary_search_by(|i| i.key.as_slice().cmp(prefix)) {
            Ok(i) => i,
            Err(i) => i,
        };

        if idx >= self.index.len() {
            return SSTableIterator {
                sstable: self,
                offset: limit,
                limit,
                last_key: Vec::new(),
            };
        }

        // To make seek_prefix efficient and correct, we should jump to the nearest RESTART point
        // BEFORE or AT the target index.
        let restart_idx = (idx / BDB_RESTART_INTERVAL) * BDB_RESTART_INTERVAL;

        SSTableIterator {
            sstable: self,
            offset: self.index[restart_idx].position as usize,
            limit,
            last_key: Vec::new(), // Restart point always has shared=0
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
        
        let mut last_key = Vec::new();
        while offset < data_end {
            let mut cursor = io::Cursor::new(&mmap[offset..data_end]);
            match read_compressed_entry(&mut cursor, &last_key) {
                Ok(entry) => {
                    let size = cursor.position() as usize;
                    index.push(IndexEntry {
                        key: entry.key.clone(),
                        position: offset as u64,
                        size,
                        timestamp: entry.timestamp,
                    });
                    last_key = entry.key.clone();
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

pub struct IndexDefinition {
    pub name: String,
    pub extractor: Arc<dyn Fn(&[u8], &[u8]) -> Option<Vec<u8>> + Send + Sync>,
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
    pub indices: Vec<IndexDefinitionInternal>,
    pub is_index: bool,
}

pub struct IndexDefinitionInternal {
    pub name: String,
    pub extractor: Arc<dyn Fn(&[u8], &[u8]) -> Option<Vec<u8>> + Send + Sync>,
    pub tree: LSMTree,
}

impl LSMTree {
    pub fn new(base_path: &Path, table_type: TableType, max_memtable_size: usize, config: crate::core::config::BrowserDBConfig) -> io::Result<Self> {
        Self::new_with_indices(base_path, table_type, max_memtable_size, config, Vec::new())
    }

    pub fn new_with_indices(
        base_path: &Path,
        table_type: TableType,
        max_memtable_size: usize,
        config: crate::core::config::BrowserDBConfig,
        index_defs: Vec<IndexDefinition>
    ) -> io::Result<Self> {
        Self::new_internal(base_path, table_type, max_memtable_size, config, index_defs, false)
    }

    fn new_index_tree(
        base_path: &Path,
        table_type: TableType,
        max_memtable_size: usize,
        config: crate::core::config::BrowserDBConfig,
    ) -> io::Result<Self> {
        Self::new_internal(base_path, table_type, max_memtable_size, config, Vec::new(), true)
    }

    fn new_internal(
        base_path: &Path,
        table_type: TableType,
        max_memtable_size: usize,
        config: crate::core::config::BrowserDBConfig,
        index_defs: Vec<IndexDefinition>,
        is_index: bool,
    ) -> io::Result<Self> {
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

        // Initialize indices
        let mut indices = Vec::new();
        let table_prefix = match table_type {
            TableType::History => "history",
            TableType::Cookies => "cookies",
            TableType::Cache => "cache",
            TableType::LocalStore => "localstore",
            TableType::Settings => "settings",
        };
        for def in index_defs {
            let idx_path = base_path.join(format!("{}_idx_{}", table_prefix, def.name));
            if !idx_path.exists() {
                fs::create_dir_all(&idx_path)?;
            }
            let idx_tree = LSMTree::new_index_tree(&idx_path, table_type, max_memtable_size / 2, config.clone())?;
            indices.push(IndexDefinitionInternal {
                name: def.name,
                extractor: def.extractor,
                tree: idx_tree,
            });
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
                indices,
                is_index,
            })
        })
    }
    
    pub fn put(&self, key: Vec<u8>, value: Vec<u8>) -> io::Result<()> {
        // Write-Side Indexing
        if !self.inner.is_index {
            for idx in &self.inner.indices {
                if let Some(idx_key) = (idx.extractor)(&key, &value) {
                    idx.tree.put(idx_key, key.clone())?;
                }
            }
        }

        let (entry_type, stored_value) = if value.len() > 64 * 1024 {
            let ptr = self.inner.blob_log.put(&key, &value)?;
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
        // Write-Side Indexing for Batch
        if !self.inner.is_index {
            for (k, v, t) in &batch.entries {
                if *t == EntryType::Insert || *t == EntryType::Update {
                    for idx in &self.inner.indices {
                        if let Some(idx_key) = (idx.extractor)(k, v) {
                            idx.tree.put(idx_key, k.clone())?;
                        }
                    }
                } else if *t == EntryType::Delete {
                    // If we delete a record, we should ideally delete the index entry too.
                    // However, without the old value, we can't know the index key.
                    // For now, BrowserDB indices will be "lazy-cleaned" or require manual management
                    // if they are not purely based on the key.
                    // But for pure "Write-Side Indexing" as requested, we handle inserts.
                }
            }
        }

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
        for idx in &self.inner.indices {
            idx.tree.clear()?;
        }

        for shard in &self.inner.memtable {
            shard.write().clear();
        }

        let mut levels = Vec::new();
        for l in &self.inner.levels {
            levels.push(l.write());
        }

        for mut level in levels {
            for sstable in level.drain(..) {
                #[cfg(target_os = "windows")]
                {
                    let path = sstable.file_path.clone();
                    drop(sstable);
                    let _ = fs::remove_file(&path);
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let _ = fs::remove_file(&sstable.file_path);
                }
            }
        }

        Ok(())
    }
    
    pub fn get(&self, key: &[u8]) -> Option<KVEntry> {
        self.inner.heat_tracker.record_access(key, QueryType::Read);

        let mut entry = self.inner.get_raw(key)?;
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
        Some(entry)
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

    pub fn streaming_iter<'a>(&'a self, prefix: &'a [u8]) -> MergeIterator<'a> {
        let mut iters = Vec::new();

        // 1. MemTable Iterators
        // We collect MemTable entries because they are small and already in RAM.
        // For true streaming from MemTable, we would need to keep the shard ReadGuard alive.
        for (i, shard) in self.inner.memtable.iter().enumerate() {
            let mem = shard.read();
            let mut entries = Vec::new();
            let range = if prefix.is_empty() {
                mem.entries.range::<Vec<u8>, _>(..)
            } else {
                mem.entries.range(prefix.to_vec()..)
            };
            for (_, entry) in range {
                if !prefix.is_empty() && !entry.key.starts_with(prefix) { break; }
                entries.push(Ok(entry.clone()));
            }
            iters.push(SourceIterator {
                iter: Box::new(entries.into_iter()),
                source_id: i,
            });
        }

        // 2. SSTable Iterators
        // We bypass the level lock issues by leaking the SSTableIterator which
        // only depends on the Mmap which is already behind an Arc.
        let mut source_id = 16;
        for level in &self.inner.levels {
            let sstables = level.read();
            for sstable in sstables.iter() {
                // seek_prefix returns an iterator with the lifetime of sstable.
                // Since SSTable is Arc-wrapped and stored in self, we can safely
                // extend the lifetime by leaking a reference if we ensure
                // the Arc stays alive.
                // However, a cleaner way is to wrap it in a struct that owns the Arc.

                let sst_clone = Arc::clone(sstable);
                let sst_ptr = Arc::as_ptr(&sst_clone);
                // UNSAFE: We're extending the lifetime of the iterator by promising
                // that the Arc<SSTable> outlives the iterator.
                // We keep the Arc alive in a vector that we move into MergeIterator.
                let iter = unsafe { (*sst_ptr).seek_prefix(prefix) };

                iters.push(SourceIterator {
                    iter: Box::new(SSTableStreamWrapper {
                        _sst: sst_clone,
                        iter,
                    }),
                    source_id,
                });
                source_id += 1;
            }
        }

        MergeIterator::new(iters, prefix.to_vec())
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
                let mut deref_entry = entry.clone();
                if deref_entry.entry_type == EntryType::BlobIndex && !deref_entry.deleted {
                    if let Some(ptr) = BlobPointer::decode(&deref_entry.value) {
                        if let Ok(val) = self.inner.blob_log.get(&ptr) {
                            deref_entry.value = val;
                        }
                    }
                }

                if !deref_entry.deleted && predicate(&deref_entry) {
                    results.insert(key.clone(), Some(deref_entry));
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
                        if let Some(mut kv) = sstable.get_at_index(idx) {
                            if kv.entry_type == EntryType::BlobIndex && !kv.deleted {
                                if let Some(ptr) = BlobPointer::decode(&kv.value) {
                                    if let Ok(val) = self.inner.blob_log.get(&ptr) {
                                        kv.value = val;
                                    }
                                }
                            }

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

    pub fn run_blob_gc(&self) -> io::Result<()> {
        self.inner.run_blob_gc()
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

            // Drop local references before removing files
            let paths_to_remove: Vec<_> = tables_to_compact.iter().map(|t| t.file_path.clone()).collect();
            drop(tables_to_compact);
            for path in paths_to_remove {
                if let Err(e) = std::fs::remove_file(&path) {
                    eprintln!("Failed to remove SSTable file {}: {}", path.display(), e);
                }
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

    pub fn run_blob_gc(&self) -> io::Result<()> {
        let blob_path = self.blob_log.get_path();
        let gc_path = blob_path.with_extension("blob.gc.tmp");

        let mut new_blob_log_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&gc_path)?;

        let mut new_pointers = Vec::new();
        let iter = BlobLogIterator::new(&blob_path)?;

        let mut current_new_offset = 0u64;

        for entry_res in iter {
            let (old_offset, _old_size, key, value) = entry_res?;

            // Check if this blob is still alive in LSM-tree
            let is_alive = if let Some(kv) = self.get_raw(&key) {
                if kv.entry_type == EntryType::BlobIndex {
                    if let Some(ptr) = BlobPointer::decode(&kv.value) {
                        ptr.offset == old_offset
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if is_alive {
                // Write to new log
                let mut log_entry = BDBLogEntry::new(EntryType::BlobIndex, key.clone(), value);
                let written = log_entry.write(&mut new_blob_log_file)?;

                new_pointers.push((key, BlobPointer {
                    offset: current_new_offset,
                    size: written as u32,
                }));

                current_new_offset += written as u64;
            }
        }

        new_blob_log_file.sync_all()?;

        // Atomic update in LSM-tree
        if !new_pointers.is_empty() {
            let mut batch = Batch::new();
            for (key, ptr) in new_pointers {
                batch.put(key, ptr.encode());
            }

            self.apply_batch_direct(batch, EntryType::BlobIndex)?;
        }

        // Swap files
        self.blob_log.swap_file(&gc_path)?;

        Ok(())
    }

    fn get_raw(&self, key: &[u8]) -> Option<KVEntry> {
        // 1. MemTable
        let shard = (key.first().cloned().unwrap_or(0) % 16) as usize;
        let entry_opt = self.memtable[shard].read().get(key);

        if let Some(entry) = entry_opt {
            return Some(entry);
        }

        // 2. Levels (0 to 9)
        for level in &self.levels {
            let sstables = level.read();
            for sstable in sstables.iter().rev() {
                if let Some(entry) = sstable.get(key) {
                    return Some(entry);
                }
            }
        }

        None
    }

    fn apply_batch_direct(&self, batch: Batch, entry_type: EntryType) -> io::Result<()> {
        let mut wal = self.wal.write();
        wal.log(&mut BDBLogEntry::new(EntryType::BatchStart, Vec::new(), Vec::new()))?;
        for (k, v, _t) in &batch.entries {
            wal.log(&mut BDBLogEntry::new(entry_type, k.clone(), v.clone()))?;
        }
        wal.log(&mut BDBLogEntry::new(EntryType::BatchEnd, Vec::new(), Vec::new()))?;
        drop(wal);

        for (k, v, _t) in batch.entries {
            let shard = (k.first().cloned().unwrap_or(0) % 16) as usize;
            let mut mem = self.memtable[shard].write();
            mem.put(k, v, entry_type);
        }
        Ok(())
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

        // Note: SSTable file removal is now handled in `run_compaction_cascade`
        // after removing the table entries from `self.levels` to prevent locking
        // and race conditions, especially on Windows.

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
