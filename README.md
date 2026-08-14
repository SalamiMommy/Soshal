# Soshal

A decentralized social media application combining the best of Facebook and MySpace, built on Freenet, Nostr, I2P, and Reticulum Mesh. Features dating, feed, events, messenger, marketplace, and customizable profiles — all running on a peer-to-peer foundation.

## Features

- **Social Feed** — Post text, images, videos, and GIFs. Sort by recency or trending (reactions).
- **Dating** — Swipe-based discovery with compatibility scoring (age, height, body type, interests, smoking, drinking). Prioritizes mutual likes and close matches.
- **Events** — Create and RSVP to events (NIP-52).
- **Marketplace** — Buy/sell listings with images, prices, and conditions (NIP-15).
- **Messenger** — End-to-end encrypted direct messages (NIP-44).
- **Custom Profiles** — MySoshal-style drag-and-drop profile builder with widgets: text, media gallery, music player, friend grid, contact card, Q&A, containers, and theme selection.
- **Theme Customization** — Customize accent color, background darkness, font family, and font size.
- **Privacy Controls** — Three levels: Public, Friends-Only, and Stealth (invisible to non-whitelisted users).
- **Web of Trust** — Friend-of-friend distance calculation and contact discovery.
- **Reticulum Mesh Support** — Off-grid, peer-to-peer mesh networking support with identity discovery (`ANNOUNCE`), 128-bit destination addressing, packet routing, and event sync over UDP, Multicast, and RNode serial interfaces.
- **Data Hosting** — Optionally host friends' content at configurable depth (1 or 2 hops).
- **Lightning Zaps** — Send sats via LNURL or Nostr Wallet Connect (NWC).
- **Multi-Account** — Create, import, and switch between multiple Nostr identities.
- **Security** — Biometric lock (Face ID / fingerprint) and PIN code protection.
- **I2P & Freenet Support** — Automatic I2P proxy detection, relay routing, and Freenet decentralized storage.

## Tech Stack

| Layer | Technology |
|-------|-----------|
| UI Framework | Flutter (Dart) — the only client |
| App Backend | Rust via `flutter-bridge` FFI adapter (auto-generated `frb_generated.dart` bindings) |
| Core Logic & Domain | Rust Native Workspace (27 crates: `crypto-core`, `db-core`, `nostr-core`, `network-core`, `pqc-core`, etc.) |
| Database | SQLite (Turso `libsql`) with FTS5 (`db-core`) |
| Nostr & Cryptography | `nostr-sdk`, `ring`, `pqc-core` post-quantum crypto (ML-KEM-768 / ML-DSA-65) |
| Mesh Networking | Native Reticulum Mesh Stack (`soshal-network-core::reticulum`) |
| Mobile Support | Android (multi-ABI) + iOS |

## Supported Nostr NIPs

- NIP-01 — Basic protocol
- NIP-02 — Contact List
- NIP-05 — DNS Verification
- NIP-06 — Basic key derivation
- NIP-07 — window.nostr extension API
- NIP-09 — Event Deletion
- NIP-15 — Marketplace (NIP-15 listings)
- NIP-18 — Reposts
- NIP-19 — Bech32 encoding
- NIP-25 — Reactions
- NIP-29 — Group messaging & channels
- NIP-44 — Encrypted DMs
- NIP-52 — Calendar Events
- NIP-57 — Lightning Zaps
- NIP-65 — Relay List Metadata
- NIP-78 — Custom Application Data
- NIP-95/96 — Blossom File Storage (upload server)
- NIP-30080 — Live streaming (video/audio)
- NIP-30081 — Live streaming chat

## Getting Started

### Prerequisites

- Rust (latest stable toolchain)
- Flutter SDK (3.x) with Android toolchain (SDK + NDK 27.x for bridge `.so` builds)
- **Linux**: GStreamer libraries for media playback (optional, for advanced media features):
  - Ubuntu/Debian: `sudo apt install libgstreamer1.0-dev gstreamer1.0-plugins-base-apps gstreamer1.0-plugins-good`
  - Fedora: `sudo dnf install gstreamer1-devel gstreamer1-plugins-base`
  - Arch: `sudo pacman -S gst-plugins-base gst-plugins-good`

### Optional Dependencies

- **i2pd**: I2P router for anonymous networking (install via system package manager; status surfaces in Settings → Network)
- **freenet**: Freenet node for decentralized storage (install manually)
- **rnsd**: Reticulum mesh network daemon (optional — run `scripts/build-reticulum.sh` to build)

### Build & Run Commands

```bash
# Type-check all workspace Rust crates (27 cores + flutter-bridge)
cargo check --workspace

# Run all Rust unit tests
cargo test --workspace

# Flutter app
cd soshal_flutter && flutter pub get && flutter build apk
cd ..

# Build Android bridge .so per ABI (recipe in AGENTS.md), then bundle via
# flutter build apk (jniLibs already wired)
```

## Architecture

```
Soshal/
├── soshal_flutter/         # Flutter UI + services + routes (the only client)
├── flutter-bridge/         # FFI adapter: 30 modules calling into *-core
├── crypto-core/            # ring sha256/hmac/hkdf, NIP-44 & PQC
├── db-core/                # SQLite (libsql) migrations & repositories
├── nostr-core/             # Nostr keys, events, relay engine (nostr-sdk)
├── media-core/             # Blossom media client & session management
├── content-core/           # Hashtag/mention/URL parsing & compression
├── identity-core/          # Web of Trust, NIP-05, key & seed management
├── network-core/           # Relay health, outbox ranking, Reticulum mesh stack & BLE sync
├── storage-core/           # Audio waveform & cache eviction
├── feed-core/              # Post ranking & feed algorithms
├── groups-core/            # NIP-29 membership & channels
├── messaging-core/         # NIP-44 encrypted messaging
├── social-core/            # Compatibility & social scoring
├── events-core/            # Check-in & NIP-52 calendar logic
├── marketplace-core/       # NIP-15 listing & escrow handling
├── dating-core/            # Compatibility scoring & filtering
├── search-core/            # FTS5 search & event→result mapping
├── notification-core/      # Notification filtering
├── moderation-core/        # Content moderation
├── analytics-core/         # Analytics & audit helpers
├── webrtc-core/            # ICE/WebRTC helpers
├── spatial-core/           # Spatial helpers
├── pqc-core/               # Post-quantum crypto (KEM, DSA) & Freenet identity
├── scripts/                # Tooling (check-core-compliance.sh, build-reticulum.sh)
└── Cargo.toml              # Cargo workspace definition (28 members)
```
```

## Privacy

Soshal is designed with privacy by default:

- **Public** — All content broadcast to relays normally
- **Friends** — Content tagged with friend pubkeys; relays filter by contact list
- **Stealth** — Only visible to explicitly whitelisted pubkeys; uses separate relay sets

Users control their own keys, data, and which relays they connect to.

## License

MIT

