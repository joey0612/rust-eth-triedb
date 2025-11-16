//! PathDB with Column Family support for sharding/partitioning.
//!
//! This module provides a PathDB implementation that uses RocksDB Column Families
//! to support multiple logical tables (sharding) within a single database instance.
//!
//! Column Families in RocksDB allow you to:
//! - Separate data logically into different "tables"
//! - Configure each Column Family independently
//! - Perform operations on specific Column Families
//! - Maintain isolation between different data types

use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::Mutex;

use rocksdb::{
    ColumnFamilyDescriptor, DB, Options, ReadOptions, WriteBatch, WriteOptions,
};
use schnellru::{ByLength, LruMap};
use tracing::{error, trace, warn};

use crate::traits::*;

use reth_metrics::{
    metrics::{Counter},
    Metrics,
};

/// Metrics for the `PathDBCF`.
#[derive(Metrics, Clone)]
#[metrics(scope = "rust.eth.triedb.pathdb.cf")]
pub(crate) struct PathDBCFMetrics {
    /// Counter of cache hits
    pub(crate) cache_hits: Counter,
    /// Counter of cache misses
    pub(crate) cache_misses: Counter,
}

/// PathDB with Column Family support for sharding.
///
/// This implementation allows you to use multiple Column Families (tables)
/// within a single RocksDB instance. Each Column Family can be configured
/// independently and provides logical separation of data.
pub struct PathDBCF {
    /// The underlying RocksDB instance.
    db: Arc<DB>,
    /// Set of Column Family names that exist in the database.
    column_family_names: Arc<Mutex<HashSet<String>>>,
    /// Configuration for the database.
    config: PathProviderConfig,
    /// Write options for batch operations.
    write_options: WriteOptions,
    /// Read options for read operations.
    read_options: ReadOptions,
    /// LRU cache for key-value pairs, keyed by (cf_name, key).
    cache: Arc<Mutex<LruMap<(String, Vec<u8>), Option<Vec<u8>>, ByLength>>>,
    /// Metrics for the PathDBCF.
    metrics: PathDBCFMetrics,
}

impl Debug for PathDBCF {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PathDBCF")
            .field("config", &self.config)
            .finish()
    }
}

