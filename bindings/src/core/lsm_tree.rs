use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write, Seek, SeekFrom};

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::{BTreeMap, BinaryHeap, HashSet};
use std::cmp::Ordering;
use parking_lot::{RwLock, RwLockReadGuard};
use memmap2::Mmap;
use self_cell::self_cell;
use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Mutex, Condvar};

use crate::core::format::{BDBLogEntry, EntryType, TableType, BDBFileHeader, BDBFileFooter, BDB_HEADER_SIZE, BDB_FOOTER_SIZE, BDB_BLOCK_SIZE, BDB_RESTART_INTERVAL};
use crate::core::heatmap::{BloomFilter, HeatTracker, QueryType};
use crate::core::wal::WALManager;
use crate::core::blob_log::{BlobLog, BlobPointer, BlobLogIterator};

#[derive(Debug, Clone)]
pub struct KVEntry {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub timestamp: u64,
    pub expires_at: u64,
    pub entry_type: EntryType,
    pub deleted: bool,
}

pub struct TokenBucket {
    pub bytes_per_sec: f64,
    pub last_checked: std::time::Instant,
    pub tokens: f64,
}

impl TokenBucket {
    pub fn new(rate_mb_per_sec: f64) -> Self {
        let bytes_per_sec = rate_mb_per_sec * 1024.0 * 1024.0;
        Self {
            bytes_per_sec,
            last_checked: std::time::Instant::now(),
            tokens: bytes_per_sec, // Start full
        }
    }

    pub fn consume(&mut self, bytes: usize) {
        if self.bytes_per_sec <= 0.0 {
            return;
        }
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_checked).as_secs_f64();
        self.last_checked = now;
        
        self.tokens = (self.tokens + elapsed * self.bytes_per_sec).min(self.bytes_per_sec);
        
        self.tokens -= bytes as f64;
        if self.tokens < 0.0 {
            let sleep_secs = -self.tokens / self.bytes_per_sec;
            if sleep_secs > 0.0 {
                std::thread::sleep(std::time::Duration::from_secs_f64(sleep_secs));
                self.last_checked = std::time::Instant::now();
                self.tokens = 0.0;
            }
        }
    }
}

impl KVEntry {
    pub fn size(&self) -> usize {
        self.key.len() + self.value.len() + 8 + 8 + 1 // timestamp + expires_at + type
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

    pub fn put(&mut self, key: Vec<u8>, mut value: Vec<u8>, mut entry_type: EntryType, expires_at: u64) {
        if entry_type == EntryType::Increment {
            if let Some(existing) = self.entries.get(&key) {
                if existing.entry_type == EntryType::Increment {
                    if existing.value.len() == 8 && value.len() == 8 {
                        let mut old_arr = [0u8; 8];
                        old_arr.copy_from_slice(&existing.value);
                        let mut new_arr = [0u8; 8];
                        new_arr.copy_from_slice(&value);
                        let old_val = i64::from_le_bytes(old_arr);
                        let new_val = i64::from_le_bytes(new_arr);
                        value = old_val.wrapping_add(new_val).to_le_bytes().to_vec();
                    }
                } else if existing.entry_type == EntryType::Insert || existing.entry_type == EntryType::Update {
                    let base_val = if existing.value.len() == 8 {
                        let mut arr = [0u8; 8];
                        arr.copy_from_slice(&existing.value);
                        i64::from_le_bytes(arr)
                    } else {
                        0
                    };
                    if value.len() == 8 {
                        let mut arr = [0u8; 8];
                        arr.copy_from_slice(&value);
                        let delta = i64::from_le_bytes(arr);
                        value = base_val.wrapping_add(delta).to_le_bytes().to_vec();
                    }
                    entry_type = EntryType::Insert;
                }
            }
        }

        let entry = KVEntry {
            key: key.clone(),
            value,
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
            expires_at,
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

    pub fn get(&self, key: &[u8]) -> Option<&KVEntry> {
        self.entries.get(key)
    }

    pub fn should_flush(&self) -> bool {
        self.current_size >= self.max_size
    }

    pub fn should_flush_tuned(&self, power_save: bool, low_memory: bool) -> bool {
        let mut target_max = self.max_size;
        if low_memory {
            target_max = target_max / 2;
        }
        if power_save {
            // Buffer more in RAM (defer flushes): e.g. double the capacity threshold or skip flushing unless low_memory is active
            if !low_memory {
                target_max = target_max * 2;
            }
        }
        self.current_size >= target_max
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
    pub data_end: usize,
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
                    expires_at: log_entry.expires_at,
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

    bytes_written += crate::core::format::write_varint(writer, entry.expires_at)?;

    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&[entry.entry_type as u8]);

    // Use consistent varint hashing for shared length
    let mut shared_buf = [0u8; 10];
    let n = {
        let mut writer_ref = &mut shared_buf[..];
        crate::core::format::write_varint(&mut writer_ref, shared as u64)?
    };
    hasher.update(&shared_buf[..n]);

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

    if value_len > 100 * 1024 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "value_len suspiciously large"));
    }

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
    let expires_at = crate::core::format::read_varint(reader).unwrap_or(0);
    let read_crc = reader.read_u32::<LittleEndian>()?;

    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&[entry_type as u8]);

    let mut shared_buf = [0u8; 10];
    let n = {
        let mut writer_ref = &mut shared_buf[..];
        crate::core::format::write_varint(&mut writer_ref, shared as u64)?
    };
    hasher.update(&shared_buf[..n]);

    hasher.update(&key_suffix);
    hasher.update(&value);
    hasher.update(&timestamp.to_le_bytes());
    let calculated_crc = hasher.finalize();

    if read_crc != calculated_crc {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "CRC mismatch in compressed entry"));
    }

    Ok(BDBLogEntry {
        entry_type,
        key,
        value,
        timestamp,
        expires_at,
        entry_crc: read_crc,
    })
}

