use std::io::{self, Read, Write};
use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian};
use crc32fast::Hasher;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAGIC_BYTES: &[u8; 9] = b"BROWSERDB";
pub const BDB_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TableType {
    History = 1,
    Cookies = 2,
    Cache = 3,
    LocalStore = 4,
    Settings = 5,
}

impl From<u8> for TableType {
    fn from(v: u8) -> Self {
        match v {
            1 => TableType::History,
            2 => TableType::Cookies,
            3 => TableType::Cache,
            4 => TableType::LocalStore,
            5 => TableType::Settings,
            _ => TableType::History,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EntryType {
    Insert = 1,
    Update = 2,
    Delete = 3,
    BatchStart = 4,
    BatchEnd = 5,
}

impl From<u8> for EntryType {
    fn from(v: u8) -> Self {
        match v {
            1 => EntryType::Insert,
            2 => EntryType::Update,
            3 => EntryType::Delete,
            4 => EntryType::BatchStart,
            5 => EntryType::BatchEnd,
            _ => EntryType::Insert,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CompressionType {
    None = 0,
    Zlib = 1,
    Lz4 = 2,
    Zstd = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EncryptionType {
    None = 0,
    AES256 = 1,
    ChaCha20 = 2,
}

#[derive(Debug, Clone)]
pub struct BDBFileHeader {
    pub magic: [u8; 9],
    pub version: u8,
    pub created_at: u64,
    pub modified_at: u64,
    pub flags: u32,
    pub reserved: u32,
    pub table_type: TableType,
    pub compression: CompressionType,
    pub encryption: EncryptionType,
    pub reserved_bytes: [u8; 6],
    pub header_crc: u32,
}

impl BDBFileHeader {
    pub fn new(table_type: TableType) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            magic: *MAGIC_BYTES,
            version: BDB_VERSION,
            created_at: timestamp,
            modified_at: timestamp,
            flags: 0,
            reserved: 0,
            table_type,
            compression: CompressionType::None,
            encryption: EncryptionType::None,
            reserved_bytes: [0; 6],
            header_crc: 0,
        }
    }

    pub fn calculate_crc(&self) -> u32 {
        let mut hasher = Hasher::new();
        hasher.update(&self.magic);
        hasher.update(&[self.version]);
        hasher.update(&self.created_at.to_le_bytes());
        hasher.update(&self.modified_at.to_le_bytes());
        hasher.update(&self.flags.to_le_bytes());
        hasher.update(&self.reserved.to_le_bytes());
        hasher.update(&[self.table_type as u8]);
        hasher.update(&[self.compression as u8]);
        hasher.update(&[self.encryption as u8]);
        hasher.update(&self.reserved_bytes);
        hasher.finalize()
    }

    pub fn write<W: Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.header_crc = self.calculate_crc();
        
        writer.write_all(&self.magic)?;
        writer.write_u8(self.version)?;
        writer.write_u64::<LittleEndian>(self.created_at)?;
        writer.write_u64::<LittleEndian>(self.modified_at)?;
        writer.write_u32::<LittleEndian>(self.flags)?;
        writer.write_u32::<LittleEndian>(self.reserved)?;
        writer.write_u8(self.table_type as u8)?;
        writer.write_u8(self.compression as u8)?;
        writer.write_u8(self.encryption as u8)?;
        writer.write_all(&self.reserved_bytes)?;
        writer.write_u32::<LittleEndian>(self.header_crc)?;
        
        Ok(())
    }

    pub fn read<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut magic = [0u8; 9];
        reader.read_exact(&mut magic)?;
        if &magic != MAGIC_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid Magic Bytes"));
        }

        let version = reader.read_u8()?;

        let created_at = reader.read_u64::<LittleEndian>()?;
        let modified_at = reader.read_u64::<LittleEndian>()?;
        let flags = reader.read_u32::<LittleEndian>()?;
        let reserved = reader.read_u32::<LittleEndian>()?;
        let table_type = reader.read_u8()?.into();
        let compression = match reader.read_u8()? {
            0 => CompressionType::None,
            1 => CompressionType::Zlib,
            2 => CompressionType::Lz4,
            3 => CompressionType::Zstd,
            _ => CompressionType::None,
        };
        let encryption = match reader.read_u8()? {
            0 => EncryptionType::None,
            1 => EncryptionType::AES256,
            2 => EncryptionType::ChaCha20,
            _ => EncryptionType::None,
        };
        
        let mut reserved_bytes = [0u8; 6];
        reader.read_exact(&mut reserved_bytes)?;
        
        let header_crc = reader.read_u32::<LittleEndian>()?;

        let header = Self {
            magic,
            version,
            created_at,
            modified_at,
            flags,
            reserved,
            table_type,
            compression,
            encryption,
            reserved_bytes,
            header_crc,
        };

        if header.calculate_crc() != header_crc {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Header CRC Mismatch"));
        }

        Ok(header)
    }
}

#[derive(Debug, Clone)]
pub struct BDBLogEntry {
    pub entry_type: EntryType,
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub timestamp: u64,
    pub entry_crc: u32,
}

impl BDBLogEntry {
    pub fn new(entry_type: EntryType, key: Vec<u8>, value: Vec<u8>) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
            
        Self {
            entry_type,
            key,
            value,
            timestamp,
            entry_crc: 0,
        }
    }

    pub fn calculate_crc(&self) -> u32 {
        let mut hasher = Hasher::new();
        hasher.update(&[self.entry_type as u8]);
        hasher.update(&self.key);
        hasher.update(&self.value);
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.finalize()
    }

    pub fn write<W: Write>(&mut self, writer: &mut W) -> io::Result<usize> {
        self.entry_crc = self.calculate_crc();
        
        let mut bytes_written = 0;
        
        writer.write_u8(self.entry_type as u8)?;
        bytes_written += 1;
        
        bytes_written += write_varint(writer, self.key.len() as u64)?;
        bytes_written += write_varint(writer, self.value.len() as u64)?;
        
        writer.write_all(&self.key)?;
        bytes_written += self.key.len();
        
        writer.write_all(&self.value)?;
        bytes_written += self.value.len();
        
        writer.write_u64::<LittleEndian>(self.timestamp)?;
        bytes_written += 8;
        
        writer.write_u32::<LittleEndian>(self.entry_crc)?;
        bytes_written += 4;
        
        Ok(bytes_written)
    }

    pub fn read<R: Read>(reader: &mut R) -> io::Result<Self> {
        let entry_type = reader.read_u8()?.into();
        let key_len = read_varint(reader)?;
        let value_len = read_varint(reader)?;
        
        let mut key = vec![0u8; key_len as usize];
        reader.read_exact(&mut key)?;
        
        let mut value = vec![0u8; value_len as usize];
        reader.read_exact(&mut value)?;
        
        let timestamp = reader.read_u64::<LittleEndian>()?;
        let entry_crc = reader.read_u32::<LittleEndian>()?;
        
        Ok(Self {
            entry_type,
            key,
            value,
            timestamp,
            entry_crc,
        })
    }
}

#[derive(Debug, Clone)]
pub struct BDBFileFooter {
    pub entry_count: u64,
    pub file_size: u64,
    pub data_offset: u64,
    pub max_entry_size: u32,
    pub total_key_size: u64,
    pub total_value_size: u64,
    pub compression_ratio: u16,
    pub reserved: [u8; 2],
    pub file_crc: u32,
}

impl BDBFileFooter {
    pub fn new() -> Self {
        Self {
            entry_count: 0,
            file_size: 0,
            data_offset: 0,
            max_entry_size: 0,
            total_key_size: 0,
            total_value_size: 0,
            compression_ratio: 100,
            reserved: [0; 2],
            file_crc: 0,
        }
    }

