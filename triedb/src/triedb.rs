//! Trie database implementation.

use std::collections::HashMap;

use alloy_primitives::B256;
use alloy_trie::EMPTY_ROOT_HASH;
use rust_eth_triedb_common::TrieDatabase;
use rust_eth_triedb_snapshotdb::SnapshotDB;
use rust_eth_triedb_state_trie::node::DiffLayers;
use rust_eth_triedb_state_trie::state_trie::StateTrie;
use rust_eth_triedb_state_trie::account::StateAccount;
use rust_eth_triedb_state_trie::{SecureTrieId, SecureTrieBuilder};

use crate::triedb_metrics::TrieDBMetrics;

/// Error type for trie database operations
#[derive(Debug, thiserror::Error)]
pub enum TrieDBError {
    #[error("Database operation failed: {0}")]
    Database(String),
    
    #[error("Invalid data format: {0}")]
    InvalidData(String),
    
    #[error("Operation not supported: {0}")]
    NotSupported(String),
    
    #[error("State trie error: {0}")]
    StateTrie(#[from] rust_eth_triedb_state_trie::secure_trie::SecureTrieError),
}

/// Trie database implementation
pub struct TrieDB<DB> 
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    pub(crate) root_hash: B256,
    pub(crate) account_trie: Option<StateTrie<DB>>,
    pub(crate) storage_tries: HashMap<B256, StateTrie<DB>>,
    pub(crate) accounts_with_storage_trie: HashMap<B256, StateAccount>,
    pub(crate) difflayer: Option<DiffLayers>,
    pub path_db: DB,
    pub snap_db: SnapshotDB,
    pub(crate) metrics: TrieDBMetrics,
}

/// External Initializer and getters 
impl<DB> TrieDB<DB>
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    /// Creates a new trie database
    pub fn new(path_db: DB, snap_db: SnapshotDB) -> Self {
        Self {
            root_hash: EMPTY_ROOT_HASH,
            account_trie: None,
            storage_tries: HashMap::new(),
            accounts_with_storage_trie: HashMap::new(),
            difflayer: None,
            path_db: path_db.clone(),
            snap_db: snap_db.clone(),
            metrics: TrieDBMetrics::new_with_labels(&[("instance", "default")]),
        }
    }

    /// Reset the state of the trie db to the given root hash and difflayer
    pub fn state_at(&mut self, root_hash: B256, difflayer: Option<&DiffLayers>) -> Result<(), TrieDBError> {
        let id = SecureTrieId::new(root_hash);
        self.account_trie = Some(
            SecureTrieBuilder::new(self.path_db.clone())
            .with_id(id)
            .build_with_difflayer(difflayer)?
        );
        self.root_hash = root_hash;
        self.difflayer = difflayer.map(|d| d.clone());
        self.storage_tries.clear();
        self.accounts_with_storage_trie.clear();
        Ok(())
    }

    /// Gets a mutable reference to the database
    pub fn get_mut_path_db_ref(&mut self) -> &mut DB {
        &mut self.path_db
    }

    /// Clean the trie db
    pub fn clean(&mut self) {
        self.root_hash = EMPTY_ROOT_HASH;
        self.account_trie = None;
        self.storage_tries.clear();
        self.accounts_with_storage_trie.clear();
        self.difflayer = None;
    }
}

impl<DB> Clone for TrieDB<DB>
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    fn clone(&self) -> Self {
        Self {
            root_hash: EMPTY_ROOT_HASH,
            account_trie: None,
            storage_tries: HashMap::new(),
            accounts_with_storage_trie: HashMap::new(),
            difflayer: None,
            path_db: self.path_db.clone(),
            snap_db: self.snap_db.clone(),
            metrics: self.metrics.clone()
        }
    }
}

impl<DB> std::fmt::Debug for TrieDB<DB>
where
    DB: TrieDatabase + Clone + Send + Sync + std::fmt::Debug,
    DB::Error: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrieDB")
            .field("root_hash", &self.root_hash)
            .field("account_trie", &format!("<StateTrie<{}>>", std::any::type_name::<DB>()))
            .field("storage_tries_count", &self.storage_tries.len())
            .field("accounts_with_storage_trie_count", &self.accounts_with_storage_trie.len())
            .field("difflayer", &self.difflayer.as_ref().map(|_| "<Difflayer>"))
            .field("db", &format!("<{}>", std::any::type_name::<DB>()))
            .finish()
    }
}









