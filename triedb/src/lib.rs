//! Trie database library for Ethereum state management.

pub mod traits;
pub mod triedb;

// Re-export main types
pub use traits::TrieDBTrait;
pub use triedb::TrieDB;
pub use triedb::TrieDBError;
pub use triedb::EMPTY_ROOT_HASH;
