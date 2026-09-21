# Soshal Workspace Audit Ledger

Finding ledger for the periodic full-workspace bug sweeps. Each round lists
what was found, what was fixed (with file refs), and what was examined and
cleared. Rounds are cumulative; last-known-good state is shaded green.

## Round 9 — 2026-09-21: transport/scheduler/native-surface post-sweep

Scope: 3 weeks of fix batches (Sept 7-21, ~138 commits) landed since round 8 —
docs/annotations exercise re-verified the workspace against source.

### Fixed (this round, verified against git log + source)
- **First-contact QUIC certs** (`network-core/quic.rs`, `sync-core/ingest.rs`,
  commit `b52ab28`) — peers presenting self-signed certs on first contact are
  accepted for mesh; no `HybridCertVerifier` regressions. Batch text-note
  ingest now notifies (was silent).
- **Scheduled publish engine** (`db-core` + `sync-core` + `flutter-bridge/src/ffi/scheduled.rs`,
  commits `6aa7d85`, `14a0665`, `8899d24`) — scheduled-post isolation, FK
  robustness, publication order, same-batch notify, upsert drift, and the
  blocked-draft loop now continues instead of stalling (`98ff033`).
- **Notification producer** (`db-core`, `feed-core`, `flutter-bridge`) —
  notifications keyed by event id; nested reply counts corrected.
- **TLS CVE bump** (`1e1d45c`) — rustls upgraded (too-early-data family);
  broken Android XML fixed; dead code cleaned.
- **Signer/session/P2P hardening** (`1dcf99b`, `4fadf8d`, `740726c`,
  `e5531fa`) — signer rate limits, session/symlink integrity, p2p transport
  races, at-rest/NIP-44 key zeroization, dating unmatch/block filtering,
  group roles/presence reactivity, moq replay watermark, pqc bounds,
  reticulum transport lifecycle.
- **Flutter lifecycle + honest surfaces** (`db9374c`, `53c9a45`) —
  lifecycle restarts, real UI feedback, honest-fied fake surfaces, dating
  deck skip, p2p lifecycle restart.
- **Bundled daemons** (2026-09-10/11) — freenet/i2pd/rnsd boot on Android &
  desktop (see `DAEMON_BUNDLING.md`); `permissions.rs`/`power.rs` live
  states re-verified.
- Audit batches 5-20 (2026-09-15..21): sync/search/relay/streaming/audio/
  mesh/zk, social/vouch/webrtc/zap/guestbook/chatrandom, logic/
  authorization/case-sensitivity/input-bounds sweeps.

### Cleared (verified against source this round)
- All "stub/gated" surfaces re-audited against `flutter-bridge/src/ffi/*.rs`:
  `zap_fetch_invoice`/`zap_send_payment` (NIP-47 NWC), friend
  suggestions/requests, `minis_fetch`, dating reactions, scheduled publishing,
  analytics, notifications are REAL. Still gated (honest UI): FCM push,
  TURN provisioning, WebRTC media transport, WASI wasm host, ZK prover,
  FROST, eBPF kernel modes, freenet seednode announce, raster no-op.
- rusqlite: zero references in code (libsql `0.10.0-pre.4` throughout);
  `check-core-compliance.sh` + `guard-lib-platform.sh` still enforce the
  purity/platform-fact boundaries.
- FFI drift re-check: `crateFfi*` tokens resolve to live Rust `pub fn`s; the
  known-removed surfaces (`streaming_start_local_server`,
  `streaming_get_video_url`, `streaming_stop_local_server`,
  `moderation_hybrid_classify_media`, `search_index_post`) remain absent as
  documented. `network_*` relay fns (`subscribe`/`publish_event`/`query_events`/
  `init_relays`/…) are now live async fns — doc's stale-codegen trap list
  updated accordingly.
- Docs/annotations: webview/Tauri-era doc comments across cores rewritten
  ("webview JSON" → FFI/wire JSON); README/QUICK_START/CONTRIBUTING/
  ARCHITECTURE/FFI_BRIDGE_GUIDE/DAEMON_BUNDLING counts + gated lists + build
  facts refreshed to 32 cores / 55 ffi modules / 34 members.

