# Soshal Workspace Audit Ledger

Finding ledger for the periodic full-workspace bug sweeps. Each round lists
what was found, what was fixed (with file refs), and what was examined and
cleared. Rounds are cumulative; last-known-good state is shaded green.

## Round 6 — 2026-09-05: full-workspace crash/robustness sweep

Scope: 8 passes — (P1) Rust panic triage, (P2) arithmetic/overflow, (P3)
byte-offset slicing, (P4) poison/lock audit, (P5) FFI drift Dart↔Rust, (P6)
Dart runtime crashes, (P7) libsql correctness spot-check, plus a findings
ledger. Commit pending.

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