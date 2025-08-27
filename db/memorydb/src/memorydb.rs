//! In-memory database implementation for trie nodes.

use alloy_primitives::B256;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

use rust_eth_triedb_common::{TrieDatabase, TrieDatabaseBatch};

/// Error type for memory database operations.
#[derive(Debug, Error)]
pub enum MemoryDBError {
    /// Node not found in database
    #[error("Node not found: {0}")]
    NodeNotFound(B256),
}

/// In-memory batch implementation for MemoryDB
#[derive(Debug)]
pub struct MemoryDBBatch {
    /// Pending operations to be applied
    operations: Vec<(Vec<u8>, Option<Vec<u8>>)>, // (key, value) where None means delete
}

impl MemoryDBBatch {
    /// Create a new empty batch
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
        }
    }
}

impl TrieDatabaseBatch for MemoryDBBatch {
    type Error = MemoryDBError;

    fn insert(&mut self, key: &[u8], value: Vec<u8>) -> Result<(), Self::Error> {
        // Check if key already exists and replace it
        if let Some(pos) = self.operations.iter().position(|(k, _)| k == key) {
            self.operations[pos] = (key.to_vec(), Some(value));
        } else {
            self.operations.push((key.to_vec(), Some(value)));
        }
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<(), Self::Error> {
        // Check if key already exists and replace it
        if let Some(pos) = self.operations.iter().position(|(k, _)| k == key) {
            self.operations[pos] = (key.to_vec(), None);
        } else {
            self.operations.push((key.to_vec(), None));
        }
        Ok(())
    }

    fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    fn len(&self) -> usize {
        self.operations.len()
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.operations.clear();
        Ok(())
    }
}

/// In-memory database implementation for trie nodes.
#[derive(Debug, Clone)]
pub struct MemoryDB {
    /// Storage for trie nodes.
    nodes: Arc<RwLock<HashMap<Vec<u8>, Vec<u8>>>>,
}

impl MemoryDB {
    /// Creates a new empty memory database.
    pub fn new() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Inserts a node into the database.
    pub fn insert(&self, hash: B256, data: Vec<u8>) {
        self.nodes.write().insert(hash.as_slice().to_vec(), data);
    }

    /// Gets a node from the database.
    pub fn get(&self, hash: &B256) -> Option<Vec<u8>> {
        self.nodes.read().get(hash.as_slice()).cloned()
    }

    /// Removes a node from the database.
    pub fn remove(&self, hash: &B256) -> Option<Vec<u8>> {
        self.nodes.write().remove(hash.as_slice())
    }

    /// Checks if a node exists in the database.
    pub fn contains(&self, hash: &B256) -> bool {
        self.nodes.read().contains_key(hash.as_slice())
    }

    /// Clears all nodes from the database.
    pub fn clear(&self) {
        self.nodes.write().clear();
    }

    /// Returns the number of nodes in the database.
    pub fn len(&self) -> usize {
        self.nodes.read().len()
    }

    /// Checks if the database is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.read().is_empty()
    }

    /// Apply a batch of operations to the database
    pub fn apply_batch(&self, batch: MemoryDBBatch) -> Result<(), MemoryDBError> {
        let mut nodes = self.nodes.write();
        
        for (key, value) in batch.operations {
            match value {
                Some(val) => {
                    // Insert or update
                    nodes.insert(key, val);
                }
                None => {
                    // Delete
                    nodes.remove(&key);
                }
            }
        }
        
        Ok(())
    }
}

impl Default for MemoryDB {
    fn default() -> Self {
        Self::new()
    }
}

impl TrieDatabase for MemoryDB {
    type Error = MemoryDBError;
    type Batch = MemoryDBBatch;

    fn get(&self, path: &[u8]) -> Result<Option<Vec<u8>>, Self::Error> {
        Ok(self.nodes.read().get(path).cloned())
    }

    fn insert(&self, path: &[u8], data: Vec<u8>) -> Result<(), Self::Error> {
        self.nodes.write().insert(path.to_vec(), data);
        Ok(())
    }

    fn contains(&self, path: &[u8]) -> Result<bool, Self::Error> {
        Ok(self.nodes.read().contains_key(path))
    }

    fn remove(&self, path: &[u8]) {
        let _ = self.nodes.write().remove(path);
    }

    fn create_batch(&self) -> Result<Self::Batch, Self::Error> {
        Ok(MemoryDBBatch::new())
    }

