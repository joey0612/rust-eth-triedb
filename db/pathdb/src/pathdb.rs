//! PathDB implementation for RocksDB integration.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::Mutex;

use rocksdb::{DB, Options, ReadOptions, WriteBatch, WriteOptions};
use schnellru::{ByLength, LruMap};
use tracing::{error, trace};

use crate::traits::*;
use rust_eth_triedb_common::{TrieDatabase,TrieDatabaseBatch};

/// PathDB implementation using RocksDB.
pub struct PathDB {
    /// The underlying RocksDB instance.
    db: Arc<DB>,
    /// Configuration for the database.
    config: PathProviderConfig,
    /// Write options for batch operations.
    write_options: WriteOptions,
    /// Read options for read operations.
    read_options: ReadOptions,
    /// LRU cache for key-value pairs.
    cache: Arc<Mutex<LruMap<Vec<u8>, Option<Vec<u8>>, ByLength>>>,
}

impl<'a> Debug for PathDB {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PathDB")
            .field("config", &self.config)
            .finish()
    }
}

impl<'a> Clone for PathDB {
    fn clone(&self) -> Self {
        let write_options = WriteOptions::default();
        let mut read_options = ReadOptions::default();
        read_options.fill_cache(self.config.fill_cache);
        read_options.set_readahead_size(self.config.readahead_size);
        read_options.set_async_io(self.config.async_io);
        read_options.set_verify_checksums(self.config.verify_checksums);

        Self {
            db: self.db.clone(),
            config: self.config.clone(),
            write_options,
            read_options,
            cache: self.cache.clone(),
        }
    }
}

impl<'a> PathDB {
    /// Create a new PathDB instance.
    pub fn new(path: &str, config: PathProviderConfig) -> PathProviderResult<Self> {
        let mut db_opts = Options::default();
        db_opts.set_max_open_files(config.max_open_files);
        db_opts.set_write_buffer_size(config.write_buffer_size);
        db_opts.set_max_write_buffer_number(config.max_write_buffer_number);
        db_opts.set_target_file_size_base(config.target_file_size_base);
        db_opts.set_max_background_jobs(config.max_background_jobs);
        db_opts.create_if_missing(config.create_if_missing);

        let db = DB::open(&db_opts, path)
            .map_err(|e| PathProviderError::Database(format!("Failed to open RocksDB: {}", e)))?;

        let write_options = WriteOptions::default();

        let mut read_options = ReadOptions::default();
        read_options.fill_cache(config.fill_cache);
        read_options.set_readahead_size(config.readahead_size);
        read_options.set_async_io(config.async_io);
        read_options.set_verify_checksums(config.verify_checksums);

        let cache_size = config.cache_size;

        Ok(Self {
            db: Arc::new(db),
            config,
            write_options,
            read_options,
            cache: Arc::new(Mutex::new(LruMap::new(ByLength::new(cache_size)))),
        })
    }

    /// Get the underlying RocksDB instance.
    pub fn inner(&self) -> &Arc<DB> {
        &self.db
    }

    /// Get the configuration.
    pub fn config(&self) -> &PathProviderConfig {
        &self.config
    }

    /// Clear the LRU cache.
    pub fn clear_cache(&self) {
        trace!(target: "pathdb::rocksdb", "Clearing LRU cache");
        self.cache.lock().unwrap().clear();
    }

    /// Get cache statistics.
    pub fn cache_stats(&self) -> (usize, u32) {
        let cache = self.cache.lock().unwrap();
        (cache.len(), self.config.cache_size)
    }
}

impl PathProvider for PathDB {
    fn get_raw(&self, key: &[u8]) -> PathProviderResult<Option<Vec<u8>>> {
        trace!(target: "pathdb::rocksdb", "Getting key: {:?}", key);

        // Check cache first
        {
            let cache = self.cache.lock().unwrap();
            if let Some(cached_value) = cache.peek(key) {
                trace!(target: "pathdb::rocksdb", "Found value in cache for key: {:?}", key);
                return Ok(cached_value.clone());
            }
        }

        // Cache miss, read from DB
        match self.db.get_opt(key, &self.read_options) {
            Ok(Some(value)) => {
                trace!(target: "pathdb::rocksdb", "Found value in DB for key: {:?}", key);
                // Cache the value
                self.cache.lock().unwrap().insert(key.to_vec(), Some(value.to_vec()));
                Ok(Some(value))
            }
            Ok(None) => {
                trace!(target: "pathdb::rocksdb", "Key not found in DB: {:?}", key);
                Ok(None)
            }
            Err(e) => {
                error!(target: "pathdb::rocksdb", "Error getting key {:?}: {}", key, e);
                Err(PathProviderError::Database(format!("RocksDB get error: {}", e)))
            }
        }
    }