### Verified
- Full workspace suite green at end of round: `cargo test --workspace`
  (2507 passed, 0 failed across 136 test binaries). Commit: `53c9a45`.

## Round 8 — 2026-09-05: sync-engine liveness (Rust truth + Dart watchdog)

Finding: the background sync engine could die silently. `flutter-bridge/src/ffi/sync.rs::STOP`
(a `static Mutex<Option<Arc<AtomicBool>>>`) was populated at `sync_start` but never cleared
when the engine thread exited for a non-stop reason (tokio runtime init failure,
`build_client`/`subscribe` error, or relay stream end `Ok(None)`). `sync_running()` therefore
stayed `true` forever; Dart `SyncService._started` stayed true and `start()` is idempotent;
no watchdog existed → after any relay failure the app ran to restart with background
feed/DM ingestion + outbox replay dead while the UI reported "running".

### Fixed
- `sync-core/src/engine.rs` — `spawn_engine` now takes an `on_exit` callback invoked on the
  engine thread after the loop terminates for any reason (incl. runtime-init failure).
- `flutter-bridge/src/ffi/sync.rs` — `sync_start` installs the STOP Arc BEFORE spawning (so a
  fast-failing thread can clear it) and passes an `on_exit` that removes the static via
  `Arc::ptr_eq` compare-and-clear (only clears if the static still holds this generation, so a
  concurrent restart is never clobbered). `sync_running()` now reflects genuine engine liveness.
- `soshal_flutter/lib/services/sync_service.dart` — 60 s watchdog (`_engineWatchdog`): when the
  Rust engine self-clears, it tears down the local stream/timers and restarts with the relays
  captured at start (`_relays`, cleared on `stop()`/sign-out so the watchdog never fights
  sign-out). Reuses the `_gcTimer` pattern; no notify storms (1 check/min, only on failure).
- `sync-core/tests/engine_gap_tests.rs` — 4-arg `spawn_engine` call updated (`|| {}`).

### Cleared (verified against source this round)
- FTS5 (search-core/fts5.rs) + user search (db-core/repos/user.rs) — terms are
  alphanumeric-only after stripping, so quotes/`AND`/`OR`/`NEAR`/`*` cannot be smuggled into
  the MATCH parser; field names filtered identically. `settings get_many` placeholders built
  from caller-fixed key lists only.
- Marketplace price/amount → u64: `price.max(0.0) as u64` + saturating float casts (negative →
  0, NaN → 0, ±Inf saturates); publish gate rejects `price == 0`. Infinity price is a cosmetic
  watch, not a defect.
- serde_json `Value` indexing `unwrap()`s (`groups-core/group_enc/seal.rs:77`,
  `flutter-bridge/src/ffi/*.rs`) — all inside `#[cfg(test)]` blocks.
- Production `.expect()` sites — compile-time-constant regex/parametrized invariants only
  (`db-core` conn-present, const regex set, HMAC-any-key); `groups-core/access.rs` "hash
  success" is test-only.
- Round-7 watermark clamp completeness — the production watermark advance already skew-caps in
  `engine.rs` (`capped = created.min(now + WATERMARK_SKEW_SECS)`, applied to feed/DM/meta);
  round-7's `ingest.rs` edits capped the storage layer + test parity. No residual future-date jam.
- Scheduled posts — fire-time `created_at` is `≤ now` by construction (futurity gate at
  scheduled.rs), so the ingest clamp is a no-op for them.
- Cross-account bleed — `session_service._resetAccountScopedServices` + per-service cache clears
  (`messaging_service`, `feed_service` pinned/reactions) exist on account switch.
- Watch-only: session `accounts` Vec (no cap, user-driven), local notifications table (no
  retention), marketplace infinity price — all low-risk, no change.

### Verified
- Gate green at end of round: fmt / clippy `-D warnings` / workspace tests ×3 (2270) /
  `flutter analyze` 0 issues. Commit: `9069c4a`.

## Round 7 — 2026-09-05: hardening follow-up (parallel builders)

