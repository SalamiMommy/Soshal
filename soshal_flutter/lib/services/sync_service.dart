// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import 'feed_service.dart';
import 'ffi_bridge.dart';
import 'messaging_service.dart';

/// Sync Service
/// Subscribes to the Rust-side background sync engine (published via the
/// `syncEvents` stream) and routes updates into the domain services:
/// new feed posts append live, incoming DMs decrypt+land in conversation
/// lists, reactions and profiles refresh their surfaces.
class SyncService extends ChangeNotifier with LastErrorMixin {
  StreamSubscription<String>? _subscription;
  Timer? _notifyDebounceTimer;
  Timer? _gcTimer;
  FeedService? _feed;
  MessagingService? _messaging;
  bool _started = false;
  bool _subscribed = false;

  /// How far back deletions must be acknowledged before tombstones go away.
  static const int _gcConsensusWindowSecs = 7 * 24 * 3600;

  bool get started => _started;

  void _scheduleNotify() {
    _notifyDebounceTimer?.cancel();
    _notifyDebounceTimer = Timer(const Duration(milliseconds: 150), () {
      notifyListeners();
    });
  }

  /// Attach downstream consumers (called from main.dart wiring).
  void attach(
      {required FeedService feed, required MessagingService messaging}) {
    _feed = feed;
    _messaging = messaging;
  }

  /// Start the engine + stream subscription for the given relays.
  /// Idempotent: repeated calls with the same relays are no-ops.
  Future<void> start({required List<String> relays}) async {
    if (relays.isEmpty) return;
    if (_started) return;
    _started = true;
    _ensureSubscribed();
    try {
      await RustLib.instance.api.crateFfiSyncSyncStart(
        relaysJson: jsonEncode(relays),
      );
      _gcTimer?.cancel();
      _gcTimer = Timer.periodic(const Duration(days: 1), (_) {
        runScheduledEpochGc();
      });
      clearLastError();
    } catch (e, st) {
      _started = false;
      setLastError(e, st);
    }
    notifyListeners();
  }

  /// Stop the engine (used on sign-out).
  Future<void> stop() async {
    if (!_started) return;
    try {
      await RustLib.instance.api.crateFfiSyncSyncStop();
    } catch (e, st) {
      setLastError(e, st);
    }
    _gcTimer?.cancel();
    _gcTimer = null;
    _started = false;
    notifyListeners();
  }

  void _ensureSubscribed() {
    if (_subscribed) return;
    _subscribed = true;
    _subscription = RustLib.instance.api.crateFfiSyncSyncEvents().listen(_route,
        onError: (Object e) {
      setLastError(e);
      _scheduleNotify();
    });
  }

  void _route(String json) {
    try {
      final msg = jsonDecode(json) as Map<String, dynamic>;
      switch (msg['t']) {
        case 'feed':
          _feed?.insertLivePost(FeedPost(
            eventId: msg['id'] as String? ?? '',
            pubkey: msg['pubkey'] as String? ?? '',
            content: msg['content'] as String? ?? '',
            createdAt: (msg['created_at'] as num?)?.toInt() ?? 0,
            reactions: 0,
            replies: 0,
            reposts: 0,
            liked: false,
          ));
        case 'dm':
          _messaging?.insertLiveDm(DirectMessage(
            id: msg['id'] as String? ?? '',
            sender: msg['sender'] as String? ?? '',
            content: msg['content'] as String? ?? '',
            createdAt: (msg['created_at'] as num?)?.toInt() ?? 0,
            decrypted: true,
            isOwn: false,
          ));
        case 'reaction':
          _feed?.applyLiveReaction(
            msg['event_id'] as String? ?? '',
            msg['pubkey'] as String? ?? '',
            msg['content'] as String? ?? '',
          );
        case 'profile':
        default:
          _scheduleNotify();
      }
    } catch (e, st) {
      setLastError(e, st);
      _scheduleNotify();
    }
  }

  /// Verify a ZK-STARK state rollup payload for instantaneous feed thread validation
  Future<Map<String, dynamic>> verifyZkRollup(String rollupJson) async {
    try {
      final resJson = RustLib.instance.api.crateFfiZkZkVerifyRollup(
        rollupJson: rollupJson,
      );
      final Map<String, dynamic> res =
          Map<String, dynamic>.from(jsonDecode(resJson) as Map);
      clearLastError();
      notifyListeners();
      return res;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return {'verified': false, 'error_msg': e.toString()};
    }
  }