type BTreeMapRange<'a> = std::collections::btree_map::Range<'a, Vec<u8>, KVEntry>;

self_cell!(
    struct MemTableIterCell<'a> {
        owner: RwLockReadGuard<'a, MemTable>,

        #[covariant]
        dependent: BTreeMapRange,
    }
);

struct MemTableIteratorWrapper<'a> {
    cell: MemTableIterCell<'a>,
    prefix: Vec<u8>,
}

impl<'a> MemTableIteratorWrapper<'a> {
    fn new(guard: RwLockReadGuard<'a, MemTable>, prefix: Vec<u8>) -> Self {
        let cell = MemTableIterCell::new(guard, |guard| {
            if prefix.is_empty() {
                guard.entries.range::<Vec<u8>, _>(..)
            } else {
                guard.entries.range(prefix.clone()..)
            }
        });

        Self {
            cell,
            prefix,
        }
    }
}

impl<'a> Iterator for MemTableIteratorWrapper<'a> {
    type Item = io::Result<KVEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        let prefix = &self.prefix;
        self.cell.with_dependent_mut(|_guard, iter| {
            if let Some((_key, entry)) = iter.next() {
                if !prefix.is_empty() && !entry.key.starts_with(prefix) {
                    return None;
                }
                return Some(Ok(entry.clone()));
            }
            None
        })
    }
}

pub struct SourceIterator<'a> {
    iter: Box<dyn Iterator<Item = io::Result<KVEntry>> + 'a>,
    source_id: usize, // used for stable tie-breaking
}

struct HeapNode {
    key: Vec<u8>,
    timestamp: u64,
    source_id: usize,
    iter_index: usize,
}

impl PartialEq for HeapNode {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.timestamp == other.timestamp
    }
}

impl Eq for HeapNode {}

impl PartialOrd for HeapNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Min-heap based on key (alphabetical)
        let mut ord = other.key.cmp(&self.key);
        if ord == Ordering::Equal {
            // Newest timestamp first for same key
            ord = self.timestamp.cmp(&other.timestamp);
        }
        if ord == Ordering::Equal {
            // Source ID as last resort for stability
            ord = other.source_id.cmp(&self.source_id);
        }
        ord
    }
}

self_cell!(
    struct SSTableIterCell {
        owner: Arc<SSTable>,

        #[covariant]
        dependent: SSTableIterator,
    }
);

struct SSTableStreamWrapper {
    cell: SSTableIterCell,
}

impl Iterator for SSTableStreamWrapper {
    type Item = io::Result<KVEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        self.cell.with_dependent_mut(|_sst, iter| iter.next())
    }
}

struct HeapSource<'a> {
    iter: Box<dyn Iterator<Item = io::Result<KVEntry>> + 'a>,
    source_id: usize,
    current_entry: Option<KVEntry>,
}

pub struct MergeIterator<'a> {
    heap: BinaryHeap<HeapNode>,
    sources: Vec<HeapSource<'a>>,
    last_yielded_key: Vec<u8>,
    prefix: Vec<u8>,
}

impl<'a> MergeIterator<'a> {
    pub fn new(iters: Vec<SourceIterator<'a>>, prefix: Vec<u8>) -> Self {
        let mut heap = BinaryHeap::new();
        let mut sources = Vec::new();
        for (i, mut src) in iters.into_iter().enumerate() {
            if let Some(res) = src.iter.next() {
                match res {
                    Ok(entry) => {
                        heap.push(HeapNode {
                            key: entry.key.clone(),
                            timestamp: entry.timestamp,
                            source_id: src.source_id,
                            iter_index: i,
                        });
                        sources.push(HeapSource {
                            iter: src.iter,
                            source_id: src.source_id,
                            current_entry: Some(entry),
                        });
                    }
                    Err(_) => {
                        // If it fails immediately, we'll still keep the entry as None.
                        sources.push(HeapSource {
                            iter: src.iter,
                            source_id: src.source_id,
                            current_entry: None,
                        });
                    }
                }
            } else {
                sources.push(HeapSource {
                    iter: src.iter,
                    source_id: src.source_id,
                    current_entry: None,
                });
            }
        }
        Self {
            heap,
            sources,
            last_yielded_key: Vec::new(),
            prefix,
        }
    }
}

impl<'a> Iterator for MergeIterator<'a> {
    type Item = io::Result<KVEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;