    fn batch_commit(&self, batch: Self::Batch) -> Result<(), Self::Error> {
        self.apply_batch(batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::keccak256;

    #[test]
    fn test_memory_db_basic_operations() {
        let db = MemoryDB::new();
        assert!(db.is_empty());

        let data = b"test data".to_vec();
        let hash = keccak256(&data);

        // Test insert and get
        db.insert(hash, data.clone());
        assert!(!db.is_empty());
        assert_eq!(db.len(), 1);
        assert!(db.contains(&hash));
        assert_eq!(db.get(&hash), Some(data.clone()));

        // Test remove
        let removed = db.remove(&hash);
        assert_eq!(removed, Some(data));
        assert!(db.is_empty());
        assert!(!db.contains(&hash));
    }

    #[test]
    fn test_memory_db_trie_interface() {
        let db = MemoryDB::new();
        let data = b"test data".to_vec();
        let hash = keccak256(&data);

        // Test TrieDatabase trait implementation
        assert!(TrieDatabase::get(&db, hash.as_slice()).unwrap().is_none());
        TrieDatabase::insert(&db, hash.as_slice(), data.clone()).unwrap();
        assert_eq!(TrieDatabase::get(&db, hash.as_slice()).unwrap(), Some(data.clone()));
        assert!(TrieDatabase::contains(&db, hash.as_slice()).unwrap());
        // Test remove - it returns () not Option<Vec<u8>>
        TrieDatabase::remove(&db, hash.as_slice());
        assert!(TrieDatabase::get(&db, hash.as_slice()).unwrap().is_none());
    }

    #[test]
    fn test_memory_db_batch_operations() {
        let db = MemoryDB::new();
        
        // Test basic batch operations
        let mut batch = db.create_batch().unwrap();
        
        // Test batch insert
        batch.insert(b"batch_key1", b"batch_value1".to_vec()).unwrap();
        batch.insert(b"batch_key2", b"batch_value2".to_vec()).unwrap();
        batch.insert(b"batch_key3", b"batch_value3".to_vec()).unwrap();
        
        // Test batch properties
        assert!(!batch.is_empty());
        assert_eq!(batch.len(), 3);
        
        // Test batch delete
        batch.delete(b"batch_key2").unwrap();
        assert_eq!(batch.len(), 3); // Should still be 3, but batch_key2 is now a delete operation
        
        // Commit batch
        db.batch_commit(batch).unwrap();
        
        // Verify committed values
        assert_eq!(TrieDatabase::get(&db, b"batch_key1").unwrap(), Some(b"batch_value1".to_vec()));
        assert_eq!(TrieDatabase::get(&db, b"batch_key2").unwrap(), None); // Should be deleted
        assert_eq!(TrieDatabase::get(&db, b"batch_key3").unwrap(), Some(b"batch_value3".to_vec()));
    }

    #[test]
    fn test_memory_db_batch_clear() {
        let db = MemoryDB::new();
        let mut batch = db.create_batch().unwrap();
        
        // Add some operations
        batch.insert(b"key1", b"value1".to_vec()).unwrap();
        batch.insert(b"key2", b"value2".to_vec()).unwrap();
        batch.delete(b"key3").unwrap();
        
        assert_eq!(batch.len(), 3);
        
        // Clear batch
        batch.clear().unwrap();
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
        
        // Commit empty batch should succeed
        db.batch_commit(batch).unwrap();
    }

    #[test]
    fn test_memory_db_batch_atomicity() {
        let db = MemoryDB::new();

        // Test that batch operations are atomic
        let mut batch = db.create_batch().unwrap();
        
        // Add multiple operations
        batch.insert(b"atomic_key1", b"atomic_value1".to_vec()).unwrap();
        batch.insert(b"atomic_key2", b"atomic_value2".to_vec()).unwrap();
        batch.delete(b"atomic_key3").unwrap();
        
        // Before commit, values should not exist
        assert_eq!(TrieDatabase::get(&db, b"atomic_key1").unwrap(), None);
        assert_eq!(TrieDatabase::get(&db, b"atomic_key2").unwrap(), None);
        
        // Commit batch
        db.batch_commit(batch).unwrap();
        
        // After commit, all operations should be visible
        assert_eq!(TrieDatabase::get(&db, b"atomic_key1").unwrap(), Some(b"atomic_value1".to_vec()));
        assert_eq!(TrieDatabase::get(&db, b"atomic_key2").unwrap(), Some(b"atomic_value2".to_vec()));
        assert_eq!(TrieDatabase::get(&db, b"atomic_key3").unwrap(), None);
    }

    #[test]
    fn test_memory_db_batch_mixed_operations() {
        let db = MemoryDB::new();

        // Insert some initial values
        TrieDatabase::insert(&db, b"initial_key", b"initial_value".to_vec()).unwrap();
        
        let mut batch = db.create_batch().unwrap();
        
        // Mix insert, delete, and update operations
        batch.insert(b"new_key", b"new_value".to_vec()).unwrap();
        batch.delete(b"initial_key").unwrap();
        batch.insert(b"updated_key", b"updated_value".to_vec()).unwrap();
        
        // Commit batch
        db.batch_commit(batch).unwrap();
        
        // Verify final state
        assert_eq!(TrieDatabase::get(&db, b"new_key").unwrap(), Some(b"new_value".to_vec()));
        assert_eq!(TrieDatabase::get(&db, b"initial_key").unwrap(), None); // Should be deleted
        assert_eq!(TrieDatabase::get(&db, b"updated_key").unwrap(), Some(b"updated_value".to_vec()));
    }

    #[test]
    fn test_memory_db_batch_performance() {
        let db = MemoryDB::new();

        // Test batch performance with many operations
        let mut batch = db.create_batch().unwrap();
        
        // Add 1000 operations to batch
        for i in 0..1000 {
            let key = format!("perf_key_{}", i).into_bytes();
            let value = format!("perf_value_{}", i).into_bytes();
            batch.insert(&key, value).unwrap();
        }
        
        assert_eq!(batch.len(), 1000);
        
        // Commit batch
        db.batch_commit(batch).unwrap();
        
        // Verify all values were committed
        for i in 0..1000 {
            let key = format!("perf_key_{}", i).into_bytes();
            let expected_value = format!("perf_value_{}", i).into_bytes();
            let retrieved = TrieDatabase::get(&db, &key).unwrap();
            assert_eq!(retrieved, Some(expected_value));
        }
    }
}
