//! PathDB implementation for RocksDB integration.

use std::collections::HashSet;
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::Mutex;

use rocksdb::{ColumnFamilyDescriptor,DB, Options, ReadOptions, WriteBatch, WriteOptions};
use schnellru::{ByLength, LruMap};
use tracing::{error, trace, warn};

use alloy_primitives::B256;
use crate::traits::*;
use rust_eth_triedb_common::{TrieDatabase, DiffLayer, TRIE_STATE_ROOT_KEY, TRIE_STATE_BLOCK_NUMBER_KEY};

use reth_metrics::{
    metrics::{Counter},
    Metrics,
};

const DEFAULT_COLUMN_FAMILY_NAME: &str = "default";
const STORAGE_ROOT_COLUMN_FAMILY_NAME: &str = "storage_root";

const COLUMN_FAMILY_NAMES: [&str; 2] = [DEFAULT_COLUMN_FAMILY_NAME, STORAGE_ROOT_COLUMN_FAMILY_NAME];

/// Metrics for the `TrieDB`.
#[derive(Metrics, Clone)]
#[metrics(scope = "rust.eth.triedb.pathdb")]
pub(crate) struct PathDBMetrics {
    /// Counter of cache hits
    pub(crate) cache_hits: Counter,
    /// Counter of cache misses
    pub(crate) cache_misses: Counter,
}

/// PathDB implementation using RocksDB.
pub struct PathDB {
    /// The underlying RocksDB instance.
    pub db: Arc<DB>,
    /// Set of Column Family names that exist in the database.
    column_family_names: Arc<Mutex<HashSet<String>>>,
    /// Configuration for the database.
    pub config: PathProviderConfig,
    /// Write options for batch operations.
    pub write_options: WriteOptions,
    /// Read options for read operations.
    pub read_options: ReadOptions,
    /// LRU cache for key-value pairs.
    pub cache: Arc<Mutex<LruMap<Vec<u8>, Option<Vec<u8>>, ByLength>>>,
    /// Metrics for the PathDB.
    metrics: PathDBMetrics,
}

impl Debug for PathDB {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PathDB")
            .field("config", &self.config)
            .field("column_family_names", &self.column_family_names)
            .finish()
    }
}

impl Clone for PathDB {
    fn clone(&self) -> Self {
        let write_options = WriteOptions::default();
        let mut read_options = ReadOptions::default();
        read_options.fill_cache(self.config.fill_cache);
        read_options.set_readahead_size(self.config.readahead_size);
        read_options.set_async_io(self.config.async_io);
        read_options.set_verify_checksums(self.config.verify_checksums);

        Self {
            db: self.db.clone(),
            column_family_names: self.column_family_names.clone(),
            config: self.config.clone(),
            write_options,
            read_options,
            cache: self.cache.clone(),
            metrics: self.metrics.clone(),
        }
    }
}