impl PathDBCF {
    /// Create a new PathDBCF instance with specified Column Families.
    ///
    /// # Arguments
    /// * `path` - Path to the RocksDB database
    /// * `config` - Configuration for the database
    /// * `column_family_names` - List of Column Family names to create/open.
    ///   Note: "default" Column Family is always included automatically.
    ///
    /// # Example
    /// ```rust,no_run
    /// use rust_eth_triedb_pathdb::traits::*;
    /// use rust_eth_triedb_pathdb::PathDBCF;
    ///
    /// let config = PathProviderConfig::default();
    /// let cf_names = vec!["accounts".to_string(), "contracts".to_string(), "state".to_string()];
    /// let db = PathDBCF::new("/path/to/db", config, cf_names)?;
    /// ```
    pub fn new(
        path: &str,
        config: PathProviderConfig,
        mut column_family_names: Vec<String>,
    ) -> PathProviderResult<Self> {
        // Ensure "default" is in the list (RocksDB always has a default CF)
        if !column_family_names.iter().any(|name| name == "default") {
            column_family_names.push("default".to_string());
        }
        
        let mut db_opts = Options::default();
        db_opts.set_max_open_files(config.max_open_files);
        db_opts.set_write_buffer_size(config.write_buffer_size);
        db_opts.set_max_write_buffer_number(config.max_write_buffer_number);
        db_opts.set_target_file_size_base(config.target_file_size_base);
        db_opts.set_max_background_jobs(config.max_background_jobs);
        db_opts.create_if_missing(config.create_if_missing);

        // Create Column Family descriptors
        let mut cf_descriptors = Vec::new();
        for cf_name in &column_family_names {
            let mut cf_opts = Options::default();
            cf_opts.set_max_write_buffer_number(config.max_write_buffer_number);
            cf_opts.set_write_buffer_size(config.write_buffer_size);
            cf_descriptors.push(ColumnFamilyDescriptor::new(cf_name, cf_opts));
        }

        // Open database with Column Families
        let db = DB::open_cf_descriptors(&db_opts, path, cf_descriptors)
            .map_err(|e| {
                PathProviderError::Database(format!(
                    "Failed to open RocksDB with Column Families: {}",
                    e
                ))
            })?;

        // Build Column Family name set
        let cf_names_set: HashSet<String> = column_family_names.into_iter().collect();

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
            metrics: PathDBCFMetrics::new_with_labels(&[("instance", "default")]),
        })
    }

    /// Get Column Family handle by name (for internal use).
    fn get_cf_handle(&self, cf_name: &str) -> PathProviderResult<()> {
        // Verify that the Column Family exists
        let cf_names = self.column_family_names.lock().unwrap();
        if !cf_names.contains(cf_name) {
            return Err(PathProviderError::Database(format!(
                "Column Family '{}' not found. Available: {:?}",
                cf_name, cf_names
            )));
        }
        Ok(())
    }

    /// Create or open a Column Family.
    ///
    /// If the Column Family already exists, it will be opened.
    /// Otherwise, a new one will be created.
    pub fn create_column_family(&self, cf_name: &str) -> PathProviderResult<()> {
        // Check if already exists
        {
            let cf_names = self.column_family_names.lock().unwrap();
            if cf_names.contains(cf_name) {
                trace!(
                    target: "pathdb::rocksdb::cf",
                    "Column Family '{}' already exists",
                    cf_name
                );
                return Ok(());
            }
        }

        // Create new Column Family options
        let mut cf_opts = Options::default();
        cf_opts.set_max_write_buffer_number(self.config.max_write_buffer_number);
        cf_opts.set_write_buffer_size(self.config.write_buffer_size);

        // Create the Column Family
        match self.db.create_cf(cf_name, &cf_opts) {
            Ok(_) => {
                let mut cf_names = self.column_family_names.lock().unwrap();
                cf_names.insert(cf_name.to_string());
                trace!(
                    target: "pathdb::rocksdb::cf",
                    "Created Column Family '{}'",
                    cf_name
                );
                Ok(())
            }
            Err(e) => Err(PathProviderError::Database(format!(
                "Failed to create Column Family '{}': {}",
                cf_name, e
            ))),
        }
    }

    /// Get a value from a specific Column Family.
    pub fn get_raw_cf(&self, cf_name: &str, key: &[u8]) -> PathProviderResult<Option<Vec<u8>>> {
        trace!(
            target: "pathdb::rocksdb::cf",
            "Getting key from CF '{}': {:?}",
            cf_name,
            key
        );

        // Check cache first
        let cache_key = (cf_name.to_string(), key.to_vec());
        {
            let cache = self.cache.lock().unwrap();
            if let Some(cached_value) = cache.peek(&cache_key) {
                self.metrics.cache_hits.increment(1);
                trace!(
                    target: "pathdb::rocksdb::cf",
                    "Found value in cache for CF '{}', key: {:?}",
                    cf_name,
                    key
                );
                return Ok(cached_value.clone());
            } else {
                self.metrics.cache_misses.increment(1);
            }
        }

        // Verify Column Family exists
        self.get_cf_handle(cf_name)?;

        // Get Column Family handle (lifetime-bound, so we use it immediately)
        let cf = self.db.cf_handle(cf_name).ok_or_else(|| {
            PathProviderError::Database(format!("Column Family '{}' handle not found", cf_name))
        })?;

        // Cache miss, read from DB
        match self.db.get_cf_opt(&cf, key, &self.read_options) {
            Ok(Some(value)) => {
                trace!(
                    target: "pathdb::rocksdb::cf",
                    "Found value in DB for CF '{}', key: {:?}",
                    cf_name,
                    key
                );
                // Cache the value
                self.cache
                    .lock()
                    .unwrap()
                    .insert(cache_key, Some(value.to_vec()));
                Ok(Some(value))
            }
            Ok(None) => {
                trace!(
                    target: "pathdb::rocksdb::cf",
                    "Key not found in DB for CF '{}', key: {:?}",
                    cf_name,
                    key
                );
                Ok(None)
            }
            Err(e) => {
                error!(
                    target: "pathdb::rocksdb::cf",
                    "Error getting key from CF '{}' {:?}: {}",
                    cf_name,
                    key,
                    e
                );
                Err(PathProviderError::Database(format!(
                    "RocksDB get_cf error: {}",
                    e
                )))
            }
        }
    }

    /// Put a key-value pair into a specific Column Family.
    pub fn put_raw_cf(
        &self,
        cf_name: &str,
        key: &[u8],
        value: &[u8],
    ) -> PathProviderResult<()> {
        trace!(
            target: "pathdb::rocksdb::cf",
            "Putting key into CF '{}': {:?}, value_len: {}",
            cf_name,
            key,
            value.len()
        );

        // Verify Column Family exists
        self.get_cf_handle(cf_name)?;

        // Get Column Family handle
        let cf = self.db.cf_handle(cf_name).ok_or_else(|| {
            PathProviderError::Database(format!("Column Family '{}' handle not found", cf_name))
        })?;

        // Update cache first
        let cache_key = (cf_name.to_string(), key.to_vec());
        self.cache
            .lock()
            .unwrap()
            .insert(cache_key.clone(), Some(value.to_vec()));

        // Then write to DB
        match self.db.put_cf_opt(&cf, key, value, &self.write_options) {
            Ok(()) => {
                trace!(
                    target: "pathdb::rocksdb::cf",
                    "Successfully put key into CF '{}': {:?}",
                    cf_name,
                    key
                );
                Ok(())
            }
            Err(e) => {
                error!(
                    target: "pathdb::rocksdb::cf",
                    "Error putting key into CF '{}' {:?}: {}",
                    cf_name,
                    key,
                    e
                );
                // Remove from cache on error
                self.cache.lock().unwrap().remove(&cache_key);
                Err(PathProviderError::Database(format!(
                    "RocksDB put_cf error: {}",
                    e
                )))
            }
        }
    }

    /// Delete a key from a specific Column Family.
    pub fn delete_raw_cf(&self, cf_name: &str, key: &[u8]) -> PathProviderResult<()> {
        trace!(
            target: "pathdb::rocksdb::cf",
            "Deleting key from CF '{}': {:?}",
            cf_name,
            key
        );

        // Verify Column Family exists
        self.get_cf_handle(cf_name)?;

        // Get Column Family handle
        let cf = self.db.cf_handle(cf_name).ok_or_else(|| {
            PathProviderError::Database(format!("Column Family '{}' handle not found", cf_name))
        })?;

        // Remove from cache first
        let cache_key = (cf_name.to_string(), key.to_vec());
        self.cache.lock().unwrap().remove(&cache_key);

        // Then delete from DB
        match self.db.delete_cf_opt(&cf, key, &self.write_options) {
            Ok(()) => {
                trace!(
                    target: "pathdb::rocksdb::cf",
                    "Successfully deleted key from CF '{}': {:?}",
                    cf_name,
                    key
                );
                Ok(())
            }
            Err(e) => {
                error!(
                    target: "pathdb::rocksdb::cf",
                    "Error deleting key from CF '{}' {:?}: {}",
                    cf_name,
                    key,
                    e
                );
                Err(PathProviderError::Database(format!(
                    "RocksDB delete_cf error: {}",
                    e
                )))
            }
        }
    }

    /// Check if a key exists in a specific Column Family.
    pub fn exists_raw_cf(&self, cf_name: &str, key: &[u8]) -> PathProviderResult<bool> {
        trace!(
            target: "pathdb::rocksdb::cf",
            "Checking existence of key in CF '{}': {:?}",
            cf_name,
            key
        );

        // Check cache first
        let cache_key = (cf_name.to_string(), key.to_vec());
        {
            let cache = self.cache.lock().unwrap();
            if let Some(cached_value) = cache.peek(&cache_key) {
                trace!(
                    target: "pathdb::rocksdb::cf",
                    "Key exists in cache for CF '{}': {:?}",
                    cf_name,
                    key
                );
                self.metrics.cache_hits.increment(1);
                return Ok(cached_value.is_some());
            } else {
                self.metrics.cache_misses.increment(1);
            }
        }

        // Verify Column Family exists
        self.get_cf_handle(cf_name)?;

        // Get Column Family handle
        let cf = self.db.cf_handle(cf_name).ok_or_else(|| {
            PathProviderError::Database(format!("Column Family '{}' handle not found", cf_name))
        })?;

        // Cache miss, check DB
        match self.db.get_cf_opt(&cf, key, &self.read_options) {
            Ok(Some(_)) => {
                trace!(
                    target: "pathdb::rocksdb::cf",
                    "Key exists in DB for CF '{}': {:?}",
                    cf_name,
                    key
                );
                // Cache the existence
                self.cache
                    .lock()
                    .unwrap()
                    .insert(cache_key, Some(vec![]));
                Ok(true)
            }
            Ok(None) => {
                trace!(
                    target: "pathdb::rocksdb::cf",
                    "Key does not exist in DB for CF '{}': {:?}",
                    cf_name,
                    key
                );
                Ok(false)
            }
            Err(e) => {
                error!(
                    target: "pathdb::rocksdb::cf",
                    "Error checking existence of key in CF '{}' {:?}: {}",
                    cf_name,
                    key,
                    e
                );
                Err(PathProviderError::Database(format!(
                    "RocksDB exists_cf error: {}",
                    e
                )))
            }
        }
    }

    /// Put multiple key-value pairs into a specific Column Family.
    pub fn put_multi_cf(
        &self,
        cf_name: &str,
        kvs: &[(Vec<u8>, Vec<u8>)],
    ) -> PathProviderResult<()> {
        trace!(
            target: "pathdb::rocksdb::cf",
            "Putting {} key-value pairs into CF '{}'",
            kvs.len(),
            cf_name
        );

        // Verify Column Family exists
        self.get_cf_handle(cf_name)?;

        // Get Column Family handle
        let cf = self.db.cf_handle(cf_name).ok_or_else(|| {
            PathProviderError::Database(format!("Column Family '{}' handle not found", cf_name))
        })?;

        // Update cache first
        {
            let mut cache = self.cache.lock().unwrap();
            for (key, value) in kvs {
                let cache_key = (cf_name.to_string(), key.clone());
                cache.insert(cache_key, Some(value.clone()));
            }
        }

        // Then write to DB
        let mut batch = WriteBatch::default();

        for (key, value) in kvs {
            batch.put_cf(&cf, key, value);
        }

        match self.db.write_opt(batch, &self.write_options) {
            Ok(()) => {
                trace!(
                    target: "pathdb::rocksdb::cf",
                    "Successfully put {} key-value pairs into CF '{}'",
                    kvs.len(),
                    cf_name
                );
                Ok(())
            }
            Err(e) => {
                error!(
                    target: "pathdb::rocksdb::cf",
                    "Error putting {} key-value pairs into CF '{}': {}",
                    kvs.len(),
                    cf_name,
                    e
                );
                // Remove from cache on error
                let mut cache = self.cache.lock().unwrap();
                for (key, _) in kvs {
                    let cache_key = (cf_name.to_string(), key.clone());
                    cache.remove(&cache_key);
                }
                Err(PathProviderError::Database(format!(
                    "RocksDB put_multi_cf error: {}",
                    e
                )))
            }
        }
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
        warn!(target: "pathdb::rocksdb::cf", "Clearing LRU cache");
        self.cache.lock().unwrap().clear();
    }

    /// Get cache statistics.
    pub fn cache_stats(&self) -> (usize, u32) {
        let cache = self.cache.lock().unwrap();
        (cache.len(), self.config.cache_size)
    }

    /// List all Column Family names.
    pub fn list_column_families(&self) -> Vec<String> {
        let cf_names = self.column_family_names.lock().unwrap();
        cf_names.iter().cloned().collect()
    }
}

