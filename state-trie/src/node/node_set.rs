//! Node set implementation for tracking modified trie nodes during commit operations.
//!
//! This module provides functionality for collecting and managing nodes that
//! have been modified during trie operations, enabling efficient batch commits.

use std::sync::Arc;
use std::collections::HashMap;

use alloy_primitives::B256;
use crate::encoding;

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

/// Leaf node representation
#[derive(Debug, Clone)]
struct Leaf {
    /// Raw blob of leaf
    #[allow(dead_code)]
    blob: Vec<u8>,
    /// Hash of parent node
    #[allow(dead_code)]
    parent: B256,
}

/// NodeSet contains a set of nodes collected during the commit operation.
/// Each node is keyed by path. It's not thread-safe to use.
#[derive(Clone)]
pub struct NodeSet {
    /// Owner hash (zero for account trie, account address hash for storage tries)
    pub owner: B256,
    /// Leaf nodes
    leaves: Vec<Arc<Leaf>>,
    /// Node map keyed by path
    pub nodes: HashMap<String, Arc<TrieNode>>,
    /// Count of updated and inserted nodes
    pub updates: usize,
    /// Count of deleted nodes
    pub deletes: usize,
}

impl NodeSet {
    /// Creates a new node set
    pub fn new(owner: B256) -> Self {
        Self {
            owner,
            leaves: Vec::new(),
            nodes: HashMap::new(),
            updates: 0,
            deletes: 0,
        }
    }

    /// Adds a node to the set
    pub fn add_node(&mut self, path: &[u8], node: Arc<TrieNode>) {
        let path_str = String::from_utf8_lossy(path).to_string();

        // Add the new node
        if node.is_deleted() {
            self.deletes += 1;
        } else {
            self.updates += 1;
        }

        self.nodes.insert(path_str, node);
    }

    /// Adds a leaf node to the set
    pub fn add_leaf(&mut self, parent: B256, blob: Vec<u8>) {
        self.leaves.push(Arc::new(Leaf { blob, parent }));
    }

    /// Returns the number of dirty nodes in the set
    pub fn size(&self) -> (usize, usize) {
        (self.updates, self.deletes)
    }

    /// Returns the number of collected leaf blobs
    pub fn leaf_count(&self) -> usize {
        self.leaves.len()
    }

    /// Returns a reference to the nodes map
    pub fn nodes(&self) -> &HashMap<String, Arc<TrieNode>> {
        &self.nodes
    }

    /// MergeSet merges this 'set' with 'other'. It assumes that the sets are disjoint,
    /// and thus does not deduplicate data (count deletes, dedup leaves etc).
    pub fn merge_set(&mut self, other: &NodeSet) -> Result<(), String> {
        if self.owner != other.owner {
            return Err(format!(
                "nodesets belong to different owner are not mergeable {:?}-{:?}",
                self.owner, other.owner
            ));
        }

        self.nodes.extend(other.nodes.clone());
        self.leaves.extend(other.leaves.clone());
        self.updates += other.updates;
        self.deletes += other.deletes;

        Ok(())
    }

    /// Returns true if the node set is empty
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.leaves.is_empty()
    }

    /// Clears all nodes and leaves
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.leaves.clear();
        self.updates = 0;
        self.deletes = 0;
    }

    /// Calculates a deterministic hash of the entire `NodeSet` contents.
    pub fn signature(&self) -> B256 {
        use alloy_primitives::{keccak256};

        let mut buf: Vec<u8> = Vec::new();

        // 1. owner
        buf.extend_from_slice(self.owner.as_slice());

        // 2. leaves (sorted)
        let mut leaves_sorted = self.leaves.clone();
        leaves_sorted.sort_by(|a, b| {
            let cmp_parent = a.parent.cmp(&b.parent);
            if cmp_parent == std::cmp::Ordering::Equal {
                a.blob.cmp(&b.blob)
            } else {
                cmp_parent
            }
        });

        for leaf in leaves_sorted {
            // parent
            buf.extend_from_slice(leaf.parent.as_slice());
            // blob data
            buf.extend_from_slice(&leaf.blob);
        }

        // 3. nodes (sorted by key)
        let mut nodes_sorted: Vec<(&String, &Arc<TrieNode>)> = self.nodes.iter().collect();
        nodes_sorted.sort_by(|(k1, _), (k2, _)| k1.cmp(k2));

        for (key, node) in nodes_sorted {
            // key length and bytes
            let key_bytes = key.as_bytes();
            buf.extend_from_slice(key_bytes);

            // hash field
            match node.hash {
                Some(h) => {
                    buf.push(1u8);
                    buf.extend_from_slice(h.as_slice());
                }
                None => {}
            }

            // blob field
            match &node.blob {
                Some(b) => {
                    buf.push(1u8);
                    buf.extend_from_slice(b);
                }
                None => {},
            }
        }

        // 4. updates & deletes
        buf.extend_from_slice(&(self.updates as u64).to_be_bytes());
        buf.extend_from_slice(&(self.deletes as u64).to_be_bytes());

        // 5. hash
        keccak256(&buf)
    }
}

