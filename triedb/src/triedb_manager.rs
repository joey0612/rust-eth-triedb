//! TrieDB Manager for managing global TrieDB instances
//!
//! This module provides a singleton manager for TrieDB instances,
//! allowing global access to a shared TrieDB across the application.

use std::sync::{OnceLock};
use rust_eth_triedb_pathdb::{PathDB, PathProviderConfig};
// use rust_eth_triedb_snapshotdb::{SnapshotDB, PathProviderConfig as SnapshotPathProviderConfig};
use super::TrieDB;
use rust_eth_triedb_state_trie::node::init_empty_root_node;

/// Global TrieDB Manager
/// 
/// A singleton manager that maintains a single TrieDB instance
/// accessible throughout the application lifecycle.
pub struct TrieDBManager {
    triedb: TrieDB<PathDB>,
}

// Global singleton instance - automatically initialized on first access
static MANAGER_INSTANCE: OnceLock<TrieDBManager> = OnceLock::new();

/// Initialize the global manager instance.
/// 
/// This function must be called once at application startup before any calls to `get_global_triedb()`.
/// Returns an error if the manager has already been initialized.
pub fn init_global_manager() {
    init_empty_root_node();
    MANAGER_INSTANCE.get_or_init(|| TrieDBManager::new());
}

// Get the initialized manager instance
fn get_manager() -> &'static TrieDBManager {
    MANAGER_INSTANCE.get()
        .expect("Global TrieDB manager not initialized. Call init_global_manager() first.")
}

/// Get the global TrieDB instance.
/// 
/// This function returns a clone of the global TrieDB instance.
/// The global manager must be initialized first by calling `init_global_manager()`.
/// 
/// # Panics
/// 
/// This function will panic if `init_global_manager()` has not been called first.
pub fn get_global_triedb() -> TrieDB<PathDB> {
    get_manager().get_triedb()
}

impl TrieDBManager {
    /// Create a new TrieDBManager with the given database
    fn new() -> Self {
        let current_dir = std::env::current_dir().unwrap();
        let path_db_dir = current_dir.join("data").join("rust_eth_triedb").to_string_lossy().to_string();
        let pathdb = PathDB::new(&path_db_dir, PathProviderConfig::default()).expect("Failed to create PathDB");

        // let snap_db_dir = current_dir.join("data").join("rust_eth_triedb_snapshots").to_string_lossy().to_string();
        // let snapdb = SnapshotDB::new(&snap_db_dir, SnapshotPathProviderConfig::default()).expect("Failed to create SnapshotDB");
        
        let triedb = TrieDB::new(pathdb);
        Self {
            triedb,
        }
    }

    /// Get a reference to the managed TrieDB instance
    pub fn get_triedb(&self) -> TrieDB<PathDB> {
        self.triedb.clone()
    }
}