        while let Some(node) = self.heap.pop() {
            let src = &mut self.sources[node.iter_index];
            let mut current_entry = src.current_entry.take().unwrap();
            let key = current_entry.key.clone();

            if let Some(res) = src.iter.next() {
                match res {
                    Ok(next_entry) => {
                        self.heap.push(HeapNode {
                            key: next_entry.key.clone(),
                            timestamp: next_entry.timestamp,
                            source_id: src.source_id,
                            iter_index: node.iter_index,
                        });
                        src.current_entry = Some(next_entry);
                    }
                    Err(e) => return Some(Err(e)),
                }
            }

            if !self.prefix.is_empty() && !key.starts_with(&self.prefix) {
                continue;
            }

            if !self.last_yielded_key.is_empty() && self.last_yielded_key == key {
                continue;
            }
            self.last_yielded_key = key.clone();

            let mut delta_sum: i64 = 0;
            let mut has_increments = false;
            let mut is_deleted = current_entry.deleted;
            
            if current_entry.entry_type == EntryType::Increment {
                has_increments = true;
                if current_entry.value.len() == 8 {
                    let mut arr = [0u8; 8];
                    arr.copy_from_slice(&current_entry.value);
                    delta_sum = delta_sum.wrapping_add(i64::from_le_bytes(arr));
                }
            }

            while let Some(peek_node) = self.heap.peek() {
                if peek_node.key != key {
                    break;
                }
                
                let peek_node = self.heap.pop().unwrap();
                let next_src = &mut self.sources[peek_node.iter_index];
                let next_entry = next_src.current_entry.take().unwrap();
                
                if let Some(res) = next_src.iter.next() {
                    match res {
                        Ok(new_entry) => {
                            self.heap.push(HeapNode {
                                key: new_entry.key.clone(),
                                timestamp: new_entry.timestamp,
                                source_id: next_src.source_id,
                                iter_index: peek_node.iter_index,
                            });
                            next_src.current_entry = Some(new_entry);
                        }
                        Err(e) => return Some(Err(e)),
                    }
                }
                
                if !has_increments {
                    continue; 
                }

                if next_entry.entry_type == EntryType::Increment {
                    if next_entry.value.len() == 8 {
                        let mut arr = [0u8; 8];
                        arr.copy_from_slice(&next_entry.value);
                        delta_sum = delta_sum.wrapping_add(i64::from_le_bytes(arr));
                    }
                } else {
                    if next_entry.deleted {
                        is_deleted = true;
                        has_increments = false;
                    } else {
                        let base_val = if next_entry.value.len() == 8 {
                            let mut arr = [0u8; 8];
                            arr.copy_from_slice(&next_entry.value);
                            i64::from_le_bytes(arr)
                        } else {
                            0
                        };
                        current_entry.value = base_val.wrapping_add(delta_sum).to_le_bytes().to_vec();
                        current_entry.entry_type = EntryType::Insert;
                        has_increments = false;
                    }
                }
            }
            
            if has_increments {
                current_entry.value = delta_sum.to_le_bytes().to_vec();
                current_entry.entry_type = EntryType::Insert;
            }

            if !is_deleted && (current_entry.expires_at == 0 || current_entry.expires_at >= now) {
                return Some(Ok(current_entry));
            }
        }
        None
    }
}

#[cfg(windows)]
fn retry_on_permission_denied<F, T>(mut f: F) -> io::Result<T>
where
    F: FnMut() -> io::Result<T>,
{
    let mut attempts = 0;
    loop {
        match f() {
            Ok(res) => return Ok(res),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && attempts < 50 => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                if attempts > 0 || e.kind() == io::ErrorKind::PermissionDenied {
                    eprintln!("BrowserDB FATAL Windows Lock Error after {} attempts: {}", attempts, e);
                }
                return Err(e);
            }
        }
    }
}

#[cfg(not(windows))]
fn retry_on_permission_denied<F, T>(mut f: F) -> io::Result<T>
where
    F: FnMut() -> io::Result<T>,
{
    f()
}

pub fn extract_prefix(key: &[u8]) -> &[u8] {
    if let Some(pos) = key.iter().position(|&b| b == b':') {
        &key[..=pos] // include the delimiter ':'
    } else {
        let n = key.len().min(8);
        &key[..n]
    }
}

impl SSTable {