// Implement PathProvider for backward compatibility (uses default CF)
impl PathProvider for PathDBCF {
    fn get_raw(&self, key: &[u8]) -> PathProviderResult<Option<Vec<u8>>> {
        self.get_raw_cf("default", key)
    }

    fn put_raw(&self, key: &[u8], value: &[u8]) -> PathProviderResult<()> {
        self.put_raw_cf("default", key, value)
    }

    fn delete_raw(&self, key: &[u8]) -> PathProviderResult<()> {
        self.delete_raw_cf("default", key)
    }

    fn exists_raw(&self, key: &[u8]) -> PathProviderResult<bool> {
        self.exists_raw_cf("default", key)
    }

    fn get_multi(&self, keys: &[Vec<u8>]) -> PathProviderResult<HashMap<Vec<u8>, Vec<u8>>> {
        let mut result = HashMap::new();
        for key in keys {
            if let Some(value) = self.get_raw(key)? {
                result.insert(key.clone(), value);
            }
        }
        Ok(result)
    }

    fn put_multi(&self, kvs: &[(Vec<u8>, Vec<u8>)]) -> PathProviderResult<()> {
        self.put_multi_cf("default", kvs)
    }

    fn delete_multi(&self, keys: &[Vec<u8>]) -> PathProviderResult<()> {
        for key in keys {
            self.delete_raw(key)?;
        }
        Ok(())
    }
}

