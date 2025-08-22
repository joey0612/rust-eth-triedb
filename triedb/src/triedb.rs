//! Trie database implementation.

use std::sync::{Arc};
use std::collections::HashMap;
use rayon::prelude::*;

use alloy_primitives::{keccak256, Address, B256};
use alloy_trie::{EMPTY_ROOT_HASH};
use rust_eth_triedb_common::TrieDatabase;
use rust_eth_triedb_state_trie::node::{MergedNodeSet, NodeSet, DiffLayer};
use rust_eth_triedb_state_trie::state_trie::StateTrie;
use rust_eth_triedb_state_trie::account::StateAccount;
use rust_eth_triedb_state_trie::{SecureTrieId, SecureTrieTrait, SecureTrieBuilder};

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
#[derive(Clone)]
pub struct TrieDB<DB> 
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    root_hash: B256,
    account_trie: StateTrie<DB>,
    storage_tries: HashMap<B256, StateTrie<DB>>,
    accounts_with_storage_trie: HashMap<B256, StateAccount>,
    difflayer: Option<Arc<DiffLayer>>,
    db: DB,
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
            .field("difflayer", &self.difflayer.as_ref().map(|_| "<MergedNodeSet>"))
            .field("db", &format!("<{}>", std::any::type_name::<DB>()))
            .finish()
    }
}

impl<DB> TrieDB<DB>
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    /// Creates a new trie database
    pub fn new(db: DB) -> Self {
        let id = SecureTrieId::new(EMPTY_ROOT_HASH);
        let account_trie = SecureTrieBuilder::new(db.clone()).with_id(id).build().unwrap();

        Self {
            root_hash: EMPTY_ROOT_HASH,
            account_trie: account_trie,
            storage_tries: HashMap::new(),
            accounts_with_storage_trie: HashMap::new(),
            difflayer: None,
            db: db.clone(),
        }
    }

    /// Gets the storage root for an account with hash state
    fn get_storage_root_with_hash_state(&mut self, hashed_address: B256) -> Result<B256, TrieDBError> {
        let account = self.get_account_with_hash_state(hashed_address)?;
        if let Some(acc) = account {
            self.accounts_with_storage_trie.insert(hashed_address, acc);
            Ok(acc.storage_root)
        } else {
            self.accounts_with_storage_trie.insert(hashed_address, StateAccount::default());
            Ok(EMPTY_ROOT_HASH)
        }
    }

    /// Gets the storage trie for an account
    fn get_storage_trie(&mut self, address: Address) -> Result<StateTrie<DB>, TrieDBError> {
        let hashed_address = keccak256(address.as_slice());

        if let Some(storage_trie) = self.storage_tries.get(&hashed_address) {
            return Ok(storage_trie.clone());
        }

        let storage_root = self.get_storage_root_with_hash_state(hashed_address)?;
        let id = SecureTrieId::new(storage_root)
            .with_owner(hashed_address);
        let mut storage_trie = SecureTrieBuilder::new(self.db.clone())
            .with_id(id)
            .build()?;

        storage_trie.with_difflayer(self.difflayer.clone())?;

        self.storage_tries.insert(hashed_address, storage_trie.clone());
        Ok(storage_trie)
    }

    fn get_storage_trie_with_hash_state(&mut self, hashed_address: B256) -> Result<StateTrie<DB>, TrieDBError> {
        if let Some(storage_trie) = self.storage_tries.get(&hashed_address) {
            return Ok(storage_trie.clone());
        }

        let storage_root = self.get_storage_root_with_hash_state(hashed_address)?;
        let id = SecureTrieId::new(storage_root)
            .with_owner(hashed_address);
        let mut storage_trie = SecureTrieBuilder::new(self.db.clone())
            .with_id(id)
            .build()?;
        storage_trie.with_difflayer(self.difflayer.clone())?;

        self.storage_tries.insert(hashed_address, storage_trie.clone());
        Ok(storage_trie)
    }
}

