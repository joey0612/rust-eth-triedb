## rust-eth-triedb

High-performance Trie/DB implementations for Ethereum state (MPT), fully compatible with geth’s trie behavior and node types. The project provides COW-based updates, RocksDB-backed persistence, and parallel hashing/commit/update within and across tries.

### Features

- Full compatibility with geth trie and node types
  - Node encoding/decoding and semantics (short/branch/leaf) match geth
  - RLP and hashing behavior align with Geth, interoperable with existing blocks and state roots

- Multiple backends and COW (Copy-On-Write)
  - PathDB (RocksDB): persistent, high-throughput backend with batch writes
  - MemoryDB: in-memory backend for testing and rapid experimentation
  - COW strategy between trie and backend to reduce write amplification and unnecessary copies

- Parallel hash/commit/update
  - Parallelized node hashing inside trie (rayon-based)
  - Commit phase aggregates and writes nodes in parallel
  - Updates/commits across tries/backends can be pipelined for higher throughput

### Project layout

- `common/`: shared interfaces and types (e.g., `TrieDatabase` abstraction)
- `db/memorydb/`: in-memory database
- `db/pathdb/`: RocksDB-backed PathDB
- `state-trie/`: secure state trie core (key hashing, node structures, encoding/hashing, commit)
- `triedb/` external interface for managing account and storage tries
- `smoke-test/`: smoke tests comparing with geth (optional FFI dependency)

### Getting started

1) Build

```bash
cargo build --workspace
```

2) Smoke Test (random updates and deletes, compare the root hash with geth)

```bash
# 1. Enter project directory
cd rust-eth-triedb/smoke-test

# 2. Compile BSC library
go build -buildmode=c-shared -o libbsc_trie.dylib bsc_trie_wrapper.go

# 3. Install dynamic library
sudo cp libbsc_trie.dylib /usr/local/lib/

# 4. Return to reth root directory and run test
cd ../../..
cargo run -p reth-triedb-smoke-test
```

3) Test (includes MemoryDB and state-trie unit tests)

```bash
cargo test --workspace
```

### Usage examples

Here's a step-by-step example demonstrating the basic usage of TrieDB:

```rust
use rust_eth_triedb::triedb::{TrieDB, TrieDBTrait};
use rust_eth_triedb::db::pathdb::{PathDB, PathProviderConfig};
use rust_eth_triedb::state_trie::account::StateAccount;
use alloy_primitives::{Address, B256};
use std::str::FromStr;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize PathDB and TrieDB
    let config = PathProviderConfig::default();
    let db = PathDB::new("/path/to/database", config)?;
    let mut triedb = TrieDB::new(db);
    
    // 2. Call state_at function to set a state
    let state_root = B256::ZERO; // or any existing state root
    triedb.state_at(state_root)?;
    
    // 3. Declare a StateAccount and call update_account
    let address = Address::from_str("0x1234567890123456789012345678901234567890")?;
    let mut account = StateAccount::default();
    account.balance = 1000000000000000000u128.into(); // 1 ETH
    account.nonce = 1;
    
    triedb.update_account(address, &account)?;
    
    // 4. Update storage for this account's address
    let storage_key = b"balance";
    let storage_value = b"1000000000000000000";
    triedb.update_storage(address, storage_key, storage_value)?;
    
    // 5. Calculate hash
    let calculated_hash = triedb.calculate_hash()?;
    println!("Calculated hash: {:?}", calculated_hash);
    
    // 6. Commit changes
    let (root_hash, node_set) = triedb.commit(true)?;
    println!("Committed root hash: {:?}", root_hash);
    
    // Alternative: Use update_all to combine the above operations
    // let mut states = HashMap::new();
    // states.insert(keccak256(address.as_slice()), Some(account));
    // 
    // let mut storage_states = HashMap::new();
    // let mut storage_kvs = HashMap::new();
    // storage_kvs.insert(keccak256(storage_key), Some(storage_value.to_vec()));
    // storage_states.insert(keccak256(address.as_slice()), storage_kvs);
    // 
    // let (root_hash, node_set) = triedb.update_all(state_root, None, states, storage_states)?;
    
    Ok(())
}
```

### License

Dual-licensed under MIT and Apache-2.0.

