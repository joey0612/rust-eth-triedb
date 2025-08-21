//! Trie database library for Ethereum state management.

// Note: Global allocator is configured by the parent project (reth-bsc)
// This crate supports jemalloc feature for dependency resolution but doesn't define global allocator

pub mod traits;
pub mod triedb;

#[cfg(test)]
mod triedb_test;

// Re-export main types
pub use traits::TrieDBTrait;
pub use triedb::TrieDB;
pub use triedb::TrieDBError;
pub use triedb::EMPTY_ROOT_HASH;
