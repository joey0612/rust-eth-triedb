//! Trie database implementation.

use std::sync::{Arc};
use std::collections::{HashMap, HashSet};
use rayon::prelude::*;
use std::time::Instant;

use alloy_primitives::{keccak256, Address, B256, U256};
use alloy_trie::{EMPTY_ROOT_HASH};
use reth_trie_common::HashedPostState;
use rust_eth_triedb_common::TrieDatabase;
use rust_eth_triedb_state_trie::node::{MergedNodeSet, NodeSet, DiffLayer, Node};
use rust_eth_triedb_state_trie::state_trie::StateTrie;
use rust_eth_triedb_state_trie::account::StateAccount;
use rust_eth_triedb_state_trie::{SecureTrieId, SecureTrieTrait, SecureTrieBuilder};
use rust_eth_triedb_state_trie::encoding::{TRIE_STATE_ROOT_KEY, TRIE_STATE_BLOCK_NUMBER_KEY};

use reth_metrics::{
    metrics::{Histogram},
    Metrics,
};

/// Metrics for the `TrieDB`.
#[derive(Metrics, Clone)]
#[metrics(scope = "rust.eth.triedb")]
pub(crate) struct TrieDBMetrics {
    /// Histogram of hash durations (in seconds)
    pub(crate) hash_duration: Histogram,
    /// Histogram of commit durations (in seconds)
    pub(crate) commit_duration: Histogram,
    /// Histogram of flush durations (in seconds)
    pub(crate) flush_duration: Histogram,
}

impl TrieDBMetrics {
    pub(crate) fn record_hash_duration(&self, duration: f64) {
        self.hash_duration.record(duration);
    }

    pub(crate) fn record_commit_duration(&self, duration: f64) {
        self.commit_duration.record(duration);
    }

    pub(crate) fn record_flush_duration(&self, duration: f64) {
        self.flush_duration.record(duration);
    }
}

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
    root_hash: B256,
    account_trie: Option<StateTrie<DB>>,
    storage_tries: HashMap<B256, StateTrie<DB>>,
    accounts_with_storage_trie: HashMap<B256, StateAccount>,
    difflayer: Option<Arc<DiffLayer>>,
    db: DB,
    metrics: TrieDBMetrics,
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
            db: self.db.clone(),
            metrics: self.metrics.clone()
        }
    }
}

/// External Initializer and getters 
impl<DB> TrieDB<DB>
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    /// Creates a new trie database
    pub fn new(db: DB) -> Self {
        Self {
            root_hash: EMPTY_ROOT_HASH,
            account_trie: None,
            storage_tries: HashMap::new(),
            accounts_with_storage_trie: HashMap::new(),
            difflayer: None,
            db: db.clone(),
            metrics: TrieDBMetrics::new_with_labels(&[("instance", "default")]),
        }
    }

    /// Reset the state of the trie db to the given root hash and difflayer
    pub fn state_at(&mut self, root_hash: B256, difflayer: Option<Arc<DiffLayer>>) -> Result<(), TrieDBError> {
        let id = SecureTrieId::new(root_hash);
        self.account_trie = Some(
            SecureTrieBuilder::new(self.db.clone())
            .with_id(id)
            .build_with_difflayer(difflayer.as_ref())?
        );
        self.root_hash = root_hash;
        self.difflayer = difflayer;
        self.storage_tries.clear();
        self.accounts_with_storage_trie.clear();
        Ok(())
    }

    /// Gets a mutable reference to the database
    pub fn get_mut_db_ref(&mut self) -> &mut DB {
        &mut self.db
    }

    /// Clean the trie db
    pub fn clean(&mut self) {
        self.root_hash = EMPTY_ROOT_HASH;
        self.account_trie = None;
        self.storage_tries.clear();
        self.accounts_with_storage_trie.clear();

        if let Some(difflayer) = &self.difflayer {
            println!("clean, 111111111111 difflayer, reference count: {:?}", Arc::strong_count(difflayer));
        } else {
            println!("clean, 111111111111 difflayer, reference none");
        }
        self.difflayer = None;
    }

    pub fn debug_reference_count(&self) {
        if let Some(account_trie) = &self.account_trie {
            println!("     account_trie, addr: {:p}, reference count: {:?}", &**account_trie.trie().root() as *const Node, Arc::strong_count(account_trie.trie().root()));
        } else {
            println!("     account_trie, reference none");
        }
        if let Some(difflayer) = &self.difflayer {
            println!("     difflayer, reference count: {:?}", Arc::strong_count(difflayer));
        } else {
            println!("     difflayer, reference none");
        }
        for (_, value) in self.storage_tries.iter() {
            println!("     storage_trie, addr: {:p}, reference count: {:?}", &**value.trie().root() as *const Node, Arc::strong_count(&value.trie().root()));
        }
    }
}


