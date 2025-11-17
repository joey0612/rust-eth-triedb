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
use rust_eth_triedb::{init_global_manager, get_global_triedb};
use reth_trie_common::{HashedPostState, HashedStorage};
use reth_primitives_traits::Account;
use alloy_primitives::{keccak256, Address, B256, U256};
use std::str::FromStr;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize the global TrieDB manager with database path
    init_global_manager("/path/to/database");
    
    // 2. Get the global TrieDB instance
    let mut triedb = get_global_triedb();
    
    // 3. Prepare account data
    let address = Address::from_str("0x1234567890123456789012345678901234567890")?;
    let hashed_address = keccak256(address);
    
    // Create account with balance and nonce
    let account = Account {
        balance: U256::from(1000000000000000000u128), // 1 ETH
        nonce: 1,
        bytecode_hash: Some(B256::ZERO),
    };
    
    // 4. Prepare storage data
    let storage_key = keccak256(b"balance");
    let storage_value = U256::from(1000000000000000000u128);
    
    // Create hashed storage with storage entries
    let hashed_storage = HashedStorage::from_iter(
        false, // wiped = false
        vec![(storage_key, storage_value)]
    );
    
    // 5. Organize data into HashedPostState
    let hashed_post_state = HashedPostState::default()
        .with_accounts(vec![(hashed_address, Some(account))])
        .with_storages(vec![(hashed_address, hashed_storage)]);
    
    // 6. Commit the hashed post state
    let state_root = B256::ZERO; // or any existing state root
    let (root_hash, difflayer) = triedb.commit_hashed_post_state(
        state_root,
        None, // no previous difflayer
        &hashed_post_state
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

