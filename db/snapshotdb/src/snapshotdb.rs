//! SnapshotDB implementation for RocksDB integration.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::Mutex;

use rocksdb::{DB, Options, ReadOptions, WriteBatch, WriteOptions};
use schnellru::{ByLength, LruMap};
use tracing::{error, trace, warn};

use alloy_primitives::B256;

use crate::traits::*;

use reth_metrics::{
    metrics::{Counter},
    Metrics,
};

/// Metrics for the `SnapshotDB`.
#[derive(Metrics, Clone)]
#[metrics(scope = "rust.eth.triedb.snapshotdb")]
pub(crate) struct SnapshotDBMetrics {
    /// Counter of cache hits
    pub(crate) cache_hits: Counter,
    /// Counter of cache misses
    pub(crate) cache_misses: Counter,
}

/// SnapshotDB implementation using RocksDB.
pub struct SnapshotDB {
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
    /// Metrics for the SnapshotDB.
    metrics: SnapshotDBMetrics,
}

impl Debug for SnapshotDB {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotDB")
            .field("config", &self.config)
            .finish()
    }
}

impl Clone for SnapshotDB {
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
            metrics: self.metrics.clone(),
        }
    }
}

impl SnapshotDB {
    /// Create a new SnapshotDB instance.
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
            metrics: SnapshotDBMetrics::new_with_labels(&[("instance", "default")]),
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
        warn!(target: "snapshotdb::rocksdb", "Clearing LRU cache");
        self.cache.lock().unwrap().clear();
    }

    /// Get cache statistics.
    pub fn cache_stats(&self) -> (usize, u32) {
        let cache = self.cache.lock().unwrap();
        (cache.len(), self.config.cache_size)
    }

    /// Create a new metrics instance for the SnapshotDB.
    pub fn with_new_metrics(&mut self, instance_name: &str) {
        self.metrics = SnapshotDBMetrics::new_with_labels(&[("instance", instance_name.to_string())]);
    }
}

impl PathProvider for SnapshotDB {
    fn get_raw(&self, key: &[u8]) -> PathProviderResult<Option<Vec<u8>>> {
        trace!(target: "snapshotdb::rocksdb", "Getting key: {:?}", key);

        // Check cache first
        {
            let cache = self.cache.lock().unwrap();
            if let Some(cached_value) = cache.peek(key) {
                self.metrics.cache_hits.increment(1);
                trace!(target: "snapshotdb::rocksdb", "Found value in cache for key: {:?}", key);
                return Ok(cached_value.clone());
            } else {
                self.metrics.cache_misses.increment(1);
            }
        }

        // Cache miss, read from DB
        match self.db.get_opt(key, &self.read_options) {
            Ok(Some(value)) => {
                trace!(target: "snapshotdb::rocksdb", "Found value in DB for key: {:?}", key);
                // Cache the value
                self.cache.lock().unwrap().insert(key.to_vec(), Some(value.to_vec()));
                Ok(Some(value))
            }
            Ok(None) => {
                trace!(target: "snapshotdb::rocksdb", "Key not found in DB: {:?}", key);
                Ok(None)
            }
            Err(e) => {
                error!(target: "snapshotdb::rocksdb", "Error getting key {:?}: {}", key, e);
                Err(PathProviderError::Database(format!("RocksDB get error: {}", e)))
            }
        }
    }

    fn put_raw(&self, key: &[u8], value: &[u8]) -> PathProviderResult<()> {
        trace!(target: "snapshotdb::rocksdb", "Putting key: {:?}, value_len: {}", key, value.len());

        // Update cache first
        self.cache.lock().unwrap().insert(key.to_vec(), Some(value.to_vec()));

        // Then write to DB
        match self.db.put_opt(key, value, &self.write_options) {
            Ok(()) => {
                trace!(target: "snapshotdb::rocksdb", "Successfully put key: {:?}", key);
                Ok(())
            }
            Err(e) => {
                error!(target: "snapshotdb::rocksdb", "Error putting key {:?}: {}", key, e);
                // Remove from cache on error
                self.cache.lock().unwrap().remove(key);
                Err(PathProviderError::Database(format!("RocksDB put error: {}", e)))
            }
        }
    }

    fn delete_raw(&self, key: &[u8]) -> PathProviderResult<()> {
        trace!(target: "snapshotdb::rocksdb", "Deleting key: {:?}", key);

        // Remove from cache first
        self.cache.lock().unwrap().remove(key);

        // Then delete from DB
        match self.db.delete_opt(key, &self.write_options) {
            Ok(()) => {
                trace!(target: "snapshotdb::rocksdb", "Successfully deleted key: {:?}", key);
                Ok(())
            }
            Err(e) => {
                error!(target: "snapshotdb::rocksdb", "Error deleting key {:?}: {}", key, e);
                Err(PathProviderError::Database(format!("RocksDB delete error: {}", e)))
            }
        }
    }

