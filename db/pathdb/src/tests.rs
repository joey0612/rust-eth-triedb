//! Tests for PathDB implementation.

use tempfile::TempDir;
use crate::{PathDB, PathProviderConfig, PathProvider};
use rust_eth_triedb_common::TrieDatabase;

#[test]
fn test_basic_operations() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path();
    let db = PathDB::new(db_path.to_str().unwrap(), PathProviderConfig::default()).unwrap();

    // Test put and get
    let key = b"test_key";
    let value = b"test_value";
    db.put_raw(key, value).unwrap();
    
    let retrieved = db.get(key).unwrap();
    assert_eq!(retrieved, Some(value.to_vec()));

    // Test exists
    assert!(db.exists_raw(key).unwrap());
    assert!(!db.exists_raw(b"non_existent_key").unwrap());

    // Test delete
    db.delete_raw(key).unwrap();
    assert_eq!(db.get_raw(key).unwrap(), None);
    assert!(!db.exists_raw(key).unwrap());
}

#[test]
fn test_multi_operations() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path();
    let db = PathDB::new(db_path.to_str().unwrap(), PathProviderConfig::default()).unwrap();

    // Test put_multi and get_multi
    let kvs = vec![
        (b"key1".to_vec(), b"value1".to_vec()),
        (b"key2".to_vec(), b"value2".to_vec()),
        (b"key3".to_vec(), b"value3".to_vec()),
    ];

    db.put_multi(&kvs).unwrap();

    let keys: Vec<Vec<u8>> = kvs.iter().map(|(k, _)| k.clone()).collect();
    let retrieved = db.get_multi(&keys).unwrap();

    assert_eq!(retrieved.len(), 3);
    assert_eq!(retrieved.get(&b"key1".to_vec()).unwrap(), &b"value1".to_vec());
    assert_eq!(retrieved.get(&b"key2".to_vec()).unwrap(), &b"value2".to_vec());
    assert_eq!(retrieved.get(&b"key3".to_vec()).unwrap(), &b"value3".to_vec());

    // Test delete_multi
    db.delete_multi(&keys).unwrap();
    
    for key in &keys {
        assert_eq!(db.get(key).unwrap(), None);
    }
}

#[test]
fn test_cache_operations() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path();
    let db = PathDB::new(db_path.to_str().unwrap(), PathProviderConfig::default()).unwrap();

    // Test cache operations
    let key = b"cache_test_key";
    let value = b"cache_test_value";
    
    db.put_raw(key, value).unwrap();
    
    // Get cache stats
    let (cache_len, cache_capacity) = db.cache_stats();
    assert!(cache_len > 0);
    assert_eq!(cache_capacity, PathProviderConfig::default().cache_size);
    
    // Clear cache
    db.clear_cache();
    let (cache_len_after_clear, _) = db.cache_stats();
    assert_eq!(cache_len_after_clear, 0);
}

#[test]
fn test_configuration() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path();
    
    let mut config = PathProviderConfig::default();
    config.cache_size = 1000;
    config.fill_cache = false;
    config.readahead_size = 256 * 1024; // 256KB
    config.async_io = false;
    config.verify_checksums = true;
    
    let db = PathDB::new(db_path.to_str().unwrap(), config.clone()).unwrap();
    
    let retrieved_config = db.config();
    assert_eq!(retrieved_config.cache_size, 1000);
    assert_eq!(retrieved_config.fill_cache, false);
    assert_eq!(retrieved_config.readahead_size, 256 * 1024);
    assert_eq!(retrieved_config.async_io, false);
    assert_eq!(retrieved_config.verify_checksums, true);
}

#[test]
fn test_error_handling() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path();
    let db = PathDB::new(db_path.to_str().unwrap(), PathProviderConfig::default()).unwrap();

    // Test get non-existent key
    let result = db.get(b"non_existent");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), None);

    // Test exists non-existent key
    let result = db.exists_raw(b"non_existent");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), false);
}

#[test]
fn test_concurrent_access() {
    use std::sync::Arc;
    use std::thread;

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path();
    let db = Arc::new(PathDB::new(db_path.to_str().unwrap(), PathProviderConfig::default()).unwrap());

    let db_clone = db.clone();
    let handle = thread::spawn(move || {
        for i in 0..100 {
            let key = format!("thread_key_{}", i).into_bytes();
            let value = format!("thread_value_{}", i).into_bytes();
            db_clone.put_raw(&key, &value).unwrap();
        }
    });

    handle.join().unwrap();

    // Verify all values were written
    for i in 0..100 {
        let key = format!("thread_key_{}", i).into_bytes();
        let expected_value = format!("thread_value_{}", i).into_bytes();
        let retrieved = db.get(&key).unwrap();
        assert_eq!(retrieved, Some(expected_value));
    }
}