Scope: 6 passes — (P1) numeric-truncation across 84 non-test `as` casts, (P2)
security invariants (SSRF, verified-events, zap amounts, timestamp sanity),
(P3) unbounded-growth, (P4) Dart async/silent-swallow, (P5) hostile-parse +
untyped jsonDecode, (P6) clock/time. Fixes applied via parallel
cavecrew-builders; full gate run at the very end. Commit: `1db740d`.

### Fixed (this round)

- **Relay timestamp clamp (`sync-core/src/ingest.rs`)** — P2/P6. Relay
  `created_at` (u64) flowed unchecked into `i64` DB rows AND into the sync
  watermark. Feed/paging SQL is bare `ORDER BY created_at DESC` (no `<= now`
  bound), so a hostile future-dated event pinned its author to the top of the
  feed; worse, the watermark advance took `max(watermark, ts)` — one
  now+10-year event would skip every subsequent legit event. Added
  `sanitize_ts`/`sanitize_ts_u64` (clamp to `[0, now + 300s]`) and applied at
  all 14 write sites (posts, users, zaps, reactions, reposts, bookmarks,
  channel DMs, watermark, watermark-advance loop). Future-dated/negative
  timestamps now land at `now+grace`/0, preserving store ordering for legit
  events (local publishes are already `≤ now`, so the clamp is a no-op there).
- `p2p_service.dart` — `_pollInFlight` re-entrancy guard: the async
  `Timer.periodic` body repled its own `_poll` (unawaited in `run`)
  producing overlapping batches; a `_pollInFlight` early-return now
  serializes, reset in `stopPolling`.
- `profile_renderer_screen.dart` (762, 975) + `profile_builder_screen.dart`
  (54, 192) — untyped `jsonDecode` → `.whereType<Map<String,dynamic>>` /
  shape guards (site 59 already try/caught; inner per-profile `catch (_) {}`
  is a deliberate best-effort skip, left as-is).
- `audio-core/src/voice.rs:131` — `pcm_len_for_secs` now rejects
  non-finite/negative/overflowing f64 durations before length math
  (encode-path panic + absurd buffer guard).

### Cleared (defense-in-depth verified against source)

- **SSRF**: `network-core/src/http3_client.rs` resolves each host, applies
  `is_private_ip_str` to the POST-DNS addresses, and pins them per-host
  (`resolve_to_addrs`, `PINNED_CLIENT_CACHE_CAP = 64`) — DNS-rebinding-safe;
  redirect policy none; size caps. Post-resolve `addr.ip()` checks also in
  `media-core/src/source.rs`, `blossom.rs`, `identity-core/src/nip05.rs`,
  `sync-core/src/engine.rs`. `common-core/src/url.rs` regex set covers
  localhost/127/0x7f/169.254/private/octal/hex/decimal + rebinding domains.
- **Verified-events**: every relay ingest surface filters `e.verify().is_ok()`
  — network (309/337), relay, chatrandom, vouch, music, search + p-tag logic
  in `ingest.rs`.
- **Zap amounts**: totals from verified receipts only (nwc.rs:188 filters
  `wallet_pk` + `verify()`); `parse_msats_from_bolt11` overflow/cap-checked;
  bridge rejects any bolt11 ≠ the requested invoice (zap.rs:239-259).
- **Codec bounds**: h264/audio MediaCodec paths fully guarded
  (`need = w*h*4` via checked_mul, offset+size ≤ capacity, `.get().unwrap_or()`
  reads; `queueInputBuffer` size is `usize` per ndk.rs) — no change.
- **linkpreview `html.rs:213` `caps.get(0).unwrap()`**: regex mandates a
  closing `>` so `len-1` cuts ASCII; prefix 5/6 slice on ASCII tag-name
  boundary — no panic path.
- **Unbounded growth**: raster `FRAME_BUFFERS` TTL-reclaims; groups
  `ATTEMPTS` swept + `MAX_ATTEMPTS_MAP=1000`; daemon maps fixed-key;
  no `mpsc::unbounded_channel`. Watch-only (no change): session `accounts`
  Vec (no cap, but user-driven), notifications table (no retention, local
  only).

