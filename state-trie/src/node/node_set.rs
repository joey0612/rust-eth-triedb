//! Node set implementation for tracking modified trie nodes during commit operations.
//!
//! This module provides functionality for collecting and managing nodes that
//! have been modified during trie operations, enabling efficient batch commits.

use alloy_primitives::B256;
use std::collections::HashMap;

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
#[derive(Debug, Clone)]
pub struct NodeSet {
    /// Owner hash (zero for account trie, account address hash for storage tries)
    pub owner: B256,
    /// Leaf nodes
    leaves: Vec<Leaf>,
    /// Node map keyed by path
    nodes: HashMap<String, TrieNode>,
    /// Count of updated and inserted nodes
    updates: usize,
    /// Count of deleted nodes
    deletes: usize,
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

    pub fn for_each_with_order(&self, f: &mut impl FnMut(String, &TrieNode)) {
        let mut paths = self.nodes.keys().collect::<Vec<_>>();
        // Bottom-up, the longest path first: reverse lexicographic order
        paths.sort_unstable_by(|a, b| b.cmp(a));
        for path in paths {
            if let Some(node) = self.nodes.get(path) {
                f(path.clone(), node);
            }
        }
    }

    /// Adds a node to the set
    pub fn add_node(&mut self, path: &[u8], node: TrieNode) {
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
        self.leaves.push(Leaf { blob, parent });
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
    pub fn nodes(&self) -> &HashMap<String, TrieNode> {
        &self.nodes
    }

    /// Returns a mutable reference to the nodes map
    pub fn nodes_mut(&mut self) -> &mut HashMap<String, TrieNode> {
        &mut self.nodes
    }

    /// Returns a set of trie nodes keyed by node hash
    pub fn hash_set(&self) -> HashMap<B256, Vec<u8>> {
        let mut ret = HashMap::new();
        for node in self.nodes.values() {
            ret.insert(node.hash.unwrap(), node.blob.clone().unwrap());
        }
        ret
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

    /// Merges another node set into this one
    pub fn merge(&mut self, owner: B256, nodes: HashMap<String, TrieNode>) -> Result<(), String> {
        if self.owner != owner {
            return Err(format!(
                "nodesets belong to different owner are not mergeable {:?}-{:?}",
                self.owner, owner
            ));
        }

        for (path, node) in &nodes {
            if let Some(prev_node) = self.nodes.get(path) {
                if prev_node.is_deleted() {
                    self.deletes -= 1;
                } else {
                    self.updates -= 1;
                }
            }
            if node.is_deleted() {
                self.deletes += 1;
            } else {
                self.updates += 1;
            }
            self.nodes.insert(path.clone(), node.clone());
        }
        return Ok(());
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
        let mut nodes_sorted: Vec<(&String, &TrieNode)> = self.nodes.iter().collect();
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

/// MergedNodeSet is a set of node sets that are merged together.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct MergedNodeSet {
    sets: HashMap<B256, NodeSet>,
}

impl MergedNodeSet {
    /// Create a new merged node set
    #[allow(dead_code)]
    pub fn new(sets: HashMap<B256, NodeSet>) -> Self {
        Self { sets }
    }

    /// Merge a node set into the merged set
    #[allow(dead_code)]
    pub fn merge(&mut self, other: &NodeSet) -> Result<(), String> {
        let subset = self.sets.get_mut(&other.owner);
        if let Some(subset) = subset {
            subset.merge(other.owner, other.nodes.clone())?;
        } else {
            self.sets.insert(other.owner, other.clone());
        }
        Ok(())
    }

    /// Flatten the merged set into a single map of nodes
    #[allow(dead_code)]
    pub fn flatten(&self) -> HashMap<B256, HashMap<String, TrieNode>> {
        let mut nodes: HashMap<B256, HashMap<String, TrieNode>> =
            HashMap::with_capacity(self.sets.len());
        for (owner, set) in &self.sets {
            nodes.insert(*owner, set.nodes.clone());
        }
        nodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b256(byte: u8) -> B256 {
        B256::from([byte; 32])
    }

    fn make_node(hash_byte: u8, blob_bytes: &[u8]) -> TrieNode {
        TrieNode::new(Some(b256(hash_byte)), Some(blob_bytes.to_vec()))
    }

    #[test]
    fn nodeset_add_and_size() {
        let mut set = NodeSet::new(B256::ZERO);
        assert_eq!(set.size(), (0, 0));

        set.add_node(b"abc", make_node(1, b"v1"));
        set.add_node(b"def", TrieNode { hash: Some(B256::ZERO), blob: Some(Vec::new()) }); // deleted
        assert_eq!(set.size(), (1, 1));
        assert_eq!(set.nodes().len(), 2);
    }

    #[test]
    fn nodeset_for_each_with_order_reverse_lex() {
        let mut set = NodeSet::new(B256::ZERO);
        set.add_node(b"abc", make_node(1, b"v1"));
        set.add_node(b"abcd", make_node(2, b"v2"));
        set.add_node(b"abb", make_node(3, b"v3"));

        let mut visited: Vec<String> = Vec::new();
        set.for_each_with_order(&mut |path, _| visited.push(path));

        // Reverse lexicographic order ensures longer prefix comes first
        assert_eq!(visited, vec![
            "abcd".to_string(),
            "abc".to_string(),
            "abb".to_string(),
        ]);
    }

    #[test]
    fn nodeset_merge_owner_mismatch_returns_err() {
        let mut set = NodeSet::new(b256(1));
        let err = set.merge(b256(2), HashMap::new()).err();
        assert!(err.is_some());
    }

    #[test]
    fn nodeset_merge_updates_counters_and_values() {
        let owner = b256(1);
        let mut set = NodeSet::new(owner);
        // initial value for k1
        set.add_node(b"k1", make_node(1, b"a"));
        assert_eq!(set.size(), (1, 0));

        let mut incoming: HashMap<String, TrieNode> = HashMap::new();
        incoming.insert("k1".to_string(), make_node(2, b"b")); // overwrite update
        incoming.insert("k2".to_string(), TrieNode { hash: Some(B256::ZERO), blob: Some(Vec::new()) }); // delete

        set.merge(owner, incoming).unwrap();

        // Overwrite cancels previous update ( -1 + 1 ), plus one delete
        assert_eq!(set.size(), (1, 1));
        let n1 = set.nodes().get("k1").unwrap();
        assert_eq!(n1.hash, Some(b256(2)));
        assert_eq!(n1.blob, Some(b"b".to_vec()));
        let n2 = set.nodes().get("k2").unwrap();
        assert!(n2.is_deleted());
    }

    #[test]
    fn merged_nodeset_merge_and_flatten() {
        let owner_a = b256(10);
        let owner_b = b256(11);

        let mut set_a = NodeSet::new(owner_a);
        set_a.add_node(b"a1", make_node(1, b"va1"));

        let mut set_b = NodeSet::new(owner_b);
        set_b.add_node(b"b1", make_node(2, b"vb1"));

        let mut merged = MergedNodeSet::new(HashMap::new());
        merged.merge(&set_a).unwrap();
        merged.merge(&set_b).unwrap();

        // Merge another set for owner_a that overwrites a1 and adds a2
        let mut set_a2 = NodeSet::new(owner_a);
        set_a2.add_node(b"a1", make_node(3, b"va1_new"));
        set_a2.add_node(b"a2", make_node(4, b"va2"));
        merged.merge(&set_a2).unwrap();

        // Flatten and validate
        let flat = merged.flatten();
        assert_eq!(flat.len(), 2);
        let a_map = flat.get(&owner_a).unwrap();
        assert_eq!(a_map.get("a1").unwrap().hash, Some(b256(3)));
        assert!(a_map.contains_key("a2"));
        let b_map = flat.get(&owner_b).unwrap();
        assert!(b_map.contains_key("b1"));
    }
}
