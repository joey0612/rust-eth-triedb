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

2) Test (includes MemoryDB and state-trie unit tests)

```bash
cargo test --workspace
```

> Note: if you enable the FFI-based comparison in `smoke-test`, make sure the required dynamic library and environment are prepared. Otherwise, you can run the core crates’ tests only.

### External API

The state trie exposes a single, unified interface for managing both the account trie and storage trie.

> TODO: Add triedb traits

### Usage examples

> TODO: Add examples for `insert/get/commit` with MemoryDB/PathDB, and sample configuration for parallel hashing.

### License

Dual-licensed under MIT and Apache-2.0.