/// Internally helper functions
impl<DB> TrieDB<DB>
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    /// Gets the storage trie for an account
    fn get_storage_trie(&mut self, address: Address) -> Result<StateTrie<DB>, TrieDBError> {
        let hashed_address = keccak256(address.as_slice());
        return Ok(self.get_storage_trie_with_hash_state(hashed_address)?);
    }

    /// Gets the storage trie for an hash address
    fn get_storage_trie_with_hash_state(&mut self, hashed_address: B256) -> Result<StateTrie<DB>, TrieDBError> {
        if let Some(storage_trie) = self.storage_tries.get(&hashed_address) {
            return Ok(storage_trie.clone());
        }

        let storage_root = self.get_storage_root_with_hash_state(hashed_address)?;
        let id = SecureTrieId::new(storage_root)
            .with_owner(hashed_address);
        let storage_trie = SecureTrieBuilder::new(self.db.clone())
            .with_id(id)
            .build_with_difflayer(self.difflayer.as_ref())?;

        self.storage_tries.insert(hashed_address, storage_trie.clone());
        Ok(storage_trie)
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
}

/// Geth interface functions
/// Storage update and delete functions are not ready in the current implementation
/// Get functions can used to prewarm the trie db
impl<DB> TrieDB<DB>
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    pub fn get_account(&mut self, address: Address) -> Result<Option<StateAccount>, TrieDBError> {
        Ok(self.account_trie.as_mut().unwrap().get_account(address)?)
    }

    pub fn update_account(&mut self, address: Address, account: &StateAccount) -> Result<(), TrieDBError> {
        Ok(self.account_trie.as_mut().unwrap().update_account(address, account)?)
    }

    pub fn delete_account(&mut self, address: Address) -> Result<(), TrieDBError> {
        Ok(self.account_trie.as_mut().unwrap().delete_account(address)?)
    }

    pub fn get_storage(&mut self, address: Address, key: &[u8]) -> Result<Option<Vec<u8>>, TrieDBError> {
        let mut storage_trie = self.get_storage_trie(address)?;
        Ok(storage_trie.get_storage(address, key)?)
    }

    #[allow(dead_code)]
    fn update_storage(&mut self, address: Address, key: &[u8], value: &[u8]) -> Result<(), TrieDBError> {
        let mut storage_trie = self.get_storage_trie(address)?;
        Ok(storage_trie.update_storage(address, key, value)?)
    }

    #[allow(dead_code)]
    fn delete_storage(&mut self, address: Address, key: &[u8]) -> Result<(), TrieDBError> {
        let mut storage_trie = self.get_storage_trie(address)?;
        Ok(storage_trie.delete_storage(address, key)?)
    }

    pub fn calculate_hash(&mut self) -> Result<B256, TrieDBError> {
        println!("calculate_hash, 111111111111");
        self.debug_reference_count();

        let hash_start = Instant::now();

        let (storage_hashes, storage_tries): (HashMap<B256, B256>, HashMap<B256, StateTrie<DB>>) = self.storage_tries
        .par_iter()
        .map(|(key, trie)| {
            let mut trie_clone = trie.clone();
            let hash = trie_clone.hash();
            (*key, hash, trie_clone)
        })
        .collect::<Vec<_>>()
        .into_iter()
        .fold((HashMap::new(), HashMap::new()), |(mut hashes, mut tries), (key, hash, trie)| {
            hashes.insert(key, hash);
            tries.insert(key, trie);
            (hashes, tries)
        });

        println!("calculate_hash, 222222222222");
        self.debug_reference_count();

        for (hashed_address, storage_hash) in storage_hashes {   
            let mut account = self.accounts_with_storage_trie.get(&hashed_address).unwrap().clone();
            account.storage_root = storage_hash;
            self.update_account_with_hash_state(hashed_address, &account)?;
        }
        println!("calculate_hash, 333333333333");
        self.debug_reference_count();

        self.storage_tries = storage_tries;

        println!("calculate_hash, 444444444444");
        self.debug_reference_count();

        self.metrics.record_hash_duration(hash_start.elapsed().as_secs_f64());
        Ok(self.account_trie.as_mut().unwrap().hash())
    }

    pub fn commit(&mut self, _collect_leaf: bool) -> Result<(B256, Arc<MergedNodeSet>), TrieDBError> {
        
        println!("commit, 111111111111");
        self.debug_reference_count();

        let root_hash = self.calculate_hash()?;

        println!("commit, 222222222222");
        self.debug_reference_count();

        let commit_start = Instant::now();
        let mut merged_node_set = MergedNodeSet::new();

        // Start both tasks in parallel using rayon
        let mut account_trie_clone = self.account_trie.as_mut().unwrap().clone();
        let (account_commit_result, storage_commit_results): (Result<(B256, Option<Arc<NodeSet>>), _>, Vec<(B256, Option<Arc<NodeSet>>)>) = rayon::join(
            || account_trie_clone.commit(true),
            || self.storage_tries
                .par_iter()
                .map(|(hashed_address, trie)| {
                    let (_, node_set) = trie.clone().commit(false).unwrap();
                    (*hashed_address, node_set)
                })
                .collect()
        );

        println!("before drop, account_trie_clone, addr: {:p}, reference count: {:?}", &**account_trie_clone.trie().root() as *const Node, Arc::strong_count(&account_trie_clone.trie().root()));
        drop(account_trie_clone);

        println!("commit, 333333333333");
        self.debug_reference_count();

        let (_, account_node_set) = account_commit_result?;

        if let Some(node_set) = account_node_set {
            merged_node_set.merge(node_set)
                .map_err(|e| TrieDBError::Database(e))?;
        }

        for (_, node_set) in storage_commit_results {
            if let Some(node_set) = node_set {
                merged_node_set.merge(node_set)
                    .map_err(|e| TrieDBError::Database(e))?;
            }
        }

        self.metrics.record_commit_duration(commit_start.elapsed().as_secs_f64());

        println!("commit, 444444444444");
        self.debug_reference_count();

        Ok((root_hash, Arc::new(merged_node_set)))
    }
}