impl PathDB {
    /// Create a new PathDB instance.
    pub fn new(path: &str, config: PathProviderConfig) -> PathProviderResult<Self> {
        let mut db_opts = Options::default();
        db_opts.set_max_open_files(config.max_open_files);
        db_opts.set_write_buffer_size(config.write_buffer_size);
        db_opts.set_max_write_buffer_number(config.max_write_buffer_number);
        db_opts.set_target_file_size_base(config.target_file_size_base);
        db_opts.set_max_background_jobs(config.max_background_jobs);
        db_opts.create_if_missing(config.create_if_missing);

        // Ensure all required Column Families exist
        ensure_column_families(path, &db_opts, &config)?;

        // Now open database with all required Column Families
        let mut cf_descriptors = Vec::new();
        for cf_name in COLUMN_FAMILY_NAMES {
            let mut cf_opts = Options::default();
            cf_opts.set_max_write_buffer_number(config.max_write_buffer_number);
            cf_opts.set_write_buffer_size(config.write_buffer_size);
            cf_descriptors.push(ColumnFamilyDescriptor::new(cf_name, cf_opts));
        }

        let db = DB::open_cf_descriptors(&db_opts, path, cf_descriptors)
            .map_err(|e| PathProviderError::Database(format!("Failed to open RocksDB: {}", e)))?;

        let cf_names_set: HashSet<String> = COLUMN_FAMILY_NAMES.iter().map(|s| s.to_string()).collect();

        let write_options = WriteOptions::default();

        let mut read_options = ReadOptions::default();
        read_options.fill_cache(config.fill_cache);
        read_options.set_readahead_size(config.readahead_size);
        read_options.set_async_io(config.async_io);
        read_options.set_verify_checksums(config.verify_checksums);

        let cache_size = config.cache_size;

        Ok(Self {
            db: Arc::new(db),
            column_family_names: Arc::new(Mutex::new(cf_names_set)),
            config,
            write_options,
            read_options,
            cache: Arc::new(Mutex::new(LruMap::new(ByLength::new(cache_size)))),
            metrics: PathDBMetrics::new_with_labels(&[("instance", "default")]),
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
        warn!(target: "pathdb::rocksdb", "Clearing LRU cache");
        self.cache.lock().unwrap().clear();
    }

    /// Get cache statistics.
    pub fn cache_stats(&self) -> (usize, u32) {
        let cache = self.cache.lock().unwrap();
        (cache.len(), self.config.cache_size)
    }

    /// Create a new metrics instance for the PathDB.
    pub fn with_new_metrics(&mut self, instance_name: &str) {
        self.metrics = PathDBMetrics::new_with_labels(&[("instance", instance_name.to_string())]);
    }
}

impl PathProvider for PathDB {
    fn get_raw(&self, key: &[u8]) -> PathProviderResult<Option<Vec<u8>>> {
        trace!(target: "pathdb::rocksdb", "Getting key: {:?}", key);

        // Check cache first
        {
            let cache = self.cache.lock().unwrap();
            if let Some(cached_value) = cache.peek(key) {
                self.metrics.cache_hits.increment(1);
                trace!(target: "pathdb::rocksdb", "Found value in cache for key: {:?}", key);
                return Ok(cached_value.clone());
            } else {
                self.metrics.cache_misses.increment(1);
            }
        }

        let cf = self.db.cf_handle(DEFAULT_COLUMN_FAMILY_NAME).ok_or_else(|| {
            PathProviderError::Database(format!("Column Family '{}' handle not found", DEFAULT_COLUMN_FAMILY_NAME))
        })?;

        // Cache miss, read from DB
        match self.db.get_cf_opt(&cf, key, &self.read_options) {
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

        let cf = self.db.cf_handle(DEFAULT_COLUMN_FAMILY_NAME).ok_or_else(|| {
            PathProviderError::Database(format!("Column Family '{}' handle not found", DEFAULT_COLUMN_FAMILY_NAME))
        })?;

        // Then write to DB
        match self.db.put_cf_opt(&cf, key, value, &self.write_options) {
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

        let cf = self.db.cf_handle(DEFAULT_COLUMN_FAMILY_NAME).ok_or_else(|| {
            PathProviderError::Database(format!("Column Family '{}' handle not found", DEFAULT_COLUMN_FAMILY_NAME))
        })?;

        // Then delete from DB
        match self.db.delete_cf_opt(&cf, key, &self.write_options) {
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
                self.metrics.cache_hits.increment(1);
                return Ok(cached_value.is_some());
            } else {
                self.metrics.cache_misses.increment(1);
            }
        }

        let cf = self.db.cf_handle(DEFAULT_COLUMN_FAMILY_NAME).ok_or_else(|| {
            PathProviderError::Database(format!("Column Family '{}' handle not found", DEFAULT_COLUMN_FAMILY_NAME))
        })?;
            
        // Cache miss, check DB
        match self.db.get_cf_opt(&cf, key, &self.read_options) {
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

    fn clear_cache(&self) {
        self.clear_cache();
    }

    fn commit_difflayer(&self, block_number: u64, state_root: B256, difflayer: &Option<Arc<DiffLayer>>) -> Result<(), Self::Error> {
        // Get Column Family handle for default CF
        let cf = self.db.cf_handle(DEFAULT_COLUMN_FAMILY_NAME).ok_or_else(|| {
            PathProviderError::Database(format!("Column Family '{}' handle not found", DEFAULT_COLUMN_FAMILY_NAME))
        })?;

        let mut batch = WriteBatch::default();
        {
            let mut cache = self.cache.lock().unwrap();
            // Write to default CF using put_cf
            batch.put_cf(&cf, TRIE_STATE_ROOT_KEY, state_root.as_slice());
            batch.put_cf(&cf, TRIE_STATE_BLOCK_NUMBER_KEY, &block_number.to_le_bytes());
            cache.insert(TRIE_STATE_ROOT_KEY.to_vec(), Some(state_root.as_slice().to_vec()));
            cache.insert(TRIE_STATE_BLOCK_NUMBER_KEY.to_vec(), Some(block_number.to_le_bytes().to_vec()));
        
            if let Some(difflayer) = difflayer {
                for (key, node) in difflayer.diff_nodes.iter() {
                    if node.is_deleted() {
                        // Delete from default CF using delete_cf
                        batch.delete_cf(&cf, key);
                        cache.remove(key);
                    } else {
                        if let Some(blob) = &node.blob {
                            cache.insert(key.clone(), Some(blob.clone()));
                            // Write to default CF using put_cf
                            batch.put_cf(&cf, key, blob);
                        }
                    }
                }
            }
        }

        match self.db.write_opt(batch, &self.write_options) {
            Ok(()) => {
                trace!(target: "pathdb::batch", "Successfully committed batch to database");
                Ok(())
            }
            Err(e) => {
                error!(target: "pathdb::batch", "Error committing batch: block_number: {}, state_root: {:?}, error: {}", block_number, state_root, e);
                Err(PathProviderError::Database(format!("Batch commit error: {}", e)))
            }
        }
    }

    fn latest_persist_state(&self) -> Result<(u64, B256), Self::Error> {
        let block_number_bytes = self.get(TRIE_STATE_BLOCK_NUMBER_KEY)?;
        let state_root_bytes = self.get(TRIE_STATE_ROOT_KEY)?;
        if let (Some(block_number_bytes), Some(state_root_bytes)) = (block_number_bytes, state_root_bytes) {
            let block_number = u64::from_le_bytes(block_number_bytes.try_into().unwrap());
            let state_root = B256::from_slice(&state_root_bytes);
            Ok((block_number, state_root))
        } else {
            Err(PathProviderError::Database("Latest persist state not found".to_string()))
        }
    }
}


/// Ensure all required Column Families exist in the database.
/// Creates missing Column Families if they don't exist.
///
/// # Arguments
/// * `path` - Path to the RocksDB database
/// * `db_opts` - Database options
/// * `config` - Path provider configuration
///
/// # Returns
/// * `Ok(())` if all Column Families exist or were successfully created
/// * `Err(PathProviderError)` if there was an error creating Column Families
fn ensure_column_families(
    path: &str,
    db_opts: &Options,
    config: &PathProviderConfig,
) -> PathProviderResult<()> {
    // List existing Column Families in the database
    let existing_cfs = DB::list_cf(db_opts, path)
        .unwrap_or_else(|_| vec!["default".to_string()]);
    let existing_cfs_set: HashSet<String> = existing_cfs.iter().cloned().collect();

    // Find missing Column Families
    let missing_cfs: Vec<&str> = COLUMN_FAMILY_NAMES
        .iter()
        .filter(|&&cf_name| !existing_cfs_set.contains(cf_name))
        .copied()
        .collect();

    // If no missing CFs, we're done
    if missing_cfs.is_empty() {
        trace!(
            target: "pathdb::rocksdb",
            "All required Column Families already exist"
        );
        return Ok(());
    }

    trace!(
        target: "pathdb::rocksdb",
        "Found {} missing Column Families: {:?}",
        missing_cfs.len(),
        missing_cfs
    );

    // Open database with existing CFs first
    let mut existing_cf_descriptors = Vec::new();
    for cf_name in &existing_cfs {
        let mut cf_opts = Options::default();
        cf_opts.set_max_write_buffer_number(config.max_write_buffer_number);
        cf_opts.set_write_buffer_size(config.write_buffer_size);
        existing_cf_descriptors.push(ColumnFamilyDescriptor::new(cf_name, cf_opts));
    }

    let temp_db = DB::open_cf_descriptors(db_opts, path, existing_cf_descriptors)
        .map_err(|e| PathProviderError::Database(format!("Failed to open RocksDB: {}", e)))?;

    // Create missing Column Families
    for cf_name in missing_cfs {
        let mut cf_opts = Options::default();
        cf_opts.set_max_write_buffer_number(config.max_write_buffer_number);
        cf_opts.set_write_buffer_size(config.write_buffer_size);
        temp_db.create_cf(cf_name, &cf_opts).map_err(|e| {
            PathProviderError::Database(format!(
                "Failed to create Column Family '{}': {}",
                cf_name, e
            ))
        })?;
        trace!(
            target: "pathdb::rocksdb",
            "Created Column Family '{}'",
            cf_name
        );
    }
    // Drop temp_db to close it before reopening with all CFs
    drop(temp_db);

    Ok(())
}