    pub fn verify_blocks(&self, start_pos: usize, end_pos: usize) -> io::Result<()> {
        let first_block = (start_pos - BDB_HEADER_SIZE) / BDB_BLOCK_SIZE;
        let last_block = (end_pos - 1 - BDB_HEADER_SIZE) / BDB_BLOCK_SIZE;

        for i in first_block..=last_block {
            let block_start = BDB_HEADER_SIZE + i * BDB_BLOCK_SIZE;
            let block_end = (block_start + BDB_BLOCK_SIZE).min(self.data_end);

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

    pub fn create(level: u8, entries: &BTreeMap<Vec<u8>, KVEntry>, base_path: &Path, table_type: TableType, rate_limit_mb: Option<f64>) -> io::Result<Self> {
        let mut attempts = 0;
        let mut rate_limiter = rate_limit_mb.map(TokenBucket::new);
        loop {
            let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?.as_millis();
            let timestamp_nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
            let filename = format!("{}_{}_{}_{}.sst", 
                match table_type {
                    TableType::History => "history",
                    TableType::Cookies => "cookies",
                    TableType::Cache => "cache",
                    TableType::LocalStore => "localstore",
                    TableType::Settings => "settings",
                    TableType::Bookmarks => "bookmarks",
                    TableType::BinaryStore => "binarystore",
                }, 
                level, timestamp, timestamp_nanos % 100000);
            let file_path = base_path.join(filename);
            
            let res = (|| {
                let mut file = retry_on_permission_denied(|| {
                    OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create(true)
                        .open(&file_path)
                })?;

                let mut header = BDBFileHeader::new(table_type);
                header.write(&mut file)?;
                let header_size = BDB_HEADER_SIZE;

                // BTreeMap is already sorted by key
                
                let mut index = Vec::new();
                let mut offset = header_size as u64;
                let mut total_key_size = 0;
                let mut max_entry_size = 0;
                
                let mut last_key: Vec<u8> = Vec::new();
                let mut count = 0;

                for entry in entries.values() {
                    let bdb_entry = BDBLogEntry {
                        entry_type: entry.entry_type,
                        key: entry.key.clone(),
                        value: entry.value.clone(),
                        timestamp: entry.timestamp,
                        expires_at: entry.expires_at,
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
                    
                    if let Some(limiter) = &mut rate_limiter {
                        limiter.consume(size);
                    }

                    index.push(IndexEntry {
                        key: entry.key.clone(),
                        position: pos,
                        size,
                        timestamp: entry.timestamp,
                    });
                    
                    offset += size as u64;
                    total_key_size += entry.key.len() as u64;
                    max_entry_size = max_entry_size.max(size as u32);
                    last_key = entry.key.clone();
                    count += 1;
                }

                let data_end = offset;

                // Calculate Block Checksums
                file.sync_all()?;
                
                let mut block_checksums = Vec::new();
                let mut curr = BDB_HEADER_SIZE as u64;
                let mut buffer = vec![0u8; BDB_BLOCK_SIZE];
                
                // REUSE the existing handle to avoid Windows sharing violations
                file.seek(SeekFrom::Start(curr))?;
                
                while curr < data_end {
                    let to_read = (data_end - curr).min(BDB_BLOCK_SIZE as u64) as usize;
                    file.read_exact(&mut buffer[..to_read])?;
                    
                    let mut hasher = crc32fast::Hasher::new();
                    hasher.update(&buffer[..to_read]);
                    block_checksums.push(hasher.finalize());
                    curr += to_read as u64;
                }

                // Seek to offset (which equals data_end) to write checksums
                file.seek(SeekFrom::Start(offset))?;

                // Write Checksums to file
                let crc_offset = offset;
                for &crc in &block_checksums {
                    file.write_u32::<LittleEndian>(crc)?;
                    offset += 4;
                }

                // Serialize the index block to the end of the file
                let index_offset = offset;
                // We'll write the number of index entries as u64, and then write each IndexEntry.
                file.write_u64::<LittleEndian>(index.len() as u64)?;
                offset += 8;
                for idx in &index {
                    file.write_u64::<LittleEndian>(idx.position)?;
                    file.write_u64::<LittleEndian>(idx.size as u64)?;
                    file.write_u64::<LittleEndian>(idx.timestamp)?;
                    file.write_u64::<LittleEndian>(idx.key.len() as u64)?;
                    file.write_all(&idx.key)?;
                    offset += 8 + 8 + 8 + 8 + idx.key.len() as u64;
                }

                let footer = BDBFileFooter {
                    entry_count: entries.len() as u64,
                    file_size: offset + BDB_FOOTER_SIZE as u64,
                    data_offset: header_size as u64,
                    block_crc_offset: crc_offset,
                    max_entry_size,
                    total_key_size,
                    index_offset,
                    compression_ratio: 100,
                    reserved: [0; 2],
                    file_crc: 0,
                };
                footer.write(&mut file)?;
                file.sync_all()?;
                
                // On Windows, mapping a file that is open for writing can be problematic.
                // We close the write handle first and retry opening for read/map.
                drop(file);
                
                let mmap = retry_on_permission_denied(|| {
                    let mmap_file = File::open(&file_path)?;
                    unsafe { Mmap::map(&mmap_file) }
                })?;
                
                // Build Bloom Filter (including prefixes)
                let mut bloom = BloomFilter::new(index.len() * 2, 0.01);
                for idx in &index {
                    bloom.add(&idx.key);
                    bloom.add(extract_prefix(&idx.key));
                }

                Ok(Self {
                    level,
                    file_path: file_path.clone(),
                    mmap,
                    index,
                    bloom_filter: Some(bloom),
                    block_checksums,
                    data_end: data_end as usize,
                })
            })();

            match res {
                Ok(sstable) => return Ok(sstable),
                Err(_e) if attempts < 5 => {
                    attempts += 1;
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => return Err(e),
            }
        }
    }
    
    pub fn get(&self, key: &[u8]) -> Option<KVEntry> {
        if let Some(bf) = &self.bloom_filter {
            if !bf.might_contain(key) {
                return None;
            }
        }

        if let Ok(idx) = self.index.binary_search_by(|i| i.key.as_slice().cmp(key)) {
            return self.get_at_index(&self.index[idx]);
        }

        None
    }

    pub fn get_at_index(&self, index_entry: &IndexEntry) -> Option<KVEntry> {
        let start = index_entry.position as usize;
        let end = start + index_entry.size;

        if end > self.mmap.len() {
            return None;
        }

        if self.verify_blocks(start, end).is_err() {
            return None;
        }

        let data = &self.mmap[start..end];
        let mut cursor = io::Cursor::new(data);

        if let Ok(log_entry) = read_compressed_entry(&mut cursor, &index_entry.key) {
            return Some(KVEntry {
                key: log_entry.key,
                value: log_entry.value,
                timestamp: log_entry.timestamp,
                expires_at: log_entry.expires_at,
                entry_type: log_entry.entry_type,
                deleted: log_entry.entry_type == EntryType::Delete,
            });
        }
        None
    }

    pub fn iter(&self) -> SSTableIterator<'_> {
        let limit = self.data_end;
        SSTableIterator {
            sstable: self,
            offset: BDB_HEADER_SIZE,
            limit,
            last_key: Vec::new(),
        }
    }

    pub fn seek_prefix(&self, prefix: &[u8]) -> SSTableIterator<'_> {
        let limit = self.data_end;

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
        let mmap = retry_on_permission_denied(|| {
            let file = OpenOptions::new().read(true).open(&file_path)?;
            unsafe { Mmap::map(&file) }
        })?;
        
        if mmap.len() < BDB_HEADER_SIZE + BDB_FOOTER_SIZE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "SSTable too small"));
        }

        let mut header_cursor = io::Cursor::new(&mmap[0..BDB_HEADER_SIZE]);
        let _header = BDBFileHeader::read(&mut header_cursor)?;

        let mut footer_cursor = io::Cursor::new(&mmap[mmap.len()-BDB_FOOTER_SIZE..]);
        let footer = BDBFileFooter::read(&mut footer_cursor)?;

        // Load block checksums
        let mut block_checksums = Vec::new();
        let num_blocks = (footer.block_crc_offset - footer.data_offset + BDB_BLOCK_SIZE as u64 - 1) / BDB_BLOCK_SIZE as u64;
        let block_crc_offset = footer.block_crc_offset as usize;
        let footer_start = mmap.len() - BDB_FOOTER_SIZE;
        if block_crc_offset > footer_start {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Corrupted SSTable footer CRC offset"));
        }
        let mut crc_cursor = io::Cursor::new(&mmap[block_crc_offset..footer_start]);
        for _ in 0..num_blocks {
            if let Ok(crc) = crc_cursor.read_u32::<LittleEndian>() {
                block_checksums.push(crc);
            }
        }

