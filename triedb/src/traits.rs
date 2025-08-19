//! Traits for trie database operations.

use std::sync::Arc;
use std::collections::HashMap;
use alloy_primitives::{B256, Address};
use reth_triedb_state_trie::account::StateAccount;
use reth_triedb_state_trie::node::MergedNodeSet;

/// Error type for trie database operations
pub type TrieDBError = super::triedb::TrieDBError;

/// Trait for trie database operations
pub trait TrieDBTrait: Sized {
    /// Associated error type
    type Error;

    /// Opens the trie database at a given root hash
    fn state_at(&mut self, root_hash: B256, difflayer: Option<Arc<MergedNodeSet>>) -> Result<Self, Self::Error>;

    /// Gets an account from the trie by address
    fn get_account(&mut self, address: Address) -> Result<Option<StateAccount>, Self::Error>;

    /// Updates an account in the trie by address
    fn update_account(&mut self, address: Address, account: &StateAccount) -> Result<(), Self::Error>;

    /// Deletes an account from the trie by address
    fn delete_account(&mut self, address: Address) -> Result<(), Self::Error>;

    /// Gets storage value for an account by key
    fn get_storage(&mut self, address: Address, key: &[u8]) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Updates storage value for an account by key
    fn update_storage(&mut self, address: Address, key: &[u8], value: &[u8]) -> Result<(), Self::Error>;

    /// Deletes storage value for an account by key
    fn delete_storage(&mut self, address: Address, key: &[u8]) -> Result<(), Self::Error>;

    /// Gets an account from the trie by hash state
    fn get_account_with_hash_state(&mut self, hashed_address: B256) -> Result<Option<StateAccount>, Self::Error>;

    /// Updates an account from the trie by hash state
    fn update_account_with_hash_state(&mut self, hashed_address: B256, account: &StateAccount) -> Result<(), Self::Error>;

    /// Deletes an account from the trie by hash state
    fn delete_account_with_hash_state(&mut self, hashed_address: B256) -> Result<(), Self::Error>;

    /// Gets storage value for an account by key and hash state
    fn get_storage_with_hash_state(&mut self, address: B256, hashed_key: B256) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Updates storage value for an account by key and hash state
    fn update_storage_with_hash_state(&mut self, address: B256, hashed_key: B256, value: &[u8]) -> Result<(), Self::Error>;

    /// Deletes storage value for an account by key and hash state
    fn delete_storage_with_hash_state(&mut self, address: B256, hashed_key: B256) -> Result<(), Self::Error>;

    /// Returns the current root hash of the triedb
    fn calculate_hash(&mut self) -> Result<B256, Self::Error>;

    /// Commits the trie and returns the root hash and modified node set
    ///
    /// The `collect_leaf` parameter determines whether to include leaf nodes in the returned node set.
    /// The returned `NodeSet` contains all modified nodes that need to be persisted to disk.
    fn commit(&mut self, collect_leaf: bool) -> Result<(B256, Arc<MergedNodeSet>), Self::Error>;

    /// Updates the states of the trie with the given states and storage states
    fn update_all(
        &mut self, 
        root_hash: B256, 
        difflayer: Option<Arc<MergedNodeSet>>, 
        states: HashMap<B256, Option<StateAccount>>, 
        storage_states: HashMap<B256, HashMap<B256, Option<Vec<u8>>>>) -> Result<(B256, Option<Arc<MergedNodeSet>>), TrieDBError>;

    fn flush(&mut self, nodes: Option<Arc<MergedNodeSet>>) -> Result<(), Self::Error>;
}