    pub fn write<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_u64::<LittleEndian>(self.entry_count)?;
        writer.write_u64::<LittleEndian>(self.file_size)?;
        writer.write_u64::<LittleEndian>(self.data_offset)?;
        writer.write_u32::<LittleEndian>(self.max_entry_size)?;
        writer.write_u64::<LittleEndian>(self.total_key_size)?;
        writer.write_u64::<LittleEndian>(self.total_value_size)?;
        writer.write_u16::<LittleEndian>(self.compression_ratio)?;
        writer.write_all(&self.reserved)?;
        writer.write_u32::<LittleEndian>(self.file_crc)?;
        Ok(())
    }

    pub fn read<R: Read>(reader: &mut R) -> io::Result<Self> {
        let entry_count = reader.read_u64::<LittleEndian>()?;
        let file_size = reader.read_u64::<LittleEndian>()?;
        let data_offset = reader.read_u64::<LittleEndian>()?;
        let max_entry_size = reader.read_u32::<LittleEndian>()?;
        let total_key_size = reader.read_u64::<LittleEndian>()?;
        let total_value_size = reader.read_u64::<LittleEndian>()?;
        let compression_ratio = reader.read_u16::<LittleEndian>()?;
        
        let mut reserved = [0u8; 2];
        reader.read_exact(&mut reserved)?;
        
        let file_crc = reader.read_u32::<LittleEndian>()?;

        Ok(Self {
            entry_count,
            file_size,
            data_offset,
            max_entry_size,
            total_key_size,
            total_value_size,
            compression_ratio,
            reserved,
            file_crc,
        })
    }
}

pub fn write_varint<W: Write>(writer: &mut W, mut value: u64) -> io::Result<usize> {
    let mut bytes_written = 0;
    while value >= 0x80 {
        writer.write_u8(((value & 0x7F) | 0x80) as u8)?;
        value >>= 7;
        bytes_written += 1;
    }
    writer.write_u8(value as u8)?;
    bytes_written += 1;
    Ok(bytes_written)
}

pub fn read_varint<R: Read>(reader: &mut R) -> io::Result<u64> {
    let mut result = 0;
    let mut shift = 0;
    loop {
        let byte = reader.read_u8()?;
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 63 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "VarInt too large"));
        }
    }
    Ok(result)
}