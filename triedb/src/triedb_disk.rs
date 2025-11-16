//! PathDB operations for TrieDB.

use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

use alloy_primitives::B256;
use rust_eth_triedb_common::{TrieDatabase, DiffLayer};

use crate::triedb::{TrieDB, TrieDBError};

/// Flush trienodes to PathDB, after commit
impl<DB> TrieDB<DB>
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    pub fn get_storage_root(&mut self, hased_address: B256) -> Result<Option<B256>, TrieDBError> {
        if let Some(dl) = self.difflayer.as_ref() {
            if let Some(root) = dl.get_storage_root(hased_address) {
                return Ok(Some(root));
            }
        }

        if let Some(root) = self.path_db.get_storage_root(hased_address)
            .map_err(|e| TrieDBError::Database(format!("Failed to get storage root: {:?}", e)))? {
            return Ok(Some(root));
        }

        if let Some(account) = self.get_account_with_hash_state(hased_address)? {
            self.updated_storage_roots.insert(hased_address, account.storage_root);
            return Ok(Some(account.storage_root));
        }

        Ok(None)
    }

    pub fn latest_persist_state(&self) -> Result<(u64, B256), TrieDBError> {
        self.path_db.latest_persist_state()
            .map_err(|e| TrieDBError::Database(format!("Failed to get latest persist state: {:?}", e)))
    }

    pub fn flush(&mut self, block_number: u64, state_root: B256, difflayer: &Option<Arc<DiffLayer>>) -> Result<(), TrieDBError> {
        let flush_start = Instant::now();

        self.path_db.commit_difflayer(block_number, state_root, difflayer)
            .map_err(|e| TrieDBError::Database(format!("Failed to commit difflayer: {:?}", e)))?;
        
        self.metrics.record_flush_duration(flush_start.elapsed().as_secs_f64());
        debug!(target: "triedb::flush", "Persisted block number: {}, state root: {:?}, duration: {:?}", block_number, state_root, flush_start.elapsed());
        Ok(())
    }

    pub fn clear_cache(&mut self) {
        self.path_db.clear_cache();
        // self.snap_db.clear_cache();
    }
}

