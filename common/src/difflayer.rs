//! DiffLayer types for tracking trie node changes.
//!
//! This module provides types for representing trie nodes and diff layers
//! used in tracking modifications during trie operations.

use std::sync::Arc;
use std::collections::HashMap;
use alloy_primitives::B256;

// Trie state storage keys
pub const TRIE_STATE_ROOT_KEY: &[u8] = b"state_root";
pub const TRIE_STATE_BLOCK_NUMBER_KEY: &[u8] = b"block_number";

/// Represents a trie node with its hash and encoded data
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrieNode {
    /// Node hash, empty for deleted node
    pub hash: Option<B256>,
    /// Encoded node data, empty for deleted node
    pub blob: Option<Vec<u8>>,
}

impl TrieNode {
    /// Creates a new trie node
    pub fn new(hash: Option<B256>, blob: Option<Vec<u8>>) -> Self {
        Self { hash, blob }
    }

    /// Creates a default trie node
    pub fn default() -> Self {
        Self { hash: None, blob: None }
    }

    /// Returns true if this node is marked as deleted
    pub fn is_deleted(&self) -> bool {
        self.blob.is_none() || (self.blob.is_some() && self.blob.as_ref().unwrap().is_empty())
    }

    /// Returns the total memory size used by this node
    pub fn size(&self) -> usize {
        if self.is_deleted() {
            return 0;
        }
        self.blob.as_ref().unwrap().len() + 32 // 32 bytes for hash
    }
}

/// Represents a leaf node with its blob and parent hash
#[derive(Debug, Clone)]
pub struct Leaf {
    /// Raw blob of leaf
    #[allow(dead_code)]
    pub blob: Vec<u8>,
    /// Hash of parent node
    #[allow(dead_code)]
    pub parent: B256,
}


/// DiffLayer is a collection of updated trie nodes and storage roots for a special block
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffLayer {
    pub diff_nodes: HashMap<Vec<u8>, Arc<TrieNode>>,
    pub diff_storage_roots: HashMap<B256, B256>,
}

impl DiffLayer {
    /// Create a new diff layer
    pub fn new(diff_nodes: HashMap<Vec<u8>, Arc<TrieNode>>, diff_storage_roots: HashMap<B256, B256>) -> Self {
        Self { diff_nodes, diff_storage_roots }
    }

    /// Get a trie node by prefix
    pub fn get_trie_nodes(&self, prefix: Vec<u8>) -> Option<Arc<TrieNode>> {
        self.diff_nodes.get(&prefix).map(|node: &Arc<TrieNode>| node.clone())
    }

    /// Get a storage root by hased address
    pub fn get_storage_roots(&self, hased_address: B256) -> Option<B256> {
        self.diff_storage_roots.get(&hased_address).map(|root| *root)
    }
}

/// DiffLayers is a collection of diff layers for uncommitted blocks
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffLayers {
    pub diff_layers: Vec<Arc<DiffLayer>>,
}

impl DiffLayers {
    /// Insert a diff layer into the collection
    pub fn insert_difflayer(&mut self, difflayer: Arc<DiffLayer>) {
        self.diff_layers.push(difflayer);
    }

    /// Get a trie node by prefix
    pub fn get_trie_nodes(&self, prefix: Vec<u8>) -> Option<Arc<TrieNode>> {
        for difflayer in &self.diff_layers {
            if let Some(node) = difflayer.get_trie_nodes(prefix.clone()) {
                return Some(node);
            }
        }
        None
    }

    /// Get a storage root by hased address
    pub fn get_storage_roots(&self, hased_address: B256) -> Option<B256> {
        for difflayer in &self.diff_layers {
            if let Some(root) = difflayer.get_storage_roots(hased_address) {
                return Some(root);
            }
        }
        None
    }

    /// Returns true if the diff layers are empty
    pub fn is_empty(&self) -> bool {
        self.diff_layers.is_empty()
    }
}

