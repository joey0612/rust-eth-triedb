//! PathDB operations for TrieDB.

use std::sync::Arc;
use std::time::Instant;

use alloy_primitives::B256;
use rust_eth_triedb_common::{TrieDatabase, TrieDatabaseBatch};
use rust_eth_triedb_state_trie::node::DiffLayer;
use rust_eth_triedb_state_trie::encoding::{TRIE_STATE_ROOT_KEY, TRIE_STATE_BLOCK_NUMBER_KEY};

use crate::triedb::{TrieDB, TrieDBError};

/// Flush trienodes to PathDB, after commit
impl<DB> TrieDB<DB>
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    pub fn latest_persist_state(&self) -> Result<(u64, B256), TrieDBError> {
        let block_number_bytes = self.path_db.get(TRIE_STATE_BLOCK_NUMBER_KEY)
            .map_err(|e| TrieDBError::Database(format!("Failed to get block number: {:?}", e)))?
            .ok_or_else(|| TrieDBError::Database("Block number not found".to_string()))?;
        let state_root_bytes = self.path_db.get(TRIE_STATE_ROOT_KEY)
            .map_err(|e| TrieDBError::Database(format!("Failed to get state root: {:?}", e)))?
            .ok_or_else(|| TrieDBError::Database("State root not found".to_string()))?;
        
        // Convert Vec<u8> back to u64 (little-endian)
        let block_number = u64::from_le_bytes(
            block_number_bytes.try_into()
                .map_err(|_| TrieDBError::Database("Invalid block number bytes length".to_string()))?
        );
        
        // Convert Vec<u8> back to B256
        let state_root = B256::from_slice(&state_root_bytes);
        
        Ok((block_number, state_root))
    }

    pub fn flush(&mut self, block_number: u64, state_root: B256, update_nodes: &Option<Arc<DiffLayer>>) -> Result<(), TrieDBError> {
        let flush_start = Instant::now();

        self.flush_trie_nodes(block_number, state_root, update_nodes)?;
        self.metrics.record_flush_duration(flush_start.elapsed().as_secs_f64());
        Ok(())
    }


    pub fn flush_trie_nodes(&mut self, block_number: u64, state_root: B256, update_nodes: &Option<Arc<DiffLayer>>) -> Result<(), TrieDBError> {
        
        let mut batch = self.path_db.create_batch().unwrap();
        
        batch.insert(TRIE_STATE_ROOT_KEY, state_root.as_slice().to_vec()).unwrap();
        batch.insert(TRIE_STATE_BLOCK_NUMBER_KEY, block_number.to_le_bytes().to_vec()).unwrap();
        
        if let Some(difflayer) = update_nodes {
            for (key, node) in difflayer.as_ref() {
                if node.is_deleted() {
                    batch.delete(key).unwrap();
                } else {
                    batch.insert(key, node.blob.as_ref().unwrap().clone()).unwrap();
                }
            }
        }

        self.path_db.batch_commit(batch).map_err(|e| TrieDBError::Database(format!("Failed to commit batch: {:?}, block number: {}", e, block_number)))?;

        Ok(())
    }

    pub fn clear_cache(&mut self) {
        // self.storage_root_cache.write().unwrap().clear();
        self.path_db.clear_cache();
    }
}

