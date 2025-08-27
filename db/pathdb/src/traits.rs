//! PathProvider trait definitions for key-value database operations.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

// Default configuration constants
pub const DEFAULT_MAX_OPEN_FILES: i32 = 10000000;
pub const DEFAULT_WRITE_BUFFER_SIZE: usize = 4 * 1024 * 1024 * 1024; // 4GB
pub const DEFAULT_MAX_WRITE_BUFFER_NUMBER: i32 = 4;
pub const DEFAULT_TARGET_FILE_SIZE_BASE: u64 = 64 * 1024 * 1024; // 64MB
pub const DEFAULT_MAX_BACKGROUND_JOBS: i32 = 4;
pub const DEFAULT_CREATE_IF_MISSING: bool = true;
pub const DEFAULT_CACHE_SIZE: u32 = 20_000_000; // 2KM entries

// ReadOptions configuration constants
pub const DEFAULT_FILL_CACHE: bool = true;
pub const DEFAULT_READAHEAD_SIZE: usize = 128 * 1024; // 128KB
pub const DEFAULT_ASYNC_IO: bool = true;
pub const DEFAULT_VERIFY_CHECKSUMS: bool = false;

/// Result type for PathProvider operations.
pub type PathProviderResult<T> = Result<T, PathProviderError>;

/// Error type for PathProvider operations.
#[derive(Debug, thiserror::Error)]
pub enum PathProviderError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Deserialization error: {0}")]
    Deserialization(String),
    #[error("Key not found: {0:?}")]
    KeyNotFound(Vec<u8>),
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

/// Trait for basic key-value database operations.
pub trait PathProvider: Send + Sync + Debug {
    /// Get a value by key.
    fn get_raw(&self, key: &[u8]) -> PathProviderResult<Option<Vec<u8>>>;

    /// Put a key-value pair.
    fn put_raw(&self, key: &[u8], value: &[u8]) -> PathProviderResult<()>;

    /// Delete a key.
    fn delete_raw(&self, key: &[u8]) -> PathProviderResult<()>;

    /// Check if a key exists.
    fn exists_raw(&self, key: &[u8]) -> PathProviderResult<bool>;

    /// Get multiple values by keys.
    fn get_multi(&self, keys: &[Vec<u8>]) -> PathProviderResult<HashMap<Vec<u8>, Vec<u8>>>;

    /// Put multiple key-value pairs.
    fn put_multi(&self, kvs: &[(Vec<u8>, Vec<u8>)]) -> PathProviderResult<()>;

    /// Delete multiple keys.
    fn delete_multi(&self, keys: &[Vec<u8>]) -> PathProviderResult<()>;
}

/// Trait for database management operations.
pub trait PathProviderManager: Send + Sync + Debug {
    /// Open or create a database at the given path.
    fn open(path: &str) -> PathProviderResult<Arc<dyn PathProvider>>;

    /// Close the database.
    fn close(&self) -> PathProviderResult<()>;

    /// Flush all pending writes to disk.
    fn flush(&self) -> PathProviderResult<()>;

    /// Compact the database.
    fn compact(&self) -> PathProviderResult<()>;
}

/// Configuration for PathProvider.
#[derive(Debug, Clone)]
pub struct PathProviderConfig {
    /// Maximum number of open files.
    pub max_open_files: i32,
    /// Write buffer size in bytes.
    pub write_buffer_size: usize,
    /// Maximum write buffer number.
    pub max_write_buffer_number: i32,
    /// Target file size for compaction.
    pub target_file_size_base: u64,
    /// Maximum background jobs.
    pub max_background_jobs: i32,
    /// Whether to create the database if it doesn't exist.
    pub create_if_missing: bool,
    /// LRU cache size in number of entries (default: 1M entries).
    pub cache_size: u32,
    /// Whether to fill cache on reads.
    pub fill_cache: bool,
    /// Readahead size in bytes for sequential reads.
    pub readahead_size: usize,
    /// Whether to enable async IO for reads.
    pub async_io: bool,
    /// Whether to verify checksums on reads.
    pub verify_checksums: bool,
}

impl Default for PathProviderConfig {
    fn default() -> Self {
        Self {
            max_open_files: DEFAULT_MAX_OPEN_FILES,
            write_buffer_size: DEFAULT_WRITE_BUFFER_SIZE,
            max_write_buffer_number: DEFAULT_MAX_WRITE_BUFFER_NUMBER,
            target_file_size_base: DEFAULT_TARGET_FILE_SIZE_BASE,
            max_background_jobs: DEFAULT_MAX_BACKGROUND_JOBS,
            create_if_missing: DEFAULT_CREATE_IF_MISSING,
            cache_size: DEFAULT_CACHE_SIZE,
            fill_cache: DEFAULT_FILL_CACHE,
            readahead_size: DEFAULT_READAHEAD_SIZE,
            async_io: DEFAULT_ASYNC_IO,
            verify_checksums: DEFAULT_VERIFY_CHECKSUMS,
        }
    }
}