/// Compatible with the clients using hashed keys to access triedb
/// Storage update and delete functions are not ready in the current implementation
/// Get functions can used to prewarm the trie db
impl<DB> TrieDB<DB>
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    pub fn get_account_with_hash_state(&mut self, hashed_address: B256) -> Result<Option<StateAccount>, TrieDBError> {
        Ok(self.account_trie.as_mut().unwrap().get_account_with_hash_state(hashed_address)?)
    }

    pub fn update_account_with_hash_state(&mut self, hashed_address: B256, account: &StateAccount) -> Result<(), TrieDBError> {
        Ok(self.account_trie.as_mut().unwrap().update_account_with_hash_state(hashed_address, account)?)
    }
    
    pub fn delete_account_with_hash_state(&mut self, hashed_address: B256) -> Result<(), TrieDBError> {
        Ok(self.account_trie.as_mut().unwrap().delete_account_with_hash_state(hashed_address)?)
    }

    pub fn get_storage_with_hash_state(&mut self, hashed_address: B256, hashed_key: B256) -> Result<Option<Vec<u8>>, TrieDBError> {
        let mut storage_trie = self.get_storage_trie_with_hash_state(hashed_address)?;
        Ok(storage_trie.get_storage_with_hash_state(hashed_address, hashed_key)?)
    }

    #[allow(dead_code)]
    fn update_storage_with_hash_state(&mut self, hashed_address: B256, hashed_key: B256, value: &[u8]) -> Result<(), TrieDBError> {
        let mut storage_trie = self.get_storage_trie_with_hash_state(hashed_address)?;
        Ok(storage_trie.update_storage_with_hash_state(hashed_address, hashed_key, value)?)
    }

    #[allow(dead_code)]
    fn delete_storage_with_hash_state(&mut self, hashed_address: B256, hashed_key: B256) -> Result<(), TrieDBError> {
        let mut storage_trie = self.get_storage_trie_with_hash_state(hashed_address)?;
        Ok(storage_trie.delete_storage_with_hash_state(hashed_address, hashed_key)?)
    }
}