    fn put_raw(&self, key: &[u8], value: &[u8]) -> PathProviderResult<()> {
        trace!(target: "pathdb::rocksdb", "Putting key: {:?}, value_len: {}", key, value.len());

        // Update cache first
        self.cache.lock().unwrap().insert(key.to_vec(), Some(value.to_vec()));

        // Then write to DB
        match self.db.put_opt(key, value, &self.write_options) {
            Ok(()) => {
                trace!(target: "pathdb::rocksdb", "Successfully put key: {:?}", key);
                Ok(())
            }
            Err(e) => {
                error!(target: "pathdb::rocksdb", "Error putting key {:?}: {}", key, e);
                // Remove from cache on error
                self.cache.lock().unwrap().remove(key);
                Err(PathProviderError::Database(format!("RocksDB put error: {}", e)))
            }
        }
    }

    fn delete_raw(&self, key: &[u8]) -> PathProviderResult<()> {
        trace!(target: "pathdb::rocksdb", "Deleting key: {:?}", key);

        // Remove from cache first
        self.cache.lock().unwrap().remove(key);

        // Then delete from DB
        match self.db.delete_opt(key, &self.write_options) {
            Ok(()) => {
                trace!(target: "pathdb::rocksdb", "Successfully deleted key: {:?}", key);
                Ok(())
            }
            Err(e) => {
                error!(target: "pathdb::rocksdb", "Error deleting key {:?}: {}", key, e);
                Err(PathProviderError::Database(format!("RocksDB delete error: {}", e)))
            }
        }
    }

    fn exists_raw(&self, key: &[u8]) -> PathProviderResult<bool> {
        trace!(target: "pathdb::rocksdb", "Checking existence of key: {:?}", key);

        // Check cache first
        {
            let cache = self.cache.lock().unwrap();
            if let Some(cached_value) = cache.peek(key) {
                trace!(target: "pathdb::rocksdb", "Key exists in cache: {:?}", key);
                return Ok(cached_value.is_some());
            }
        }

        // Cache miss, check DB
        match self.db.get_opt(key, &self.read_options) {
            Ok(Some(_)) => {
                trace!(target: "pathdb::rocksdb", "Key exists in DB: {:?}", key);
                // Cache the existence
                self.cache.lock().unwrap().insert(key.to_vec(), Some(vec![]));
                Ok(true)
            }
            Ok(None) => {
                trace!(target: "pathdb::rocksdb", "Key does not exist in DB: {:?}", key);
                Ok(false)
            }
            Err(e) => {
                error!(target: "pathdb::rocksdb", "Error checking existence of key {:?}: {}", key, e);
                Err(PathProviderError::Database(format!("RocksDB exists error: {}", e)))
            }
        }
    }

    fn get_multi(&self, keys: &[Vec<u8>]) -> PathProviderResult<HashMap<Vec<u8>, Vec<u8>>> {
        trace!(target: "pathdb::rocksdb", "Getting {} keys", keys.len());

        let mut result = HashMap::new();

        for key in keys {
            if let Some(value) = self.get_raw(key)? {
                result.insert(key.clone(), value);
            }
        }

        trace!(target: "pathdb::rocksdb", "Retrieved {} values", result.len());
        Ok(result)
    }

    fn put_multi(&self, kvs: &[(Vec<u8>, Vec<u8>)]) -> PathProviderResult<()> {
        trace!(target: "pathdb::rocksdb", "Putting {} key-value pairs", kvs.len());

        // Update cache first
        {
            let mut cache = self.cache.lock().unwrap();
            for (key, value) in kvs {
                cache.insert(key.clone(), Some(value.clone()));
            }
        }

        // Then write to DB
        let mut batch = WriteBatch::default();

        for (key, value) in kvs {
            batch.put(key, value);
        }

        match self.db.write_opt(batch, &self.write_options) {
            Ok(()) => {
                trace!(target: "pathdb::rocksdb", "Successfully put {} key-value pairs", kvs.len());
                Ok(())
            }
            Err(e) => {
                error!(target: "pathdb::rocksdb", "Error putting {} key-value pairs: {}", kvs.len(), e);
                // Remove from cache on error
                let mut cache = self.cache.lock().unwrap();
                for (key, _) in kvs {
                    cache.remove(key);
                }
                Err(PathProviderError::Database(format!("RocksDB put_multi error: {}", e)))
            }
        }
    }

