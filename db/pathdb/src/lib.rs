//! PathDB implementation for RocksDB integration.
//!
//! This crate provides a thread-safe abstraction over RocksDB with support for:
//! - Basic key-value operations (get, put, delete)
//! - Batch operations
//! - Iterators
//! - Snapshots
//! - Thread safety
//! - Column Family support for sharding/partitioning

pub mod pathdb;
pub mod traits;

#[cfg(test)]
pub mod tests;

pub use pathdb::PathDB;
pub use traits::*;