impl std::fmt::Debug for NodeSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== NodeSet Debug Info ===")?;
        writeln!(f, "Owner: {:?}", self.owner)?;
        writeln!(f, "Total nodes: {} (updates: {}, deletes: {})", self.nodes.len(), self.updates, self.deletes)?;
        
        if !self.leaves.is_empty() {
            writeln!(f, "Leaves ({}):", self.leaves.len())?;
            for (i, leaf) in self.leaves.iter().enumerate() {
                writeln!(f, "  [{}] Parent: {:?}, Blob size: {}", i, leaf.parent, leaf.blob.len())?;
            }
        }
        
        if !self.nodes.is_empty() {
            writeln!(f, "Nodes:")?;
            let mut paths: Vec<_> = self.nodes.keys().collect();
            paths.sort();
            
            for path in paths {
                if let Some(node) = self.nodes.get(path) {
                    if node.is_deleted() {
                        writeln!(f, "  Path: {:x?} -> DELETED", path.as_bytes())?;
                    } else {
                        let hash_str = match node.hash {
                            Some(h) => format!("{:?}", h),
                            None => "None".to_string(),
                        };
                        let blob_size = node.blob.as_ref().map(|b| b.len()).unwrap_or(0);
                        writeln!(f, "  Path: {:x?} -> Hash: {}, Blob size: {}", 
                            path.as_bytes(), hash_str, blob_size)?;
                    }
                }
            }
        }
        writeln!(f, "=== End NodeSet Debug ===")
    }
}

/// Alias for difflayer node mapping
// pub type DiffLayer = HashMap<Vec<u8>, Arc<TrieNode>>;

// pub type DiffLayers = Vec<Arc<DiffLayer>>;


/// DiffLayer is a collection of updates nodes and storage roots for a given block
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

/// MergedNodeSet is a set of node sets that are merged together.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MergedNodeSet {
    pub sets: HashMap<B256, Arc<NodeSet>>,
}

impl MergedNodeSet {
    /// Create a new merged node set
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self { sets: HashMap::new() }
    }

    /// Merge a node set into the merged set
    #[allow(dead_code)]
    pub fn merge(&mut self, other: Arc<NodeSet>) -> Result<(), String> {
        if self.sets.contains_key(&other.owner) {
            panic!("repeated nodeset to merge, owner: {:?} already exists", other.owner);
        }
        self.sets.insert(other.owner, other.clone());
        Ok(())
    }

    /// Convert the merged node set to a difflayer
    pub fn to_diff_nodes(&self) -> Arc<HashMap<Vec<u8>, Arc<TrieNode>>> {
        let mut difflayer = HashMap::new();
        for (owner, set) in &self.sets {
            for (path, node) in &set.nodes {
                if owner == &B256::ZERO {
                    let key = encoding::account_trie_node_key(path.as_bytes());
                    difflayer.insert(key, node.clone());
                } else {
                    let key = encoding::storage_trie_node_key(owner.as_slice(), path.as_bytes());
                    difflayer.insert(key, node.clone());
                }
            }
        }
        Arc::new(difflayer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b256(byte: u8) -> B256 {
        B256::from([byte; 32])
    }

    fn make_node(hash_byte: u8, blob_bytes: &[u8]) -> Arc<TrieNode> {
       Arc::new(TrieNode::new(Some(b256(hash_byte)), Some(blob_bytes.to_vec())))
    }

    #[test]
    fn nodeset_add_and_size() {
        let mut set = NodeSet::new(B256::ZERO);
        assert_eq!(set.size(), (0, 0));

        set.add_node(b"abc", make_node(1, b"v1"));
        set.add_node(b"def", Arc::new(TrieNode::new(Some(B256::ZERO), Some(Vec::new())))); // deleted
        assert_eq!(set.size(), (1, 1));
        assert_eq!(set.nodes().len(), 2);
    }
}