        let mut index = Vec::new();
        let index_offset = footer.index_offset as usize;
        let footer_start = mmap.len() - BDB_FOOTER_SIZE;
        if index_offset > 0 && index_offset < footer_start {
            let mut index_cursor = io::Cursor::new(&mmap[index_offset..footer_start]);
            if let Ok(entry_count) = index_cursor.read_u64::<LittleEndian>() {
                for _ in 0..entry_count {
                    if let (Ok(position), Ok(size), Ok(timestamp), Ok(key_len)) = (
                        index_cursor.read_u64::<LittleEndian>(),
                        index_cursor.read_u64::<LittleEndian>(),
                        index_cursor.read_u64::<LittleEndian>(),
                        index_cursor.read_u64::<LittleEndian>(),
                    ) {
                        let mut key = vec![0u8; key_len as usize];
                        if index_cursor.read_exact(&mut key).is_ok() {
                            index.push(IndexEntry {
                                key,
                                position,
                                size: size as usize,
                                timestamp,
                            });
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }

        // Fallback to full file parsing if index loading failed or is empty
        if index.is_empty() {
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
        }
        
        // Build Bloom Filter (including prefixes)
        let mut bloom = BloomFilter::new(index.len() * 2, 0.01);
        for idx in &index {
            bloom.add(&idx.key);
            bloom.add(extract_prefix(&idx.key));
        }

        Ok(Self {
            level,
            file_path,
            mmap,
            index,
            bloom_filter: Some(bloom),
            block_checksums,
            data_end: footer.block_crc_offset as usize,
        })
    }
}

pub struct IndexDefinition {
    pub name: String,
    pub field_name: String,
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

#[derive(Debug)]
pub struct CompactionTask {
    pub level: usize,
    pub created_at: SystemTime,
}

pub struct CompactionQueue {
    pub pending: Vec<CompactionTask>,
    pub active_levels: HashSet<usize>,
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
    pub last_active_time: Arc<AtomicU64>,
    pub compaction_state: Arc<(Mutex<CompactionQueue>, Condvar)>,
    pub power_save_mode: std::sync::atomic::AtomicBool,
    pub low_memory_mode: std::sync::atomic::AtomicBool,
    pub shutdown: Arc<std::sync::atomic::AtomicBool>,
}

pub struct IndexDefinitionInternal {
    pub name: String,
    pub field_name: String,
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
            TableType::Bookmarks => "bookmarks",
            TableType::BinaryStore => "binarystore",
        }));
        let wal = WALManager::new(&wal_path)?;

        let blob_path = base_path.join(format!("{}.blob", match table_type {
            TableType::History => "history",
            TableType::Cookies => "cookies",
            TableType::Cache => "cache",
            TableType::LocalStore => "localstore",
            TableType::Settings => "settings",
            TableType::Bookmarks => "bookmarks",
            TableType::BinaryStore => "binarystore",
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
                            memtable[shard].write().put(k, v, t, 0);
                        }
                        in_batch = false;
                    }
                }
                _ => {
                    if in_batch {
                        batch_entries.push((entry.key, entry.value, entry.entry_type));
                    } else {
                        let shard = (entry.key.first().cloned().unwrap_or(0) % 16) as usize;
                        memtable[shard].write().put(entry.key, entry.value, entry.entry_type, entry.expires_at);
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
            TableType::Bookmarks => "bookmarks",
            TableType::BinaryStore => "binarystore",
        };
        for def in index_defs {
            let idx_path = base_path.join(format!("{}_idx_{}", table_prefix, def.name));
            if !idx_path.exists() {
                fs::create_dir_all(&idx_path)?;
            }
            let idx_tree = LSMTree::new_index_tree(&idx_path, table_type, max_memtable_size / 2, config.clone())?;
            indices.push(IndexDefinitionInternal {
                name: def.name,
                field_name: def.field_name,
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
                TableType::Bookmarks => "bookmarks",
                TableType::BinaryStore => "binarystore",
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
        
        let last_active_time = Arc::new(AtomicU64::new(
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
        ));

        let compaction_state = Arc::new((
            Mutex::new(CompactionQueue {
                pending: Vec::new(),
                active_levels: HashSet::new(),
            }),
            Condvar::new(),
        ));

        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let inner = Arc::new(LSMTreeInner {
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
            last_active_time,
            compaction_state,
            power_save_mode: std::sync::atomic::AtomicBool::new(false),
            low_memory_mode: std::sync::atomic::AtomicBool::new(false),
            shutdown: Arc::clone(&shutdown),
        });

        // Start background compaction worker thread
        let inner_clone = Arc::clone(&inner);
        std::thread::spawn(move || {
            // Set thread priorities (lowest)
            #[cfg(unix)]
            unsafe {
                extern "C" {
                    fn setpriority(which: i32, who: i32, prio: i32) -> i32;
                }
                setpriority(0, 0, 20);
            }
            #[cfg(windows)]
            unsafe {
                extern "system" {
                    fn GetCurrentThread() -> *mut std::ffi::c_void;
                    fn SetThreadPriority(thread: *mut std::ffi::c_void, priority: i32) -> i32;
                }
                SetThreadPriority(GetCurrentThread(), -2); // THREAD_PRIORITY_LOWEST
            }

            loop {
                if inner_clone.shutdown.load(AtomicOrdering::Relaxed) {
                    break;
                }

                let task = {
                    let &(ref lock, ref cvar) = &*inner_clone.compaction_state;
                    let mut queue = lock.lock().unwrap();
                    loop {
                        // Check if we can run any pending task
                        let mut task_idx = None;
                        for (idx, pending_task) in queue.pending.iter().enumerate() {
                            if !queue.active_levels.contains(&pending_task.level) {
                                // Check if we should delay due to "Silent Window" + "Max Deadline"
                                let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
                                let last_active = inner_clone.last_active_time.load(AtomicOrdering::Relaxed);
                                let idle_duration = now.saturating_sub(last_active);
                                
                                let l0_count = inner_clone.levels[0].read().len();
                                let time_pending = pending_task.created_at.elapsed().unwrap_or_default().as_secs();
                                
                                // Silent window and deadline configured dynamically
                                let deadline = inner_clone.config.lsm_tree.compaction_deadline_sec;
                                let idle_threshold = inner_clone.config.lsm_tree.compaction_idle_threshold_ms;

                                let force_run = (l0_count >= 4 && time_pending >= deadline) || 
                                                (inner_clone.config.lsm_tree.max_level0_files >= 4 && 
                                                 l0_count >= inner_clone.config.lsm_tree.max_level0_files && 
                                                 time_pending >= deadline);
                                
                                // If power_save_mode is enabled, defer/delay compaction tasks in worker queue (unless low_memory_mode is active)
                                let power_save = inner_clone.power_save_mode.load(AtomicOrdering::SeqCst);
                                let low_mem = inner_clone.low_memory_mode.load(AtomicOrdering::SeqCst);
                                
                                let mut is_idle_or_forced = idle_duration >= idle_threshold || force_run;
                                if power_save && !low_mem {
                                    // Defer/disable compaction tasks: only run if extremely forced (e.g., time_pending >= 5 * deadline)
                                    is_idle_or_forced = force_run && (time_pending >= 5 * deadline);
                                }

                                if is_idle_or_forced {
                                    task_idx = Some(idx);
                                    break;
                                }
                            }
                        }

                        if let Some(idx) = task_idx {
                            let t = queue.pending.remove(idx);
                            queue.active_levels.insert(t.level);
                            break Some(t);
                        }

                        // Wait for a notification or a timeout to check silent window / deadline again
                        let result = cvar.wait_timeout(queue, std::time::Duration::from_millis(100)).unwrap();
                        queue = result.0;
                    }
                };

                if let Some(t) = task {
                    // Gather tables to compact
                    let mut tables_to_compact = {
                        let levels = inner_clone.levels[t.level].read();
                        levels.clone()
                    };

                    if t.level == 0 {
                        // Use HeatTracker to influence compaction priority for L0
                        let heat = &inner_clone.heat_tracker;
                        tables_to_compact.sort_by_cached_key(|t| {
                            let mut total_heat = 0u64;
                            for idx in &t.index {
                                total_heat += heat.get_heat(&idx.key) as u64;
                            }
                            total_heat / t.index.len().max(1) as u64
                        });
                    }

                    if !tables_to_compact.is_empty() {
                        inner_clone.clone().run_compaction_cascade(t.level, tables_to_compact);
                    }

                    // Done with this level's compaction
                    let &(ref lock, ref cvar) = &*inner_clone.compaction_state;
                    let mut queue = lock.lock().unwrap();
                    queue.active_levels.remove(&t.level);
                    cvar.notify_all();
                }
            }
        });

        Ok(Self { inner })
    }
    
    pub fn set_power_save_mode(&self, enabled: bool) {
        self.inner.power_save_mode.store(enabled, AtomicOrdering::SeqCst);
    }

    pub fn set_low_memory_mode(&self, enabled: bool) {
        self.inner.low_memory_mode.store(enabled, AtomicOrdering::SeqCst);
        if enabled {
            let _ = self.flush();
        }
    }

    pub fn shutdown(&self) {
        self.inner.shutdown.store(true, AtomicOrdering::Relaxed);
        let &(ref lock, ref cvar) = &*self.inner.compaction_state;
        let _queue = lock.lock();
        cvar.notify_all();
    }
}

impl LSMTree {
    pub fn put(&self, key: Vec<u8>, value: Vec<u8>) -> io::Result<()> {
        self.put_with_field_filter(key, value, None)
    }

    /// Write `key`/`value` to the tree, building only those write-side
    /// indices whose `field_name` is in `allowed_fields`. Pass `None` to
    /// build every registered index (the default behavior of [`put`]).
    pub fn put_with_field_filter(
        &self,
        key: Vec<u8>,
        value: Vec<u8>,
        allowed_fields: Option<&[&str]>,
    ) -> io::Result<()> {
        let now_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        self.inner.last_active_time.store(now_time, AtomicOrdering::Relaxed);

        // Level 0 Write Stall (Backpressure) to protect reads and prevent OOM/disk exhaustion.
        let l0_count = self.inner.levels[0].read().len();
        if l0_count >= 12 {
            // Hard limit: stall incoming writes significantly
            std::thread::sleep(std::time::Duration::from_millis(100));
        } else if l0_count >= 8 {
            // Soft limit: scale delay dynamically (e.g. 10ms - 40ms)
            let delay = (l0_count - 7) as u64 * 10;
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }

        // Write-Side Indexing
        if !self.inner.is_index {
            for idx in &self.inner.indices {
                let skip = match allowed_fields {
                    Some(fields) => !fields.contains(&idx.field_name.as_str()),
                    None => false,
                };
                if skip {
                    continue;
                }
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
        mem.put(key, stored_value, entry_type, 0);

        let power_save = self.inner.power_save_mode.load(AtomicOrdering::SeqCst);
        let low_memory = self.inner.low_memory_mode.load(AtomicOrdering::SeqCst);
        if mem.should_flush_tuned(power_save, low_memory) {
            drop(mem); // unlock
            self.flush()?;
        }
        Ok(())
    }

    pub fn increment(&self, key: Vec<u8>, delta: i64) -> io::Result<()> {
        let now_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        self.inner.last_active_time.store(now_time, AtomicOrdering::Relaxed);

        let value = delta.to_le_bytes().to_vec();
        let mut wal_entry = BDBLogEntry::new(EntryType::Increment, key.clone(), value.clone());
        self.inner.wal.write().log(&mut wal_entry)?;

        let shard = (key.first().cloned().unwrap_or(0) % 16) as usize;
        let mut mem = self.inner.memtable[shard].write();
        mem.put(key, value, EntryType::Increment, 0);

        let power_save = self.inner.power_save_mode.load(AtomicOrdering::SeqCst);
        let low_memory = self.inner.low_memory_mode.load(AtomicOrdering::SeqCst);
        if mem.should_flush_tuned(power_save, low_memory) {
            drop(mem);
            self.flush()?;
        }
        Ok(())
    }

    pub fn put_with_ttl(&self, key: Vec<u8>, value: Vec<u8>, ttl_ms: u64) -> io::Result<()> {
        let now_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        self.inner.last_active_time.store(now_time, AtomicOrdering::Relaxed);

        let expires_at = now_time + ttl_ms;

        // Write-Side Indexing
        if !self.inner.is_index {
            for idx in &self.inner.indices {
                if let Some(idx_key) = (idx.extractor)(&key, &value) {
                    idx.tree.put(idx_key, key.clone())?;
                }
            }
        }

        let value_size = value.len();
        let mut entry_type = EntryType::Insert;
        let mut stored_value = value.clone();

        if value_size > 65536 {
            if let Ok(ptr) = self.inner.blob_log.put(&key, &value) {
                entry_type = EntryType::BlobIndex;
                stored_value = ptr.encode();
            }
        }

        let mut wal_entry = BDBLogEntry::with_ttl(entry_type, key.clone(), stored_value.clone(), expires_at);
        self.inner.wal.write().log(&mut wal_entry)?;

        let shard = (key.first().cloned().unwrap_or(0) % 16) as usize;
        let mut mem = self.inner.memtable[shard].write();
        mem.put(key, stored_value, entry_type, expires_at);

        let power_save = self.inner.power_save_mode.load(AtomicOrdering::SeqCst);
        let low_memory = self.inner.low_memory_mode.load(AtomicOrdering::SeqCst);
        if mem.should_flush_tuned(power_save, low_memory) {
            drop(mem);
            self.flush()?;
        }
        Ok(())
    }

    pub fn apply_batch(&self, batch: Batch) -> io::Result<()> {
        let now_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        self.inner.last_active_time.store(now_time, AtomicOrdering::Relaxed);

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

        let power_save = self.inner.power_save_mode.load(AtomicOrdering::SeqCst);
        let low_memory = self.inner.low_memory_mode.load(AtomicOrdering::SeqCst);
        let mut needs_flush = false;
        for (k, v, t) in batch.entries {
            let shard = (k.first().cloned().unwrap_or(0) % 16) as usize;
            let mut mem = self.inner.memtable[shard].write();
            mem.put(k, v, t, 0);
            if mem.should_flush_tuned(power_save, low_memory) {
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
        let now_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        self.inner.last_active_time.store(now_time, AtomicOrdering::Relaxed);

        self.inner.heat_tracker.record_access(key, QueryType::Read);

        let mut entry = self.inner.get_raw(key)?;
        if entry.deleted {
            return None;
        }

        let now = now_time;
        if entry.expires_at > 0 && entry.expires_at < now {
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
        let now_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        self.inner.last_active_time.store(now_time, AtomicOrdering::Relaxed);

        let mut wal_entry = BDBLogEntry::new(EntryType::Delete, key.clone(), Vec::new());
        self.inner.wal.write().log(&mut wal_entry)?;

        let shard = (key.first().cloned().unwrap_or(0) % 16) as usize;
        let mut mem = self.inner.memtable[shard].write();
        mem.put(key, Vec::new(), EntryType::Delete, 0);

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
        for (i, shard) in self.inner.memtable.iter().enumerate() {
            let guard = shard.read();
            iters.push(SourceIterator {
                iter: Box::new(MemTableIteratorWrapper::new(guard, prefix.to_vec())),
                source_id: i,
            });
        }

        // 2. SSTable Iterators
        let mut source_id = 16;
        for level in &self.inner.levels {
            let sstables = level.read();
            for sstable in sstables.iter() {
                if !prefix.is_empty() {
                    if let Some(bf) = &sstable.bloom_filter {
                        if !bf.might_contain(prefix) && !bf.might_contain(extract_prefix(prefix)) {
                            continue;
                        }
                    }
                }
                let sst_clone = Arc::clone(sstable);
                let cell = SSTableIterCell::new(sst_clone, |sst| sst.seek_prefix(prefix));

                iters.push(SourceIterator {
                    iter: Box::new(SSTableStreamWrapper { cell }),
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
                let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
                if deref_entry.expires_at > 0 && deref_entry.expires_at < now {
                    results.insert(key.clone(), None);
                    continue;
                }
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
                if !prefix.is_empty() {
                    if let Some(bf) = &sstable.bloom_filter {
                        if !bf.might_contain(prefix) && !bf.might_contain(extract_prefix(prefix)) {
                            continue;
                        }
                    }
                }
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
                            if kv.expires_at > 0 && kv.expires_at < SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64 {
                                continue;
                            }
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
        let sstable = Arc::new(SSTable::create(0, &all_entries, &self.inner.base_path, self.inner.table_type, None)?);
        
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

        let should_compact = {
            let levels = self.levels[level].read();
            if level == 0 {
                levels.len() >= self.config.lsm_tree.max_level0_files
            } else {
                let total_size: u64 = levels.iter().map(|s| s.mmap.len() as u64).sum();
                let threshold = self.config.lsm_tree.level_size_thresholds_mb.get(level - 1)
                    .cloned()
                    .unwrap_or(10 * 10usize.pow(level as u32 - 1)) as u64 * 1024 * 1024;
                total_size > threshold
            }
        };

        if should_compact {
            let &(ref lock, ref cvar) = &*self.compaction_state;
            let mut queue = lock.lock().unwrap();
            
            // Track active level compactions to prevent queueing duplicate tasks for the same level.
            let is_duplicate = queue.pending.iter().any(|t| t.level == level) || queue.active_levels.contains(&level);
            if !is_duplicate {
                queue.pending.push(CompactionTask {
                    level,
                    created_at: SystemTime::now(),
                });
                cvar.notify_all();
            }
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
                if let Err(e) = retry_on_permission_denied(|| std::fs::remove_file(&path)) {
                    eprintln!("Failed to remove SSTable file {}: {}", path.display(), e);
                }
            }

            // Cascade to next level if threshold exceeded
            if next_level < 9 {
                let (should_compact_next, _tables_next) = {
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
                    self.clone().trigger_compaction(next_level);
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
        let mut delta_sum: i64 = 0;
        let mut has_increments = false;
        let mut newest_entry: Option<KVEntry> = None;

        // 1. MemTable
        let shard = (key.first().cloned().unwrap_or(0) % 16) as usize;
        if let Some(entry_ref) = self.memtable[shard].read().get(key) {
            let entry = entry_ref.clone();
            if entry.entry_type == EntryType::Increment {
                has_increments = true;
                if entry.value.len() == 8 {
                    let mut arr = [0u8; 8];
                    arr.copy_from_slice(&entry.value);
                    delta_sum = delta_sum.wrapping_add(i64::from_le_bytes(arr));
                }
                if newest_entry.is_none() {
                    newest_entry = Some(entry.clone());
                }
            } else {
                return Some(entry);
            }
        }

        // 2. Levels (0 to 9)
        for level in &self.levels {
            let sstables = level.read();
            for sstable in sstables.iter().rev() {
                if let Some(entry) = sstable.get(key) {
                    if entry.entry_type == EntryType::Increment {
                        has_increments = true;
                        if entry.value.len() == 8 {
                            let mut arr = [0u8; 8];
                            arr.copy_from_slice(&entry.value);
                            delta_sum = delta_sum.wrapping_add(i64::from_le_bytes(arr));
                        }
                        if newest_entry.is_none() {
                            newest_entry = Some(entry.clone());
                        }
                    } else {
                        // Found a base entry.
                        if has_increments {
                            let mut final_entry = newest_entry.unwrap();
                            let base_val = if entry.value.len() == 8 {
                                let mut arr = [0u8; 8];
                                arr.copy_from_slice(&entry.value);
                                i64::from_le_bytes(arr)
                            } else {
                                0
                            };
                            let sum = base_val.wrapping_add(delta_sum);
                            final_entry.value = sum.to_le_bytes().to_vec();
                            final_entry.entry_type = EntryType::Insert;
                            final_entry.deleted = false;
                            return Some(final_entry);
                        } else {
                            return Some(entry);
                        }
                    }
                }
            }
        }

        // If we only found increments but no base, treat base as 0
        if has_increments {
            let mut final_entry = newest_entry.unwrap();
            final_entry.value = delta_sum.to_le_bytes().to_vec();
            final_entry.entry_type = EntryType::Insert;
            final_entry.deleted = false;
            return Some(final_entry);
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
            mem.put(k, v, entry_type, 0);
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
                let mut delta_sum: i64 = 0;
                let mut has_increments = false;

                for iter in iterators.iter_mut() {
                    let should_consume = if let Some(Ok(peeked)) = iter.peek() {
                        peeked.key == key
                    } else {
                        false
                    };

                    if should_consume {
                        if let Some(Ok(entry)) = iter.next() {
                            if entry.entry_type == EntryType::Increment {
                                has_increments = true;
                                if entry.value.len() == 8 {
                                    let mut arr = [0u8; 8];
                                    arr.copy_from_slice(&entry.value);
                                    let delta = i64::from_le_bytes(arr);
                                    delta_sum = delta_sum.wrapping_add(delta);
                                }
                                if best_entry.is_none() || entry.timestamp > best_entry.as_ref().unwrap().timestamp {
                                    best_entry = Some(entry.clone());
                                }
                            } else {
                                if best_entry.is_none() || entry.timestamp > best_entry.as_ref().unwrap().timestamp {
                                    best_entry = Some(entry);
                                }
                            }
                        }
                    }
                }

                if let Some(mut entry) = best_entry {
                    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
                    if entry.expires_at > 0 && entry.expires_at < now {
                        // Skip expired
                    } else {
                        if has_increments {
                            let base_val = if entry.entry_type != EntryType::Increment && entry.value.len() == 8 {
                                let mut arr = [0u8; 8];
                                arr.copy_from_slice(&entry.value);
                                i64::from_le_bytes(arr)
                            } else {
                                0
                            };
                            let final_val = base_val.wrapping_add(delta_sum);
                            entry.value = final_val.to_le_bytes().to_vec();
                            entry.entry_type = EntryType::Insert;
                        }

                        if !is_final_level || !entry.deleted {
                            merged_entries.insert(key, entry);
                        }
                    }
                }
            } else {
                break; // No more entries
            }
        }

        // Derive write rate limit in MB/s from compaction_cpu_limit (e.g. compaction_cpu_limit * 200.0 MB/s, default 0.05 -> 10.0 MB/s)
        let rate_limit = if self.config.lsm_tree.compaction_cpu_limit > 0.0 {
            Some(self.config.lsm_tree.compaction_cpu_limit * 200.0)
        } else {
            Some(10.0)
        };
        let new_sstable = Arc::new(SSTable::create(level, &merged_entries, &self.base_path, self.table_type, rate_limit)?);

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