    fn delete_multi(&self, keys: &[Vec<u8>]) -> PathProviderResult<()> {
        trace!(target: "pathdb::rocksdb", "Deleting {} keys", keys.len());

        // Remove from cache first
        {
            let mut cache = self.cache.lock().unwrap();
            for key in keys {
                cache.remove(key);
            }
        }

        // Then delete from DB
        let mut batch = WriteBatch::default();

        for key in keys {
            batch.delete(key);
        }

        match self.db.write_opt(batch, &self.write_options) {
            Ok(()) => {
                trace!(target: "pathdb::rocksdb", "Successfully deleted {} keys", keys.len());
                Ok(())
            }
            Err(e) => {
                error!(target: "pathdb::rocksdb", "Error deleting {} keys: {}", keys.len(), e);
                Err(PathProviderError::Database(format!("RocksDB delete_multi error: {}", e)))
            }
        }
    }
}

impl PathProviderManager for PathDB {
    fn open(path: &str) -> PathProviderResult<Arc<dyn PathProvider>> {
        trace!(target: "pathdb::rocksdb", "Opening database at path: {}", path);

        let config = PathProviderConfig::default();
        let db = Self::new(path, config)?;
        Ok(Arc::new(db))
    }

    fn close(&self) -> PathProviderResult<()> {
        trace!(target: "pathdb::rocksdb", "Closing database");

        // RocksDB automatically closes when the last Arc is dropped
        Ok(())
    }

    fn flush(&self) -> PathProviderResult<()> {
        trace!(target: "pathdb::rocksdb", "Flushing database");

        match self.db.flush() {
            Ok(()) => {
                trace!(target: "pathdb::rocksdb", "Successfully flushed database");
                Ok(())
            }
            Err(e) => {
                error!(target: "pathdb::rocksdb", "Error flushing database: {}", e);
                Err(PathProviderError::Database(format!("Flush error: {}", e)))
            }
        }
    }

    fn compact(&self) -> PathProviderResult<()> {
        trace!(target: "pathdb::rocksdb", "Compacting database");

        // Simplified compact implementation
        Ok(())
    }
}

impl TrieDatabase for PathDB {
    type Error = PathProviderError;
    type Batch = PathDBBatch;

    fn get(&self, path: &[u8]) -> Result<Option<Vec<u8>>, Self::Error> {
        self.get_raw(path)
    }

    fn insert(&self, path: &[u8], data: Vec<u8>) -> Result<(), Self::Error> {
        self.put_raw( path, &data)
    }

    fn contains(&self, path: &[u8]) -> Result<bool, Self::Error> {
        self.exists_raw( path)
    }

    fn remove(&self, path: &[u8]) {
        let _ = self.delete_raw( path);
    }

    fn create_batch(&self) -> Result<Self::Batch, Self::Error> {
        Ok(PathDBBatch::new())
    }

    fn batch_commit(&self, batch: Self::Batch) -> Result<(), Self::Error> {
        match self.db.write_opt(batch.batch, &self.write_options) {
            Ok(()) => {
                {
                    // Update cache with batch operations
                    let mut cache = self.cache.lock().unwrap();
                    for (key, value) in &batch.operations {
                        match value {
                            Some(val) => {
                                // Insert or update operation
                                cache.insert(key.clone(), Some(val.clone()));
                            }
                            None => {
                                // Delete operation
                                cache.remove(key);
                            }
                        }
                    }
                }
                trace!(target: "pathdb::batch", "Successfully committed batch to database");
                Ok(())
            }
            Err(e) => {
                error!(target: "pathdb::batch", "Error committing batch: {}", e);
                Err(PathProviderError::Database(format!("Batch commit error: {}", e)))
            }
        }
    }
}

/// PathDB batch implementation using RocksDB WriteBatch
pub struct PathDBBatch {
    /// The underlying RocksDB WriteBatch
    pub batch: WriteBatch,
    /// Track operations for cache updates
    operations: Vec<(Vec<u8>, Option<Vec<u8>>)>, // (key, value) where None means delete
}

impl PathDBBatch {
    /// Create a new PathDB batch
    pub fn new() -> Self {
        Self {
            batch: WriteBatch::default(),
            operations: Vec::new(),
        }
    }
}

impl TrieDatabaseBatch for PathDBBatch {
    type Error = PathProviderError;

    fn insert(&mut self, key: &[u8], value: Vec<u8>) -> Result<(), Self::Error> {
        self.batch.put(key, &value);
        self.operations.push((key.to_vec(), Some(value)));
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<(), Self::Error> {
        self.batch.delete(key);
        self.operations.push((key.to_vec(), None));
        Ok(())
    }

    fn is_empty(&self) -> bool {
        self.batch.is_empty()
    }

    fn len(&self) -> usize {
        self.batch.len()
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.batch.clear();
        self.operations.clear();
        Ok(())
    }
}


