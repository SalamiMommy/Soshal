# Security Policy

## Reporting a Vulnerability

Report vulnerabilities to dev@soshal.app. Do not file public GitHub issues for security bugs.

We aim to acknowledge receipt within 48 hours and provide a fix timeline within 5 business days.

## Scope

- Rust backend: key handling (nsec, PQC, NIP-44 v2), data at rest, relay
  data validation, FFI boundary
- FFI bridge (`flutter-bridge`): marshaling, signer locking, keychain
  operations (pubkeys only across FFI)
- SQLite storage (bundled libsql (Turso), WAL, trusted_schema=OFF,
  secure_delete=ON, transactional migrations)
- Dependency supply chain (Rust + Dart)

Out of scope: legacy Tauri 2.0 / Dioxus WASM client (deleted 2026-08) — no
webview, no Tauri IPC surface exists anymore.

## Audit Status

| Check | Status |
|-------|--------|
| cargo audit | pre-push + CI gate (`cargo audit`) |
| crate purity | `scripts/check-core-compliance.sh` in CI (no tauri/flutter/ndk in cores) |
| Deprecated Dioxus path | code removed; compliance script blocks `dioxus`, `tauri`, `soshal-ui` deps |
| Rust memory safety | `unsafe_code = "deny"` workspace-wide |

## Supported Versions

Latest release only.

## Residual Risks & Accepted Decisions

Audited 2026-08 (community security review). No known CRITICAL. Accepted residuals:

- **WoT sybil resistance** — `identity-core/src/wot.rs` trust scores use unverified contact graph data; sybil swarms can raise scores only within hard caps (bounded, by design).
- **NIP-44 version byte** — `crypto-core/src/nip44.rs` version byte sits outside the AEAD (spec-compliant); tampering yields garbage plaintext, not disclosure.
- **LAN beacon** — opt-in via `lan_beacon_start`; MAC key is derived from the at-rest key (any device of the same identity can verify peers). LAN sync transport is encrypted (ephemeral X25519 + ChaCha20-Poly1305); the bearer token gates connect but is transmitted in the plaintext handshake.
- **Group keys** — `group_keys.rs` verifies the distributor is a current member at fetch time; removed members retain the shared key until an admin re-keys (NIP-29 limitation).
- **PIN lockout** — lockout state lives in the local DB and uses device clock; a local attacker with write access to the DB can clear the lockout counter. At-rest encryption + keychain still protect secrets.
- **At-rest scope** — at-rest v2 seals key material only; user posts/messages in SQLite are not encrypted at rest.
- **Signer lock** — in-process signer wipes key material on lock; unlock goes through the OS keychain (pubkey-pinned) or recovery phrase. Keychain absence degrades gracefully.
- **Reticulum Mesh Validation** — `network-core/src/reticulum/packet.rs` validates binary wire headers, destination hash lengths, and caps hop counts to prevent mesh routing loops or malformed packet injection.
- **rate_limit_check_json** — client-supplied rate-limit state in `network-core/src/rate_limit.rs` is a UI throttle, not a security gate (PIN lockout uses server-side DB state).