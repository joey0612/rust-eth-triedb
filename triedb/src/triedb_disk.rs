//! PathDB operations for TrieDB.

use std::sync::Arc;
use std::time::Instant;
use tracing::info;

use alloy_primitives::B256;
use rust_eth_triedb_common::{TrieDatabase, DiffLayer};

use crate::triedb::{TrieDB, TrieDBError};

/// Flush trienodes to PathDB, after commit
impl<DB> TrieDB<DB>
where
    DB: TrieDatabase + Clone + Send + Sync,
    DB::Error: std::fmt::Debug,
{
    pub fn latest_persist_state(&self) -> Result<(u64, B256), TrieDBError> {
        self.path_db.latest_persist_state()
            .map_err(|e| TrieDBError::Database(format!("Failed to get latest persist state: {:?}", e)))
    }

    pub fn flush(&mut self, block_number: u64, state_root: B256, update_nodes: &Option<Arc<DiffLayer>>) -> Result<(), TrieDBError> {
        let flush_start = Instant::now();

        self.path_db.commit_difflayer(block_number, state_root, update_nodes)
            .map_err(|e| TrieDBError::Database(format!("Failed to commit difflayer: {:?}", e)))?;
        
        self.metrics.record_flush_duration(flush_start.elapsed().as_secs_f64());
        info!(target: "triedb::flush", "Async persisted block number: {}, state root: {:?}, duration: {:?}", block_number, state_root, flush_start.elapsed());
        Ok(())
    }

    pub fn clear_cache(&mut self) {
        self.path_db.clear_cache();
        // self.snap_db.clear_cache();
    }
}

