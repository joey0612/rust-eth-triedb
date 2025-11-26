//! Metrics for TrieDB operations.

use reth_metrics::{
    metrics::{Histogram, Counter},
    Metrics,
};

/// Metrics for the `TrieDB`.
#[derive(Metrics, Clone)]
#[metrics(scope = "rust.eth.triedb")]
pub(crate) struct TrieDBMetrics {
    /// Histogram of hashed post state transform durations (in seconds)
    // pub(crate) hashed_post_state_transform_histogram: Histogram,
    /// Histogram of update and commit prepare durations (in seconds)
    pub(crate) update_prepare_histogram: Histogram,
    /// Histogram of update and commit durations (in seconds)
    pub(crate) update_histogram: Histogram,

    /// Histogram of hash durations (in seconds)
    pub(crate) hash_histogram: Histogram,
    /// Histogram of commit durations (in seconds)
    pub(crate) commit_histogram: Histogram,
    /// Histogram of flush durations (in seconds)
    pub(crate) flush_histogram: Histogram,

    /// Counter of get storage root from flat database
    pub(crate) get_storage_root_from_flat_counter: Counter,
    /// Counter of get storage root from trie database
    pub(crate) get_storage_root_from_trie_counter: Counter,
}

impl TrieDBMetrics {
    pub(crate) fn record_hash_duration(&self, duration: f64) {
        self.hash_histogram.record(duration);
    }

    pub(crate) fn record_commit_duration(&self, duration: f64) {
        self.commit_histogram.record(duration);
    }

    pub(crate) fn record_flush_duration(&self, duration: f64) {
        self.flush_histogram.record(duration);
    }

    // pub(crate) fn record_hashed_post_state_transform_duration(&self, duration: f64) {
    //     self.hashed_post_state_transform_histogram.record(duration);
    // }

    pub(crate) fn record_update_prepare_duration(&self, duration: f64) {
        self.update_prepare_histogram.record(duration);
    }

    pub(crate) fn record_update_duration(&self, duration: f64) {
        self.update_histogram.record(duration);
    }

    pub(crate) fn increment_get_storage_root_from_flat_counter(&self) {
        self.get_storage_root_from_flat_counter.increment(1);
    }

    pub(crate) fn increment_get_storage_root_from_trie_counter(&self) {
        self.get_storage_root_from_trie_counter.increment(1);
    }
}

