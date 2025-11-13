//! SnapshotDB implementation for RocksDB integration.
//!
//! This crate provides a thread-safe abstraction over RocksDB with support for:
//! - Basic key-value operations (get, put, delete)
//! - Batch operations
//! - Iterators
//! - Snapshots
//! - Thread safety

pub mod snapshotdb;
pub mod traits;

#[cfg(test)]
pub mod tests;

pub use snapshotdb::SnapshotDB;
pub use traits::*;

