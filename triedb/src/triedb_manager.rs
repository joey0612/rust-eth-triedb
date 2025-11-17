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
/// The `path` parameter specifies the database path for the TrieDB instance.
/// 
/// # Behavior
/// - On the first call, initializes the manager with the provided path.
/// - On subsequent calls, the path parameter is ignored and the existing instance is returned.
/// 
/// # Arguments
/// * `path` - Path to the database directory
/// 
/// # ⚠️ Important: Single Initialization Pattern
/// 
/// **Only the first call to `init_global_manager` is effective.** The path provided in the
/// first call determines the database path for the entire lifetime of the application.
/// 
/// All subsequent calls to `init_global_manager` will **ignore the path parameter** and
/// return the same database instance that was created with the first path.
/// 
/// ## Recommended Usage Pattern
/// 
/// ```ignore
/// // ✅ Correct: Call once at application startup
/// fn main() {
///     // Initialize once at startup with the desired database path
///     init_global_manager("/path/to/database");
///     
///     // Later in your code, just get the instance
///     let triedb = get_global_triedb();
///     // ... use triedb
/// }
/// 
/// // ❌ Incorrect: Multiple initialization attempts
/// fn main() {
///     init_global_manager("/path/to/db1");  // This path is used
///     init_global_manager("/path/to/db2");  // This path is IGNORED!
///     // The database will still use "/path/to/db1"
/// }
/// ```
/// 
/// **Best Practice**: Call `init_global_manager` exactly once during application
/// initialization (e.g., in `main()` or startup code), then use `get_global_triedb()`
/// throughout the rest of your application to access the singleton instance.
pub fn init_global_manager(path: &str) {
    init_empty_root_node();
    MANAGER_INSTANCE.get_or_init(|| {
        let path_str = path.to_string();
        TrieDBManager::new(&path_str)
    });
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
    /// Create a new TrieDBManager with the given database path
    /// 
    /// # Arguments
    /// * `path` - Path to the database directory
    fn new(path: &str) -> Self {
        let pathdb = PathDB::new(path, PathProviderConfig::default())
            .expect("Failed to create PathDB");

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