/// Compatible with Reth client usage scenarios
impl<DB> TrieDB<DB>
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{  
    /// Transfers HashedPostState to triedb structure and commits the changes
    /// Compatible with Reth usage scenarios
    pub fn commit_hashed_post_state(
        &mut self, 
        root_hash: B256, 
        difflayer: Option<Arc<DiffLayer>>, 
        hashed_post_state: &HashedPostState) -> 
        Result<(B256, Option<Arc<DiffLayer>>), TrieDBError> {

        println!("commit_hashed_post_state, 111111111111");
        self.debug_reference_count();

        if let Some(ref difflayer) = difflayer {
            println!("commit_hashed_post_state, 222222222222, difflayer: {:?}", Arc::strong_count(difflayer));
        } else {
            println!("commit_hashed_post_state, 222222222222, difflayer: none");
        }

        let mut states: HashMap<alloy_primitives::FixedBytes<32>, Option<StateAccount>> = HashMap::new();
        let mut states_rebuild = HashSet::new();
        let mut storage_states = HashMap::new();
        
        for (hashed_address, account) in hashed_post_state.accounts.iter() {
            match account {
                Some(account) => {
                    let code_hash = match account.bytecode_hash {
                        Some(code_hash) => code_hash,
                        None => alloy_trie::KECCAK_EMPTY
                    };
                    let acc = StateAccount::default()
                        .with_nonce(account.nonce)
                        .with_balance(account.balance)
                        .with_code_hash(code_hash);
                    states.insert(*hashed_address, Some(acc));

                    // check if the account is being rebuilt
                    if let Some(storages) = hashed_post_state.storages.get(hashed_address) {
                        if storages.wiped {
                            states_rebuild.insert(*hashed_address);
                        }
                    }
                }
                None => {
                    states.insert(*hashed_address, None);
                }
            }
        }

        for (hashed_address, storages) in hashed_post_state.storages.iter() {
            if storages.storage.is_empty() {
                continue;
            }
            let mut kvs = HashMap::new();
            for (hashed_key, value) in storages.storage.iter() {
                if value.is_zero() {
                    // if the value is zero, it means the storage is being deleted
                    kvs.insert(*hashed_key, None);
                } else {
                    kvs.insert(*hashed_key, Some(*value));
                }
            }
            storage_states.insert(*hashed_address, kvs);
        }

        let (root_hash, node_set) = self.update_and_commit(
            root_hash, 
            difflayer, 
            states, 
            states_rebuild, 
            storage_states)?;

        if let Some(node_set) = node_set {
            let difflayer = node_set.to_difflayer();
            return Ok((root_hash, Some(difflayer)));
        } 
        Ok((root_hash, None))
    }

    /// Batch update the changes and commit
    /// Compatible with Reth usage scenarios
    /// 
    /// 1. Reset the trie db state
    /// 2. Prepare accounts to be updated
    /// 3. Prepare required data to avoid borrowing conflicts for parallel execution
    /// 4. Parallel execution: update accounts and storage simultaneously
    /// 5. Commit the changes
    pub fn update_and_commit(
        &mut self, 
        root_hash: B256, 
        difflayer: Option<Arc<DiffLayer>>, 
        states: HashMap<B256, Option<StateAccount>>,
        states_rebuild: HashSet<B256>,
        storage_states: HashMap<B256, HashMap<B256, Option<U256>>>) -> 
        Result<(B256, Option<Arc<MergedNodeSet>>), TrieDBError> {

        println!("update_and_commit, 111111111111");
        self.debug_reference_count();

        // 1. Reset the trie db state
        self.state_at(root_hash, difflayer)?;

        println!("update_and_commit, 222222222222");
        self.debug_reference_count();

        // 2. Prepare accounts to be updated
        let mut update_accounts = HashMap::new();
        let mut update_accounts_with_storage = HashMap::new();
        for (hashed_address, new_account) in states {
            if new_account.is_none() {
                // if the account is deleted, None is inserted
                update_accounts.insert(hashed_address, None);
                continue;
            }

            let final_account = if states_rebuild.contains(&hashed_address) {
                // if the account is being rebuilt, use the new account
                new_account.unwrap()
            } else {
                // if the account is not being rebuilt, use the old account
                let old_account = self.get_account_with_hash_state(hashed_address)?;           
                match old_account {
                    Some(mut acc) => {
                        // keep the old account's storage root
                        let new_account = new_account.unwrap();
                        acc.nonce = new_account.nonce;
                        acc.balance = new_account.balance;
                        acc.code_hash = new_account.code_hash;
                        acc
                    }
                    None => {
                        new_account.unwrap()
                    }
                }
            };
            
            if storage_states.contains_key(&hashed_address) {
                update_accounts_with_storage.insert(hashed_address, final_account);
            } else {
                update_accounts.insert(hashed_address, Some(final_account));
            }
        }
        self.accounts_with_storage_trie = update_accounts_with_storage.clone();

        // 3. Prepare required data to avoid borrowing conflicts for parallel execution
        let db_clone = self.db.clone();
        let difflayer_clone = self.difflayer.as_ref().map(|d| d.clone());

        // 4. Parallel execution: update accounts and storage simultaneously
        let (_, update_storage): ((), HashMap<B256, StateTrie<DB>>) = rayon::join(
            || {
                // Task 1: Update account trie (serial execution)
                // delete accounts that are being rebuilt, to collect deleted trie nodes
                for hashed_address in states_rebuild {
                    self.delete_account_with_hash_state(hashed_address).unwrap();
                }
                // update accounts that are being updated
                for (hashed_address, account) in update_accounts {
                    if let Some(account) = account {
                        self.update_account_with_hash_state(hashed_address, &account).unwrap();
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
                        let account = update_accounts_with_storage.get(&hashed_address).unwrap();
                        let storage_root = account.storage_root;

                        let id = SecureTrieId::new(storage_root)
                            .with_owner(hashed_address);
                        let mut storage_trie = SecureTrieBuilder::new(db_clone.clone())
                            .with_id(id)
                            .build_with_difflayer(difflayer_clone.as_ref()).unwrap();

                        // Serial execution for kvs within each address
                        for (hashed_key, new_value) in kvs {
                            if let Some(new_value) = new_value {
                                storage_trie.update_storage_u256_with_hash_state(hashed_address, hashed_key, new_value).unwrap();
                            } else {
                                storage_trie.delete_storage_with_hash_state(hashed_address, hashed_key).unwrap();
                            }
                        }

                        (hashed_address, storage_trie)
                    })
                    .collect()
            }
        );
        self.storage_tries = update_storage;

        drop(db_clone);
        drop(difflayer_clone);

        println!("update_and_commit, 333333333333");
        self.debug_reference_count();

        // 5. Commit the changes
        let (root_hash, node_set) = self.commit(true)?;

        println!("update_and_commit, 444444444444");
        self.debug_reference_count();

        self.clean();

        println!("update_and_commit, 555555555555");
        self.debug_reference_count();

        Ok((root_hash, Some(node_set)))
    }
}


/// Flush trienodes to PathDB, after commit
impl<DB> TrieDB<DB>
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    pub fn flush(&mut self, block_number: u64, state_root: B256, update_nodes: &Option<Arc<DiffLayer>>) -> Result<(), TrieDBError> {
        let flush_start = Instant::now();

        if let Some(difflayer) = update_nodes {
            for (key, node) in difflayer.as_ref() {
                if node.is_deleted() {
                    self.db.remove(&key);
                } else {
                    self.db.insert(&key, node.blob.as_ref().unwrap().clone())
                        .map_err(|e| TrieDBError::Database(format!("Failed to insert node: {:?}", e)))?;
                }
            }
        }
        self.db.insert(TRIE_STATE_ROOT_KEY, state_root.as_slice().to_vec())
            .map_err(|e| TrieDBError::Database(format!("Failed to insert state root: {:?}", e)))?;
        self.db.insert(TRIE_STATE_BLOCK_NUMBER_KEY, block_number.to_le_bytes().to_vec())
            .map_err(|e| TrieDBError::Database(format!("Failed to insert block number: {:?}", e)))?;
        
        self.metrics.record_flush_duration(flush_start.elapsed().as_secs_f64());
        Ok(())
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