//! Database traits for trie operations.

use std::sync::Arc;
use alloy_primitives::B256;
use auto_impl::auto_impl;
use crate::difflayer::DiffLayer;

/// Simple database trait for trie operations
#[auto_impl(Box, Arc, Clone, Send + Sync + Debug + Unpin + 'static)]
pub trait TrieDatabase {
    /// Associated error type for database operations
    type Error;

    /// Get a node from the database by its hash
    fn get(&self, path: &[u8]) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Insert a node into the database with the given hash
    fn insert(&self, path: &[u8], data: Vec<u8>) -> Result<(), Self::Error>;

    /// Check if a node exists in the database
    fn contains(&self, path: &[u8]) -> Result<bool, Self::Error>;

    /// Remove a node from the database and return its data if found
    fn remove(&self, path: &[u8]);

    /// Commit a diff layer to the database
    fn commit_difflayer(&self, block_number: u64, state_root: B256, difflayer: &Option<Arc<DiffLayer>>) -> Result<(), Self::Error>;

    /// Get the latest persisted state from the database
    fn latest_persist_state(&self) -> Result<(u64, B256), Self::Error>;

    /// Clear the cache
    fn clear_cache(&self);
}