### Verified
- Gate green at end of round: fmt / clippy `-D warnings` / workspace tests
  3× (2270) / `flutter analyze` 0 issues. Commit hash appended on commit.

## Round 6 — 2026-09-05: full-workspace crash/robustness sweep

Scope: 8 passes — (P1) Rust panic triage, (P2) arithmetic/overflow, (P3)
byte-offset slicing, (P4) poison/lock audit, (P5) FFI drift Dart↔Rust, (P6)
Dart runtime crashes, (P7) libsql correctness spot-check, plus a findings
ledger. Commit: `52af388`.

### Fixed (this round)

**network-core (P1/P2)**
- `swarm.rs:338` — `done.lock().unwrap()` poisoned-lock panic when a download
  worker panics. Now `unwrap_or_else(|e| e.into_inner())`. (swarm.rs:311/324
  already used `into_inner`; this was the one straggler.)
- `blob_grab.rs:77/112` — `chr.offset as usize` unsigned→pointer-size
  truncation on 32-bit hosts. Both the QUIC path (`LanChunkRequest.offset`)
  and the TCP fallback (`fetch_verified_chunk`) now validate
  `usize::try_from(chr.offset)` and bail out with an error.
- `freenet_cache_router.rs:92` — `manifest.total_size as usize` truncation.
  Now `let Ok(total_size) = usize::try_from(manifest.total_size) else { return None }`.
- `skademlia.rs:50` — `hash[full_bytes]` out-of-bounds panic when
  `bits > 256`. `check_leading_zeros` now short-circuits `bits > 256`.

**telemetry-core (P2/P3, hostile local input)**
- `read_header` (`lib.rs:372`) — forged `len` past the mmap end walked reads
  off the tail. Clamped: `len.min(mmap.len() as u64 - HEADER_LEN as u64)`.
  (The header digest is a plain sha256 of a recomputable prefix, so the
  checksum alone is not an integrity barrier.)
- `read_all` — `pos + ENTRY_HEADER as u64 > end` guard added before entry
  body reads (ENTRY_HEADER = 17: 4+1+8+4). Forged short files no longer walk
  reads past the mmap tail.

**storage-core (P2)**
- `erasure_fountain.rs::encode_fountain` — lacked the `MAX_FOUNTAIN_LEN`
  bound that `decode_fountain` already had. Now rejects
  `data.is_empty() || data.len() > MAX_FOUNTAIN_LEN` before the block-math.
  (Reachable from `flutter-bridge/src/ffi/p2p.rs`.)

**Soshal Flutter (P6, Dart runtime crashes)**
- `lib/screens/widgets/wgpu_mesh_canvas.dart` — the mesh render loop died
  after ONE rendered frame: `_renderTick` cancelled `_renderTimer` but never
  nulled it, so the `finally` re-arm (`_renderTimer ??= Timer(...)`) was a
  permanent no-op. `_renderTimer = null` after cancel restores the loop.
- `lib/services/p2p_service.dart` — `_pollPower` re-armed the periodic power
  timer unconditionally after `await powerSampleOsState()`; a dispose during
  the await resurrected the timer (leak + notifyListeners-after-dispose).
  `_disposed` flag now guards both the success and catch paths, and is set in
  `dispose()`.
- `lib/services/calls_service.dart`, `chatrandom_service.dart`,
  `zap_service.dart` — `notifyListeners()` after real awaits with no
  disposed-guard. All three now mix in `DeferredNotify` and use
  `notifyDeferred()` (same pattern as feed/messaging/groups/etc.). Bonus:
  fixes the "markNeedsBuild during build" assertion class for `initState`
  reachable fetches.

### Verified clean (this round)

- **Bridge FFI surfaces (P1)**: zero reachable panics in non-test bodies
  across `flutter-bridge/src/ffi/**` — unwraps/expects are confined to
  debug/test code; production fns all return `Result`.