impl PathProviderManager for PathDBCF {
    fn open(path: &str) -> PathProviderResult<Arc<dyn PathProvider>> {
        trace!(
            target: "pathdb::rocksdb::cf",
            "Opening database with Column Families at path: {}",
            path
        );

        let config = PathProviderConfig::default();
        // Try to list existing Column Families
        let existing_cfs = DB::list_cf(&Options::default(), path)
            .unwrap_or_else(|_| vec!["default".to_string()]);

        let db = Self::new(path, config, existing_cfs)?;
        Ok(Arc::new(db))
    }

    fn close(&self) -> PathProviderResult<()> {
        trace!(target: "pathdb::rocksdb::cf", "Closing database");
        // RocksDB automatically closes when the last Arc is dropped
        Ok(())
    }

    fn flush(&self) -> PathProviderResult<()> {
        trace!(target: "pathdb::rocksdb::cf", "Flushing database");

        match self.db.flush() {
            Ok(()) => {
                trace!(
                    target: "pathdb::rocksdb::cf",
                    "Successfully flushed database"
                );
                Ok(())
            }
            Err(e) => {
                error!(
                    target: "pathdb::rocksdb::cf",
                    "Error flushing database: {}",
                    e
                );
                Err(PathProviderError::Database(format!("Flush error: {}", e)))
            }
        }
    }

    fn compact(&self) -> PathProviderResult<()> {
        trace!(target: "pathdb::rocksdb::cf", "Compacting database");
        // Simplified compact implementation
        Ok(())
    }
}
