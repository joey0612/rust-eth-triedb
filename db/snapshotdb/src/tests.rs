//! Tests for SnapshotDB

use std::collections::HashMap;
use tempfile::TempDir;
use alloy_primitives::B256;
use crate::{SnapshotDB, PathProviderConfig};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_storage_root_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path();
        let db = SnapshotDB::new(db_path.to_str().unwrap(), PathProviderConfig::default()).unwrap();

        // Test getting a non-existent storage root
        let hash_address = B256::from([0x01; 32]);
        let result = db.get_storage_root(hash_address).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_storage_root_found() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path();
        let db = SnapshotDB::new(db_path.to_str().unwrap(), PathProviderConfig::default()).unwrap();

        // Insert a storage root using batch_insert
        let hash_address = B256::from([0x01; 32]);
        let storage_root = B256::from([0x02; 32]);
        let state_root = B256::from([0x03; 32]);
        let block_number = 100u64;
        let mut storage_roots = HashMap::new();
        storage_roots.insert(hash_address, storage_root);

        db.bacth_insert_storage_root(block_number, state_root, storage_roots).unwrap();

        // Retrieve it
        let result = db.get_storage_root(hash_address).unwrap();
        assert_eq!(result, Some(storage_root));
    }

    #[test]
    fn test_bacth_insert_storage_root_empty() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path();
        let db = SnapshotDB::new(db_path.to_str().unwrap(), PathProviderConfig::default()).unwrap();

        // Test with empty HashMap
        let state_root = B256::from([0x03; 32]);
        let block_number = 100u64;
        let storage_roots = HashMap::new();
        let result = db.bacth_insert_storage_root(block_number, state_root, storage_roots);
        assert!(result.is_ok());
    }

    #[test]
    fn test_bacth_insert_storage_root_single() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path();
        let db = SnapshotDB::new(db_path.to_str().unwrap(), PathProviderConfig::default()).unwrap();

        // Insert a single storage root
        let hash_address = B256::from([0x10; 32]);
        let storage_root = B256::from([0x20; 32]);
        let state_root = B256::from([0x30; 32]);
        let block_number = 200u64;
        let mut storage_roots = HashMap::new();
        storage_roots.insert(hash_address, storage_root);

        let result = db.bacth_insert_storage_root(block_number, state_root, storage_roots);
        assert!(result.is_ok());

        // Verify it was inserted
        let retrieved = db.get_storage_root(hash_address).unwrap();
        assert_eq!(retrieved, Some(storage_root));
    }

    #[test]
    fn test_bacth_insert_storage_root_multiple() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path();
        let db = SnapshotDB::new(db_path.to_str().unwrap(), PathProviderConfig::default()).unwrap();

        // Insert multiple storage roots
        let mut storage_roots = HashMap::new();
        let mut expected_results = HashMap::new();
        let state_root = B256::from([0x40; 32]);
        let block_number = 300u64;

        for i in 0..10 {
            let mut hash_bytes = [0u8; 32];
            hash_bytes[0] = i;
            let hash_address = B256::from(hash_bytes);

            let mut root_bytes = [0u8; 32];
            root_bytes[0] = i + 100;
            let storage_root = B256::from(root_bytes);

            storage_roots.insert(hash_address, storage_root);
            expected_results.insert(hash_address, storage_root);
        }

        let result = db.bacth_insert_storage_root(block_number, state_root, storage_roots);
        assert!(result.is_ok());

        // Verify all were inserted correctly
        for (hash_address, expected_root) in expected_results {
            let retrieved = db.get_storage_root(hash_address).unwrap();
            assert_eq!(retrieved, Some(expected_root));
        }
    }

    #[test]
    fn test_bacth_insert_and_get_storage_root_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path();
        let db = SnapshotDB::new(db_path.to_str().unwrap(), PathProviderConfig::default()).unwrap();

        // Create test data
        let hash_address1 = B256::from([0xAA; 32]);
        let storage_root1 = B256::from([0xBB; 32]);
        
        let hash_address2 = B256::from([0xCC; 32]);
        let storage_root2 = B256::from([0xDD; 32]);

        let state_root = B256::from([0xEE; 32]);
        let block_number = 400u64;

        let mut storage_roots = HashMap::new();
        storage_roots.insert(hash_address1, storage_root1);
        storage_roots.insert(hash_address2, storage_root2);

        // Insert
        db.bacth_insert_storage_root(block_number, state_root, storage_roots).unwrap();

        // Retrieve and verify
        let result1 = db.get_storage_root(hash_address1).unwrap();
        assert_eq!(result1, Some(storage_root1));

        let result2 = db.get_storage_root(hash_address2).unwrap();
        assert_eq!(result2, Some(storage_root2));

        // Verify non-existent key returns None
        let non_existent = B256::from([0xFF; 32]);
        let result3 = db.get_storage_root(non_existent).unwrap();
        assert_eq!(result3, None);
    }
}

