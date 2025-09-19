// ! State trie implementation for secure trie operations.

use alloy_primitives::{Address, B256, U256, keccak256};
use std::{sync::Arc};

use alloy_rlp::{Encodable, Decodable};
use rust_eth_triedb_common::TrieDatabase;

use super::account::StateAccount;
use super::secure_trie::{SecureTrieId, SecureTrieError};
use super::traits::SecureTrieTrait;
use super::trie::Trie;
use super::node::{NodeSet, DiffLayers};
use super::node::rlp_raw;

/// State trie implementation that wraps a trie with secure key hashing
#[derive(Clone)]
pub struct StateTrie<DB> {
    trie: Trie<DB>,
    id: SecureTrieId,
}

impl<DB> std::fmt::Debug for StateTrie<DB>
where
    DB: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateTrie")
            // .field("trie", &self.trie)
            .field("id", &self.id)
            .finish()
    }
}

impl<DB> StateTrie<DB>
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    /// Creates a new state trie with the given identifier and database
    pub fn new(id: SecureTrieId, database: DB, difflayer: Option<&DiffLayers>) -> Result<Self, SecureTrieError> {
        let trie = Trie::new(&id, database, difflayer)?;
        Ok(Self { trie, id })
    }

    /// Returns the identifier of this state trie
    pub fn id(&self) -> &SecureTrieId {
        &self.id
    }

    /// Returns a reference to the underlying trie
    pub fn trie(&self) -> &Trie<DB> {
        &self.trie
    }

    /// Returns a mutable reference to the underlying trie
    pub fn trie_mut(&mut self) -> &mut Trie<DB> {
        &mut self.trie
    }

    /// Creates a copy of this state trie
    pub fn copy(&self) -> Self {
        Self {
            trie: self.trie.clone(),
            id: self.id.clone(),
        }
    }

    /// Hashes a key using keccak256
    pub fn hash_key(&self, key: &[u8]) -> B256 {
        keccak256(key)
    }
}

impl<DB> SecureTrieTrait for StateTrie<DB>
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    type Error = SecureTrieError;

    fn id(&self) -> &SecureTrieId {
        &self.id
    }

    fn get_account(&mut self, address: Address) -> Result<Option<StateAccount>, Self::Error> {
        let hashed_address = self.hash_key(address.as_slice());
        if let Some(data) = self.trie.get(hashed_address.as_slice())? {
            let account = StateAccount::decode(&mut &data[..])
                .map_err(|_| SecureTrieError::InvalidAccount)?;
            Ok(Some(account))
        } else {
            Ok(None)
        }
    }

    fn update_account(&mut self, address: Address, account: &StateAccount) -> Result<(), Self::Error> {
        let hashed_address = self.hash_key(address.as_slice());
        let mut encoded_account = Vec::new();
        account.encode(&mut encoded_account);
        self.trie.update(hashed_address.as_slice(), &encoded_account)?;
        Ok(())
    }

    fn delete_account(&mut self, address: Address) -> Result<(), Self::Error> {
        let hashed_address = self.hash_key(address.as_slice());
        self.trie.delete(hashed_address.as_slice())?;
        Ok(())
    }

    fn get_storage(&mut self, _address: Address, key: &[u8]) -> Result<Option<Vec<u8>>, Self::Error> {
        let hashed_key = self.hash_key(key);
        let enc = self.trie.get(hashed_key.as_slice())?;

        if enc.is_none() {
            return Ok(None);
        }

        let enc = enc.unwrap();
        if enc.is_empty() {
            return Ok(None);
        }

        // Extract the RLP string/content. Map any raw-RLP error to our domain error.
        let (_, value, _) = rlp_raw::split(&enc).map_err(|_| SecureTrieError::InvalidStorage)?;
        Ok(Some(value.to_vec()))
    }

    fn update_storage(&mut self, _address: Address, key: &[u8], value: &[u8]) -> Result<(), Self::Error> {
        let hashed_key = self.hash_key(key);
        let encoded_value = alloy_rlp::encode(value);
        self.trie.update(hashed_key.as_slice(), &encoded_value)?;
        Ok(())
    }

    fn delete_storage(&mut self, _address: Address, key: &[u8]) -> Result<(), Self::Error> {
        let hashed_key = self.hash_key(key);
        self.trie.delete(hashed_key.as_slice())?;
        Ok(())
    }

    fn get_account_with_hash_state(&mut self, hashed_address: B256) -> Result<Option<StateAccount>, Self::Error> {
        if let Some(data) = self.trie.get(hashed_address.as_slice())? {
            let account = StateAccount::decode(&mut &data[..])
                .map_err(|_| SecureTrieError::InvalidAccount)?;
            Ok(Some(account))
        } else {
            Ok(None)
        }
    }

    fn update_account_with_hash_state(&mut self, hashed_address: B256, account: &StateAccount) -> Result<(), Self::Error> {
        let mut encoded_account = Vec::new();
        account.encode(&mut encoded_account);
        self.trie.update(hashed_address.as_slice(), &encoded_account)?;
        Ok(())
    }

    fn delete_account_with_hash_state(&mut self, hashed_address: B256) -> Result<(), Self::Error> {
        self.trie.delete(hashed_address.as_slice())?;
        Ok(())
    }

    fn get_storage_with_hash_state(&mut self, _: B256, hashed_key: B256) -> Result<Option<Vec<u8>>, Self::Error> {
        let enc = self.trie.get(hashed_key.as_slice())?;

        if enc.is_none() {
            return Ok(None);
        }

        let enc = enc.unwrap();
        if enc.is_empty() {
            return Ok(None);
        }

        // Extract the RLP string/content. Map any raw-RLP error to our domain error.
        let (_, value, _) = rlp_raw::split(&enc).map_err(|_| SecureTrieError::InvalidStorage)?;
        Ok(Some(value.to_vec()))
    }

    fn update_storage_with_hash_state(&mut self, _: B256, hashed_key: B256, value: &[u8]) -> Result<(), Self::Error> {
        let encoded_value = alloy_rlp::encode(value);
        self.trie.update(hashed_key.as_slice(), &encoded_value)?;
        Ok(())
    }

    fn update_storage_u256_with_hash_state(&mut self, _: B256, hashed_key: B256, value: U256) -> Result<(), Self::Error> {
        let encoded_value = alloy_rlp::encode(&value);
        self.trie.update(hashed_key.as_slice(), &encoded_value)?;
        Ok(())
    }

    fn get_storage_u256_with_hash_state(&mut self, _: B256, hashed_key: B256) -> Result<Option<U256>, Self::Error> {
        let enc = self.trie.get(hashed_key.as_slice())?;

        if enc.is_none() {
            return Ok(None);
        }

        let enc = enc.unwrap();
        if enc.is_empty() {
            return Ok(None);
        }

        // Decode the U256 value from RLP
        let value_dec = U256::decode(&mut &enc[..]).map_err(|_| SecureTrieError::InvalidStorage)?;
        Ok(Some(value_dec))
    }
    
    fn delete_storage_with_hash_state(&mut self, _: B256, hashed_key: B256) -> Result<(), Self::Error> {
        self.trie.delete(hashed_key.as_slice())?;
        Ok(())
    }

    fn hash(&mut self) -> B256 {
        self.trie.hash()
    }

    fn commit(&mut self, _collect_leaf: bool) -> Result<(B256, Option<Arc<NodeSet>>), Self::Error> {
        self.trie.commit(_collect_leaf)
    }
}

/// Type alias for secure trie
pub type SecureTrie<DB> = StateTrie<DB>;