- **sync-core / minis-core / streaming-core (P1)**: unwrap sites are
  invariants, not hostile input — `prolly_sync.rs:96` (len-invariant index),
  `ingest.rs:724` (1:1 masks), `minis-core/events.rs` tag slices are
  defensively truncated; streaming video paths don't index raw input.
- **Poison/lock audit (P4)**: every production Mutex/RwLock use tolerates
  poison (`into_inner`) or probes with `if let Ok` — incl. transport.rs,
  wot.rs, sync.rs, identicon.rs, nostr-core models.rs, nip44.rs,
  `MEASURE_CACHE`. No `unwrap()` after `.lock()` in non-test code.
- **libsql (P7)**: no rusqlite leftovers; all migrations run through
  transactional `execute_batch`; libsql Connection clone semantics respected
  (`params![x.as_str()]`, no 1-tuples, `block_on` around builders).
- **Byte-offset slicing (P3)**: `measure.rs:135`, `entities.rs:102`,
  `glitter.rs:163`, `sync.rs:213` (`event_id_of`) — all indexes are
  char-aligned by construction (linebreak/word/regex boundaries won't split
  UTF-8); moderation regexes are linear-time (`regex` crate) and input-capped
  via `MAX_MODERATION_INPUT_LEN`.
- **waveform.rs, pir.rs** (P2): `sum / ch` is `inf` not a panic; bounded by
  `MAX_CONTAINER_SAMPLES`. PIR math uses checked/guarded paths.
- **FFI drift (P5)**: `crateFfi*` wire tokens across all `lib/ffi/*.dart`
  glue + `frb_generated.dart` (505 tokens) each resolve to a live Rust
  `pub fn` (509 live). The three known-removed surfaces are confirmed gone
  (`streaming_start_local_server`, `streaming_get_video_url`,
  `streaming_stop_local_server`, `moderation_hybrid_classify_media`,
  `search_index_post` singular — only the plural `search_index_posts`
  remains). `p2p_swarm_status_dto_default` maps to generated
  `P2pSwarmStatusDto::default()`, not a hand-written fn — expected.
  Checker: `/tmp/opencode/ffi_drift.py`, frb naming rule is
  `crateFfi + Title(module_stem) + Title(full_fn_name)`.
- **Dart screens/widgets**: broadly mounted-guarded; the 5 service-level
  gaps above were the actionable set.

### Notes / watch items
- Discarded angle: byte-slice "truncation" flags in hashtag extraction use
  `\w` boundaries (ASCII-safe, char-aligned) — no fix needed.
- `/tmp` is a 16G tmpfs: full `cargo test` loops can fill it with
  `soshal_*` fixtures — `rm -rf /tmp/soshal_*` between iterations is
  required, not optional.

## Round 5 — 2026-09: test flake + swsearch hardening (committed `b28c6e5`)

### Fixed
- `flutter-bridge/tests/flutter_bridge_tests.rs::test_database_workflow` —
  called `db_init` (re-points the process-global DB handle) with no lock,
  racing every live DB test → the `streaming_story_roundtrip` flake. Looked
  for in the audit already, but the top-level file sits OUTSIDE the per-module
  glob; now holds `crate::test_util::lock()`.
- `auth_generate_keypair` (auth.rs:22) and `auth_restore_from_mnemonic`
  (auth.rs:62) both mutate the process-global signer via `signer_unlock`.
  Three tests (`test_auth_keypair_generation`, `test_auth_npub_encode_decode`,
  `test_auth_flow_end_to_end`) called them lock-free → "signer locked"
  cross-module races. All now hold the global test lock;
  `test_auth_flow_end_to_end` carries `#[allow(clippy::await_holding_lock)]`.
- swsearch posture hardening: text handling, category parse, and rate-limit
  fixes for the round-5 pass.

### Verified
- 40/40 `flutter_bridge_tests` runs green with `/tmp/soshal_*` cleanup;
  workspace suite 3/3 green; `flutter analyze` 0 issues.
- The 9-failure "database is full / no space" cascade was environmental
  (tmpfs exhaustion), not logic.

## Round 4 — earlier: Dart crash cleanup (committed `414fd9c`, see git log)