    fn exists_raw(&self, key: &[u8]) -> PathProviderResult<bool> {
        trace!(target: "snapshotdb::rocksdb", "Checking existence of key: {:?}", key);

        // Check cache first
        {
            let cache = self.cache.lock().unwrap();
            if let Some(cached_value) = cache.peek(key) {
                trace!(target: "snapshotdb::rocksdb", "Key exists in cache: {:?}", key);
                self.metrics.cache_hits.increment(1);
                return Ok(cached_value.is_some());
            } else {
                self.metrics.cache_misses.increment(1);
            }
        }

        // Cache miss, check DB
        match self.db.get_opt(key, &self.read_options) {
            Ok(Some(_)) => {
                trace!(target: "snapshotdb::rocksdb", "Key exists in DB: {:?}", key);
                // Cache the existence
                self.cache.lock().unwrap().insert(key.to_vec(), Some(vec![]));
                Ok(true)
            }
            Ok(None) => {
                trace!(target: "snapshotdb::rocksdb", "Key does not exist in DB: {:?}", key);
                Ok(false)
            }
            Err(e) => {
                error!(target: "snapshotdb::rocksdb", "Error checking existence of key {:?}: {}", key, e);
                Err(PathProviderError::Database(format!("RocksDB exists error: {}", e)))
            }
        }
    }

    fn get_multi(&self, keys: &[Vec<u8>]) -> PathProviderResult<HashMap<Vec<u8>, Vec<u8>>> {
        trace!(target: "snapshotdb::rocksdb", "Getting {} keys", keys.len());

        let mut result = HashMap::new();

        for key in keys {
            if let Some(value) = self.get_raw(key)? {
                result.insert(key.clone(), value);
            }
        }

        trace!(target: "snapshotdb::rocksdb", "Retrieved {} values", result.len());
        Ok(result)
    }

    fn put_multi(&self, kvs: &[(Vec<u8>, Vec<u8>)]) -> PathProviderResult<()> {
        trace!(target: "snapshotdb::rocksdb", "Putting {} key-value pairs", kvs.len());

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
                trace!(target: "snapshotdb::rocksdb", "Successfully put {} key-value pairs", kvs.len());
                Ok(())
            }
            Err(e) => {
                error!(target: "snapshotdb::rocksdb", "Error putting {} key-value pairs: {}", kvs.len(), e);
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
        trace!(target: "snapshotdb::rocksdb", "Deleting {} keys", keys.len());

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
                trace!(target: "snapshotdb::rocksdb", "Successfully deleted {} keys", keys.len());
                Ok(())
            }
            Err(e) => {
                error!(target: "snapshotdb::rocksdb", "Error deleting {} keys: {}", keys.len(), e);
                Err(PathProviderError::Database(format!("RocksDB delete_multi error: {}", e)))
            }
        }
    }
}

impl PathProviderManager for SnapshotDB {
    fn open(path: &str) -> PathProviderResult<Arc<dyn PathProvider>> {
        trace!(target: "snapshotdb::rocksdb", "Opening database at path: {}", path);

        let config = PathProviderConfig::default();
        let db = Self::new(path, config)?;
        Ok(Arc::new(db))
    }

    fn close(&self) -> PathProviderResult<()> {
        trace!(target: "snapshotdb::rocksdb", "Closing database");

        // RocksDB automatically closes when the last Arc is dropped
        Ok(())
    }

    fn flush(&self) -> PathProviderResult<()> {
        trace!(target: "snapshotdb::rocksdb", "Flushing database");

        match self.db.flush() {
            Ok(()) => {
                trace!(target: "snapshotdb::rocksdb", "Successfully flushed database");
                Ok(())
            }
            Err(e) => {
                error!(target: "snapshotdb::rocksdb", "Error flushing database: {}", e);
                Err(PathProviderError::Database(format!("Flush error: {}", e)))
            }
        }
    }

    fn compact(&self) -> PathProviderResult<()> {
        trace!(target: "snapshotdb::rocksdb", "Compacting database");

        // Simplified compact implementation
        Ok(())
    }
}

impl SnapshotDB {
    pub fn get_storage_root(&self, hash_address: B256) -> PathProviderResult<Option<B256>> {
        let key = hash_address.as_slice();
        if let Some(value) = self.get_raw(key)? {
            if value.len() == 32 {
                Ok(Some(B256::from_slice(&value)))
            } else {
                panic!("Storage root value length is not 32 for address: {:?}, value_len: {:?}", hash_address, value.len());
            }
        } else {
            // TODO:: if none will return empty root hash, after building full storage root snapshot database
            Ok(None)
        }
    }

    pub fn bacth_insert_storage_root(&self, storage_roots: HashMap<B256, B256>) -> PathProviderResult<()> {
        
        if storage_roots.is_empty() {
            return Ok(());
        }

        let count = storage_roots.len();
        let keys: Vec<B256> = storage_roots.keys().cloned().collect();
        let mut batch = WriteBatch::default();
        
        // Update cache first
        {
            let mut cache = self.cache.lock().unwrap();
            
            for (key, value) in storage_roots {
                let key_bytes = key.as_slice();
                let value_bytes = value.as_slice();
                cache.insert(key_bytes.to_vec(), Some(value_bytes.to_vec()));
                // storage_roots can not be none, default is empty root hash
                batch.put(key_bytes, value_bytes);
            }
        }

        match self.db.write_opt(batch, &self.write_options) {
            Ok(()) => {
                trace!(target: "snapshotdb::rocksdb", "Successfully put {} key-value pairs", count);
                Ok(())
            }
            Err(e) => {
                error!(target: "snapshotdb::rocksdb", "Error putting {} key-value pairs: {}", count, e);
                // Remove from cache on error
                let mut cache = self.cache.lock().unwrap();
                for key in keys {
                    cache.remove(key.as_slice());
                }
                Err(PathProviderError::Database(format!("RocksDB put_multi error: {}", e)))
            }
        }
    }
}