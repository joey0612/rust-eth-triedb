//! Simple test for TrieDB functionality
//!
//! This test demonstrates basic TrieDB operations:
//! 1. Initialize global manager
//! 2. Create TrieDB instance with PathDB
//! 3. Update an account
//! 4. Commit changes

use std::collections::HashMap;
use alloy_primitives::{B256, U256, keccak256};
use rust_eth_triedb_state_trie::account::StateAccount;
use rust_eth_triedb_pathdb::{PathDB, PathProviderConfig};
use rust_eth_triedb_state_trie::node::init_empty_root_node;
use tempfile::TempDir;
use super::TrieDB;

#[test]
fn test_multiple_accounts_update() {
    // Initialize global manager
    init_empty_root_node();

    // Create temporary directory for database
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let db_path = temp_dir.path().to_str().unwrap();
    
    // Create path database and TrieDB instance
    let config = PathProviderConfig::default();
    let db = PathDB::new(db_path, config).expect("Failed to create PathDB");
    let mut triedb = TrieDB::new(db);

    let total_operations = 10000;

    let mut states = HashMap::new();
    let states_rebuild = std::collections::HashSet::new();
    let storage_states = HashMap::new();

    for i in 0..total_operations {
        let hashed_address = keccak256((i as u64).to_le_bytes());
        let account = StateAccount::default()
            .with_nonce(i as u64)
            .with_balance(U256::from(i as u64));

        states.insert(hashed_address, Some(account));
    }
    // Update and commit
    let (root_hash, merged_node_set) = triedb.update_and_commit(
        B256::ZERO,
        None,
        states,
        states_rebuild,
        storage_states,
    ).unwrap();

    if let Some(merged_node_set) = merged_node_set {
        let difflayer = merged_node_set.to_difflayer();
        triedb.flush(0, root_hash, &Some(difflayer)).unwrap();
    }

    triedb.state_at(root_hash, None).unwrap();

    for i in 0..total_operations {
        let hashed_address = keccak256((i as u64).to_le_bytes());
        triedb.get_account_with_hash_state(hashed_address).unwrap().unwrap();
    }
    triedb.clean();

    println!("Result: {:?}", root_hash);

    
}