## rust-eth-triedb

High-performance TrieDB implementations for Ethereum state (MPT), fully compatible with geth’s trie behavior and node types. The project provides COW-based updates, RocksDB-backed persistence, and parallel hashing/commit/update within and across tries.

### Features

- Full compatibility with geth trie and node types
  - Node encoding/decoding and semantics (short/branch/leaf) match geth
  - RLP and hashing behavior align with Geth, interoperable with existing blocks and state roots

- Multiple backends and COW (Copy-On-Write)
  - PathDB (RocksDB): persistent, high-throughput backend with batch writes
  - Pluggable backend architecture: any storage backend implementing the `TrieDatabase` trait can be integrated
  - COW strategy between trie and backend to reduce write amplification and unnecessary copies

- Parallel hash/commit/update
  - Parallelized node hashing inside trie (rayon-based)
  - Commit phase aggregates and writes nodes in parallel
  - Updates/commits across tries/backends can be pipelined for higher throughput

- **Jemalloc support (optional)**
  - Enable jemalloc memory allocator for better performance and memory management
  - Use `--features jemalloc` to enable during compilation
  - Provides improved memory fragmentation handling and multi-threaded performance

- **ASM Keccak support (optional)**
  - Replace the default pure-Rust Keccak256 implementation with an assembly-optimized version for significant hash computation performance improvements
  - Use `--features asm-keccak` to enable during compilation
  - Leverages CPU-specific instruction sets to accelerate cryptographic hashing operations, which is critical for trie node hashing and state root calculations

### Project layout

- `common/`: shared interfaces and types (e.g., `TrieDatabase` abstraction)
- `db/pathdb/`: RocksDB-backed PathDB
- `state-trie/`: secure state trie core (key hashing, node structures, encoding/hashing, commit)
- `triedb/` external interface for managing account and storage tries
- `smoke-test/`: smoke tests comparing with geth (optional FFI dependency)

### Getting started

1) Build

```bash
# Standard build
cargo build --workspace

# Build with jemalloc support (recommended for production)
cargo build --workspace --features jemalloc,asm-keccak
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
use rust_eth_triedb::{init_global_triedb_manager, get_global_triedb, TrieDBHashedPostState};
use rust_eth_triedb_state_trie::account::StateAccount;
use alloy_primitives::{keccak256, Address, B256, U256};
use alloy_consensus::constants::KECCAK_EMPTY;
use std::str::FromStr;
use std::collections::{HashMap, HashSet};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize the global TrieDB manager with database path
    init_global_triedb_manager("/path/to/database");
    
    // 2. Get the global TrieDB instance
    let mut triedb = get_global_triedb();
    
    // 3. Prepare account data
    let address = Address::from_str("0x1234567890123456789012345678901234567890")?;
    let hashed_address = keccak256(address);
    
    // Create StateAccount with balance, nonce, and code hash
    let state_account = StateAccount::default()
        .with_balance(U256::from(1000000000000000000u128)) // 1 ETH
        .with_nonce(1)
        .with_code_hash(KECCAK_EMPTY); // or use actual code hash
    
    // 4. Prepare storage data
    let storage_key = keccak256(b"balance");
    let storage_value = U256::from(1000000000000000000u128);
    
    // Create storage states map: hashed_address -> (hashed_key -> value)
    let mut storage_states = HashMap::new();
    let mut account_storage = HashMap::new();
    account_storage.insert(storage_key, Some(storage_value));
    storage_states.insert(hashed_address, account_storage);
    
    // 5. Organize data into TrieDBHashedPostState
    let mut triedb_hashed_post_state = TrieDBHashedPostState::default();
    
    // Set account states
    triedb_hashed_post_state.states.insert(hashed_address, Some(state_account));
    
    // Set storage states
    triedb_hashed_post_state.storage_states = storage_states;
    
    // Optionally mark accounts for rebuild (if storage was wiped)
    // triedb_hashed_post_state.states_rebuild.insert(hashed_address);
    
    // 6. Commit the hashed post state
    let state_root = B256::ZERO; // or any existing state root
    let (root_hash, difflayer) = triedb.commit_hashed_post_state(
        state_root,
        None, // no previous difflayer
        &triedb_hashed_post_state
    )?;
    println!("Committed root hash: {:?}", root_hash);
    
    // 7. Flush changes to persistent storage
    let block_number = 1;
    triedb.flush(block_number, root_hash, &difflayer)?;
    println!("Flushed block {} to persistent storage", block_number);
    
    Ok(())
}
```

### License

Dual-licensed under MIT and Apache-2.0.

