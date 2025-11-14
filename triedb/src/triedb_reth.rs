//! Reth-compatible implementations for TrieDB.

use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use rayon::prelude::*;
use std::time::Instant;

use alloy_primitives::B256;
use alloy_primitives::U256;
use alloy_trie::KECCAK_EMPTY;
use reth_trie_common::HashedPostState;
use rust_eth_triedb_common::TrieDatabase;
use rust_eth_triedb_state_trie::node::{MergedNodeSet, DiffLayer, DiffLayers};
use rust_eth_triedb_state_trie::state_trie::StateTrie;
use rust_eth_triedb_state_trie::account::StateAccount;
use rust_eth_triedb_state_trie::{SecureTrieId, SecureTrieTrait, SecureTrieBuilder};

use crate::triedb::{TrieDB, TrieDBError};

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
        difflayer: Option<&DiffLayers>, 
        hashed_post_state: &HashedPostState) -> 
        Result<(B256, Option<Arc<DiffLayer>>), TrieDBError> {

        let hashed_post_state_transform_start = Instant::now();
        let mut states: HashMap<alloy_primitives::FixedBytes<32>, Option<StateAccount>> = HashMap::new();
        let mut states_rebuild = HashSet::new();
        let mut storage_states = HashMap::new();
        
        for (hashed_address, account) in hashed_post_state.accounts.iter() {
            match account {
                Some(account) => {
                    let code_hash = match account.bytecode_hash {
                        Some(code_hash) => code_hash,
                        None => KECCAK_EMPTY
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

        self.metrics.record_hashed_post_state_transform_duration(hashed_post_state_transform_start.elapsed().as_secs_f64());

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
        difflayer: Option<&DiffLayers>, 
        states: HashMap<B256, Option<StateAccount>>,
        states_rebuild: HashSet<B256>,
        storage_states: HashMap<B256, HashMap<B256, Option<U256>>>) -> 
        Result<(B256, Option<Arc<MergedNodeSet>>), TrieDBError> {
        
        let update_prepare_start = Instant::now();

        // 1. Reset the trie db state
        self.state_at(root_hash, difflayer)?;

        // 2. Prepare accounts to be updated
        let mut update_accounts = HashMap::new();
        let mut update_accounts_with_storage = HashMap::new();
        let mut get_storage_root_from_trie_count = 0;
        for (hashed_address, new_account) in states {
            if new_account.is_none() {
                // if the account is deleted, None is inserted
                // self.storage_root_cache.write().unwrap().insert(hashed_address.as_slice().to_vec(), Some(alloy_trie::KECCAK_EMPTY.as_slice().to_vec()));
                update_accounts.insert(hashed_address, None);
                continue;
            }

            let final_account = if states_rebuild.contains(&hashed_address) {
                // if the account is being rebuilt, use the new account
                new_account.unwrap()
            }else {
                if let Some(storage_root) = self.snap_db.get_storage_root(hashed_address).unwrap() {
                    let mut new_account = new_account.unwrap();
                    new_account.storage_root = storage_root;
                    new_account
                } else {
                    get_storage_root_from_trie_count += 1;
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
                }
            };

            // self.storage_root_cache.write().unwrap().insert(hashed_address.as_slice().to_vec(), Some(final_account.storage_root.as_slice().to_vec()));
            self.metrics.get_storage_root_from_trie.set(get_storage_root_from_trie_count as f64);
            
            if storage_states.contains_key(&hashed_address) {
                update_accounts_with_storage.insert(hashed_address, final_account);
            } else {
                update_accounts.insert(hashed_address, Some(final_account));
            }
        }
        self.accounts_with_storage_trie = update_accounts_with_storage.clone();

        self.metrics.record_update_prepare_duration(update_prepare_start.elapsed().as_secs_f64());

        let update_start = Instant::now();
        // 3. Prepare required data to avoid borrowing conflicts for parallel execution
        let path_db_clone = self.path_db.clone();
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
                        let mut storage_trie = SecureTrieBuilder::new(path_db_clone.clone())
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

        drop(path_db_clone);
        drop(difflayer_clone);
        self.metrics.record_update_duration(update_start.elapsed().as_secs_f64());

        // 5. Commit the changes
        let (root_hash, node_set) = self.commit(true)?;
        self.clean();

        Ok((root_hash, Some(node_set)))
    }
}


