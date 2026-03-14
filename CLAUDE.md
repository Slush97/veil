# Veil

End-to-end encrypted, decentralized group messenger built in Rust.

## Workspace structure

| Crate | Path | Purpose |
|-------|------|---------|
| `veil-crypto` | `crates/veil-crypto` | Identity (Ed25519/X25519), group keys, key rings, device certificates |
| `veil-core` | `crates/veil-core` | Message types, sealing/verification, control messages, invites |
| `veil-net` | `crates/veil-net` | QUIC transport (quinn), peer management, relay client, mDNS discovery |
| `veil-store` | `crates/veil-store` | Encrypted local storage (redb), blob/shard erasure coding |
| `veil-relay` | `crates/veil-relay` | Relay server binary |
| `veil-client` | `veil-client` | Desktop GUI (iced) |

## Build & test

```sh
cargo build --workspace        # Build everything
cargo test --workspace         # Run all tests
cargo check -p veil-client     # Quick client typecheck
cargo run -p veil-client       # Launch desktop client
cargo run -p veil-relay        # Launch relay server
```

## Architecture

The desktop client follows the **Iced Elm architecture**: `update()` handles messages, `view()` renders UI, `subscription()` drives the async network worker. The `App` struct in `veil-client/src/ui/` is split across modules:

- `types.rs` — enums/structs (Screen, ChatMessage, ConnectionState, etc.)
- `message.rs` — Message and NetCommand enums
- `network.rs` — async network worker (P2P, relay, mDNS)
- `app.rs` — App struct, Default, helpers, subscription
- `setup.rs` — identity loading, group setup, message history
- `control.rs` — control message handling (key rotation, device certs)
- `update/` — update() dispatcher split by category
- `views/` — view methods (setup, sidebar, messages, settings)

## Key types

- `MasterIdentity` / `DeviceIdentity` — hierarchical identity (master signs devices)
- `GroupKey` / `GroupKeyRing` — symmetric group encryption with key rotation
- `SealedMessage` — encrypted+signed message envelope
- `PeerManager` — manages QUIC connections and peer events
- `RelayClient` — connects to relay for offline message delivery
- `LocalStore` — encrypted redb database for messages, blobs, settings

## Conventions

- All App fields are `pub(crate)` — accessed by `impl App` blocks across modules
- The `impl App` pattern: multiple files add methods to the same struct
- Network commands flow via `NetCommand` enum through an mpsc channel
- UI messages flow via `Message` enum (Iced's update loop)