  /// Run epoch garbage collection across active peer vector clocks to prune tombstones.
  /// Call this daily via a timer in the service lifecycle.
  Future<Map<String, dynamic>> runEpochGarbageCollection({
    required String domain,
    required Map<String, int> peerVectorClocks,
    required int gcThresholdSecs,
  }) async {
    try {
      final clocksJson = jsonEncode(peerVectorClocks);
      final resJson =
          RustLib.instance.api.crateFfiSyncSyncRunEpochGarbageCollection(
        domain: domain,
        peerVectorClocksJson: clocksJson,
        gcThresholdSecs: BigInt.from(gcThresholdSecs),
      );
      final Map<String, dynamic> res =
          Map<String, dynamic>.from(jsonDecode(resJson) as Map);
      clearLastError();
      notifyListeners();
      return res;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return {'success': false, 'error_msg': e.toString()};
    }
  }

  /// Compare feed state against a peer's Prolly-tree root hash and get back
  /// the sync-protocol response (branch requests / delta requests). Call
  /// after the peer's state root is known (BLE state-root beacon / handshake).
  Future<Map<String, dynamic>> reconcileFeedWithPeer(
      String remoteRootHash) async {
    final feed = _feed;
    if (feed == null || feed.posts.isEmpty) {
      return {'error_msg': 'no local feed state to reconcile'};
    }
    final kv = <List<String>>[
      for (final p in feed.posts) [p.eventId, p.content]
    ];
    try {
      final resJson =
          await RustLib.instance.api.crateFfiNetworkNetworkReconcileProllyTree(
        localKvJson: jsonEncode(kv),
        remoteRootHash: remoteRootHash,
      );
      final Map<String, dynamic> res =
          Map<String, dynamic>.from(jsonDecode(resJson) as Map);
      clearLastError();
      notifyListeners();
      return res;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return {'error_msg': e.toString()};
    }
  }

  /// Daily sweep: treat each pubkey's newest cached event as its vector-clock
  /// horizon (everyone acknowledged up to that point), then prune tombstones
  /// older than the consensus window. No-op when nothing is cached yet.
  Future<void> runScheduledEpochGc() async {
    final feed = _feed;
    if (feed == null || feed.posts.isEmpty) return;
    final clocks = <String, int>{};
    for (final p in feed.posts) {
      final prev = clocks[p.pubkey] ?? 0;
      if (p.createdAt > prev) clocks[p.pubkey] = p.createdAt;
    }
    if (clocks.isEmpty) return;
    await runEpochGarbageCollection(
      domain: 'feed',
      peerVectorClocks: clocks,
      gcThresholdSecs: _gcConsensusWindowSecs,
    );
  }

  /// Whether the Rust-side sync engine is running.
  bool syncRunning() {
    try {
      return RustLib.instance.api.crateFfiSyncSyncRunning();
    } catch (e) {
      setLastError(e);
      return false;
    }
  }

  /// Outbox summary JSON (pending/broadcast counters).
  String outboxSummary() {
    try {
      return RustLib.instance.api.crateFfiSyncSyncGetOutboxSummary();
    } catch (e) {
      return '{}';
    }
  }

  /// One bounded background-sync pass over relays (feed + DM ingestion).
  Future<int> runBackgroundSync() async {
    try {
      final dbPath = await FfiBridge.getDbPath();
      return RustLib.instance.api
          .crateFfiHeadlessBackgroundSyncTask(dbPath: dbPath);
    } catch (e, st) {
      setLastError(e, st);
      return -1;
    }
  }

  /// Apply a verified ZK state rollup directly to the database cache.
  Future<bool> applyRollup(String rollupJson) async {
    try {
      final dbPath = await FfiBridge.getDbPath();
      final ok = RustLib.instance.api.crateFfiZkZkApplyRollup(
        dbPath: dbPath,
        rollupJson: rollupJson,
      );
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return false;
    }
  }

  @override
  void dispose() {
    _notifyDebounceTimer?.cancel();
    _gcTimer?.cancel();
    _subscription?.cancel();
    super.dispose();
  }
}
