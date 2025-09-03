//! TrieDB Manager for managing global TrieDB instances
//!
//! This module provides a singleton manager for TrieDB instances,
//! allowing global access to a shared TrieDB across the application.

use std::sync::{OnceLock};
use rust_eth_triedb_pathdb::{PathDB, PathProviderConfig};
use super::TrieDB;

/// Global TrieDB Manager
/// 
/// A singleton manager that maintains a single TrieDB instance
/// accessible throughout the application lifecycle.
pub struct TrieDBManager {
    triedb: TrieDB<PathDB>,
}

// Global singleton instance - automatically initialized on first access
static MANAGER_INSTANCE: OnceLock<TrieDBManager> = OnceLock::new();

// Auto-initialization function
fn get_or_init_manager() -> &'static TrieDBManager {
    MANAGER_INSTANCE.get_or_init(|| TrieDBManager::new())
}

pub fn get_global_triedb() -> TrieDB<PathDB> {
    get_or_init_manager().get_triedb()
}

impl TrieDBManager {
    /// Create a new TrieDBManager with the given database
    fn new() -> Self {
        let current_dir = std::env::current_dir().unwrap();
        let db_path = current_dir.join("data").join("rust_eth_triedb").to_string_lossy().to_string();

        // Create path database and TrieDB instance
        let config = PathProviderConfig::default();
        let pathdb = PathDB::new(&db_path, config).expect("Failed to create PathDB");
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