impl<DB> TrieDB<DB>
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    pub fn state_at(&mut self, root_hash: B256, difflayer: Option<Arc<DiffLayer>>) -> Result<Self, TrieDBError> {
        let id = SecureTrieId::new(root_hash);
        self.account_trie = SecureTrieBuilder::new(self.db.clone())
        .with_id(id)
        .build()?;
        self.account_trie.with_difflayer(difflayer.clone())?;

        self.root_hash = root_hash;
        self.difflayer = difflayer;
        self.storage_tries.clear();
        Ok(self.clone())
    }

    pub fn get_account(&mut self, address: Address) -> Result<Option<StateAccount>, TrieDBError> {
        Ok(self.account_trie.get_account(address)?)
    }

    pub fn update_account(&mut self, address: Address, account: &StateAccount) -> Result<(), TrieDBError> {
        Ok(self.account_trie.update_account(address, account)?)
    }

    pub fn delete_account(&mut self, address: Address) -> Result<(), TrieDBError> {
        Ok(self.account_trie.delete_account(address)?)
    }

    pub fn get_storage(&mut self, address: Address, key: &[u8]) -> Result<Option<Vec<u8>>, TrieDBError> {
        let mut storage_trie = self.get_storage_trie(address)?;
        Ok(storage_trie.get_storage(address, key)?)
    }

    pub fn update_storage(&mut self, address: Address, key: &[u8], value: &[u8]) -> Result<(), TrieDBError> {
        let mut storage_trie = self.get_storage_trie(address)?;
        Ok(storage_trie.update_storage(address, key, value)?)
    }

    pub fn delete_storage(&mut self, address: Address, key: &[u8]) -> Result<(), TrieDBError> {
        let mut storage_trie = self.get_storage_trie(address)?;
        Ok(storage_trie.delete_storage(address, key)?)
    }

    pub fn get_account_with_hash_state(&mut self, hashed_address: B256) -> Result<Option<StateAccount>, TrieDBError> {
        Ok(self.account_trie.get_account_with_hash_state(hashed_address)?)
    }

    pub fn update_account_with_hash_state(&mut self, hashed_address: B256, account: &StateAccount) -> Result<(), TrieDBError> {
        Ok(self.account_trie.update_account_with_hash_state(hashed_address, account)?)
    }
    
    pub fn delete_account_with_hash_state(&mut self, hashed_address: B256) -> Result<(), TrieDBError> {
        Ok(self.account_trie.delete_account_with_hash_state(hashed_address)?)
    }

    pub fn get_storage_with_hash_state(&mut self, hashed_address: B256, hashed_key: B256) -> Result<Option<Vec<u8>>, TrieDBError> {
        let mut storage_trie = self.get_storage_trie_with_hash_state(hashed_address)?;
        Ok(storage_trie.get_storage_with_hash_state(hashed_address, hashed_key)?)
    }

    pub fn update_storage_with_hash_state(&mut self, hashed_address: B256, hashed_key: B256, value: &[u8]) -> Result<(), TrieDBError> {
        let mut storage_trie = self.get_storage_trie_with_hash_state(hashed_address)?;
        Ok(storage_trie.update_storage_with_hash_state(hashed_address, hashed_key, value)?)
    }

    pub fn delete_storage_with_hash_state(&mut self, hashed_address: B256, hashed_key: B256) -> Result<(), TrieDBError> {
        let mut storage_trie = self.get_storage_trie_with_hash_state(hashed_address)?;
        Ok(storage_trie.delete_storage_with_hash_state(hashed_address, hashed_key)?)
    }

    pub fn calculate_hash(&mut self) -> Result<B256, TrieDBError> {
        let storage_hashes: HashMap<B256, B256> = self.storage_tries
        .par_iter()
        .map(|(key, trie)| (*key, trie.clone().hash()))
        .collect();

        if self.accounts_with_storage_trie.len() != storage_hashes.len() {
            panic!("accounts_with_storage_trie and storage_tries have different lengths");
        }

        for (hashed_address, storage_hash) in storage_hashes {            
            let mut account = self.accounts_with_storage_trie.get(&hashed_address).unwrap().clone();
            account.storage_root = storage_hash;
            self.update_account_with_hash_state(hashed_address, &account)?;
        }

        Ok(self.account_trie.hash())
    }

    pub fn commit(&mut self, _collect_leaf: bool) -> Result<(B256, Arc<MergedNodeSet>), TrieDBError> {
        let root_hash = self.calculate_hash()?;

        let mut merged_node_set = MergedNodeSet::new();

        // Start both tasks in parallel using rayon
        let mut account_trie_clone = self.account_trie.clone();
        let (account_commit_result, storage_results): (Result<(B256, Option<Arc<NodeSet>>), _>, Vec<(B256, Option<Arc<NodeSet>>)>) = rayon::join(
            || account_trie_clone.commit(true),
            || self.storage_tries
                .par_iter()
                .map(|(hashed_address, trie)| {
                    let (root_hash, node_set) = trie.clone().commit(false).unwrap_or((B256::ZERO, None));
                    if root_hash == EMPTY_ROOT_HASH || root_hash == B256::ZERO {
                        panic!("storage root hash is empty");
                    }
                    (*hashed_address, node_set)
                })
                .collect()
        );

        let (_, account_node_set) = account_commit_result?;

        merged_node_set.merge(account_node_set.unwrap())
            .map_err(|e| TrieDBError::Database(e))?;

        for (_, node_set) in storage_results {
            merged_node_set.merge(node_set.unwrap())
                .map_err(|e| TrieDBError::Database(e))?;
        }

        Ok((root_hash, Arc::new(merged_node_set))) 
    }
    
    pub fn update_and_commit(
        &mut self, 
        root_hash: B256, 
        difflayer: Option<Arc<DiffLayer>>, 
        states: HashMap<B256, Option<StateAccount>>, 
        storage_states: HashMap<B256, HashMap<B256, Option<Vec<u8>>>>) -> Result<(B256, Option<Arc<MergedNodeSet>>), TrieDBError> {
        
        // clear the trie db state
        self.state_at(root_hash, difflayer)?;

        // touch and update the accounts
        let mut update_accounts = HashMap::new();
        let mut update_accounts_with_storage = HashMap::new();
        for (hashed_address, new_account) in states {
            if new_account.is_none() {
                update_accounts.insert(hashed_address, None);
                continue;
            }

            let mut old_account = self.get_account_with_hash_state(hashed_address)?;
            if old_account.is_some() {
                old_account.unwrap().nonce = new_account.unwrap().nonce;
                old_account.unwrap().balance = new_account.unwrap().balance;
                old_account.unwrap().code_hash = new_account.unwrap().code_hash;
            } else {
                old_account = new_account;
            }
            if storage_states.contains_key(&hashed_address) {
                update_accounts_with_storage.insert(hashed_address, old_account.unwrap());
            } else {
                update_accounts.insert(hashed_address, old_account);
            }
        }
        self.accounts_with_storage_trie = update_accounts_with_storage;

        let accounts_with_storage_trie_len = self.accounts_with_storage_trie.len();
        let storage_states_len = storage_states.len();
        assert_eq!(accounts_with_storage_trie_len, storage_states_len);

        // Clone required data to avoid borrowing conflicts
        let accounts_clone = self.accounts_with_storage_trie.clone();
        let db_clone = self.db.clone();
        let difflayer_clone = self.difflayer.as_ref().map(|d| d.clone());
        
        // Parallel execution: update accounts and storage simultaneously
        let (_, update_storage): ((), HashMap<B256, StateTrie<DB>>) = rayon::join(
            || {
                // Task 1: Update account trie (serial execution)
                for (hashed_address, account) in update_accounts {
                    if account.is_some() {
                        self.update_account_with_hash_state(hashed_address, &account.unwrap()).unwrap();
                    } else {
                        self.delete_account_with_hash_state(hashed_address).unwrap();
                    }
                }
            },
            || {
                // Task 2: Update storage states (parallel execution for addresses, serial for kvs)
                storage_states
                    .into_par_iter()
                    .map(|(hashed_address, kvs)| {
                        let account = accounts_clone.get(&hashed_address).unwrap();
                        let storage_root = account.storage_root;
                        
                        let id = SecureTrieId::new(storage_root)
                            .with_owner(hashed_address);
                        let mut storage_trie = SecureTrieBuilder::new(db_clone.clone())
                            .with_id(id)
                            .build().unwrap();
                        storage_trie.with_difflayer(difflayer_clone.as_ref().map(|d| d.clone())).unwrap();

                        // Serial execution for kvs within each address
                        for (hashed_key, new_value) in kvs {
                            if new_value.is_none() {
                                storage_trie.delete_storage_with_hash_state(hashed_address, hashed_key).unwrap();
                            } else {
                                storage_trie.update_storage_with_hash_state(hashed_address, hashed_key, new_value.unwrap().as_slice()).unwrap();
                            }
                        }

                        (hashed_address, storage_trie)
                    })
                    .collect()
            }
        );
        self.storage_tries = update_storage;


        let (root_hash, node_set) = self.commit(true)?;
        Ok((root_hash, Some(node_set)))
    }

    pub fn flush(&mut self, update_nodes: Option<Arc<DiffLayer>>) -> Result<(), TrieDBError> {
        if update_nodes.is_none() {
            return Ok(());
        }

        let difflayer = update_nodes.unwrap();
        for (key, node) in difflayer.as_ref() {
            if node.is_deleted() {
                self.db.remove(&key);
            } else {
                self.db.insert(&key, node.blob.as_ref().unwrap().clone())
                    .map_err(|e| TrieDBError::Database(format!("Failed to insert node: {:?}", e)))?;
            }
        }
        Ok(())
    }
}
