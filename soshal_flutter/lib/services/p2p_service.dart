// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:soshal_flutter/ffi/p2p.dart'
    show
        P2pPeerDto,
        P2pPowerDto,
        P2pSwarmStatusDto,
        p2PLanServerPort,
        p2PLanServerStart,
        p2PLanServerStop,
        p2PMdnsAdvertiseStart,
        p2PMdnsAdvertiseStop,
        p2PMdnsBrowseDrain,
        p2PMdnsBrowseStart,
        p2PMdnsBrowseStop,
        p2PPowerMode,
        p2PPowerUpdate,
        p2PDecodeFountainPayload,
        p2PEncodeFountainPayload,
        p2PQuicFetchChunk,
        p2PQuicServerStart,
        p2PQuicServerStop,
        p2PSwarmCancel,
        p2PSwarmDownload,
        p2PSwarmPoll,
        p2PStopAll;
import 'package:soshal_flutter/ffi/power.dart'
    show PowerStateDto, powerSampleOsState;

export 'package:soshal_flutter/ffi/p2p.dart'
    show P2pPeerDto, P2pPowerDto, P2pSwarmStatusDto;

import 'error_log.dart';

/// P2P Service
///
/// Local-first peer stack: mDNS LAN discovery (advertise + browse), the
/// HMAC-authenticated LAN chunk server, parallel swarm downloads into
/// mmap'd sparse files, and the thermal/battery-aware seeding scheduler.
/// All key material stays in the Rust signer; OS power/connectivity facts
/// come from Rust too (JNI on Android, UPower + NetworkManager on Linux).
class P2pService extends ChangeNotifier with LastErrorMixin {
  final List<P2pPeerDto> _peers = [];
  final Map<String, P2pSwarmStatusDto> _downloads = {};
  List<P2pPeerDto> _cachedPeers = const [];
  Map<String, P2pSwarmStatusDto> _cachedDownloads = const {};
  int? _lanPort;
  int? _quicPort;
  P2pPowerDto? _power;
  bool _advertising = false;
  bool _browsing = false;
  Timer? _pollTimer;
  Duration? _pollInterval;
  bool _pollingEnabled = false;

  List<P2pPeerDto> get peers => _cachedPeers;
  Map<String, P2pSwarmStatusDto> get downloads => _cachedDownloads;
  int? get lanPort => _lanPort;
  int? get quicPort => _quicPort;
  P2pPowerDto? get power => _power;
  bool get advertising => _advertising;
  bool get browsing => _browsing;

  /// Battery/connectivity handles live in Rust (`ffi/power.dart`); the
  /// periodic poller below pushes samples into the seeding scheduler.
  Timer? _powerTimer;
  AppLifecycleListener? _lifecycle;
  bool _appActive = true;
  bool _disposed = false;
  Duration _powerInterval = const Duration(seconds: 30);
  PowerStateDto? _lastPowerSample;

  P2pService() {
    _lifecycle = AppLifecycleListener(
      onHide: _pausePollAndPowerTimers,
      onPause: _pausePollAndPowerTimers,
      onResume: _resumePollAndPowerTimers,
    );
    _startPowerTimer();
    _pollPower();
  }

  void _pausePollAndPowerTimers() {
    _pausePollTimer();
    _pausePowerTimer();
    if (_lanPort != null) stopLanServer();
    if (_quicPort != null) stopQuicServer();
  }

  void _resumePollAndPowerTimers() {
    _resumePollTimer();
    _resumePowerTimer();
  }

  /// (Re)start the power poller at the current interval; no-op while the
  /// app is backgrounded so battery/connectivity FFI stays quiescent.
  void _startPowerTimer() {
    if (!_appActive) return;
    _powerTimer?.cancel();
    _powerTimer = Timer.periodic(_powerInterval, (_) => _pollPower());
  }

  void _pausePowerTimer() {
    _appActive = false;
    _powerTimer?.cancel();
    _powerTimer = null;
  }

  void _resumePowerTimer() {
    _appActive = true;
    _startPowerTimer();
    _pollPower();
  }

  /// Start the LAN + QUIC chunk servers and advertise them over mDNS.
  /// Returns the LAN port. Pubkey empty = use unlocked signer. The QUIC
  /// server's port is advertised in TXT records so peers prefer it.
  Future<int> start({String pubkey = ''}) async {
    try {
      _lanPort = p2PLanServerStart(storeRoot: '');
      _quicPort = p2PQuicServerStart(storeRoot: '');

      _advertising = p2PMdnsAdvertiseStart(
        pubkey: pubkey,
        port: _lanPort!,
        quicPort: _quicPort,
      );
      clearLastError();
      notifyListeners();
      return _lanPort!;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Begin browsing the subnet for `_soshal._tcp` services.
  Future<void> startBrowsing() async {
    try {
      _browsing = p2PMdnsBrowseStart();
      notifyListeners();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Stop browsing (and drop the mDNS daemon).
  Future<bool> stopBrowsing() async {
    try {
      final ok = p2PMdnsBrowseStop();
      _browsing = false;
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return false;
    }
  }

  /// Advertise this device's chunk server over mDNS. `port` falls back to
  /// the running LAN server (or a fresh start), `quicPort` to the running
  /// QUIC server.
  Future<bool> startAdvertising({
    String pubkey = '',
    int? port,
    int? quicPort,
  }) async {
    try {
      final p = port ?? _lanPort ?? p2PLanServerPort();
      _advertising = p2PMdnsAdvertiseStart(
        pubkey: pubkey,
        port: p,
        quicPort: quicPort ?? _quicPort,
      );
      clearLastError();
      notifyListeners();
      return _advertising;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return false;
    }
  }

  /// Stop advertising (and drop the mDNS daemon).
  Future<bool> stopAdvertising() async {
    try {
      final ok = p2PMdnsAdvertiseStop();
      _advertising = false;
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return false;
    }
  }

  /// Start only the QUIC stream media server (no mDNS advertise).
  /// Returns the bound port.
  Future<int?> startQuicServer({String storeRoot = ''}) async {
    try {
      _quicPort = p2PQuicServerStart(storeRoot: storeRoot);
      clearLastError();
      notifyListeners();
      return _quicPort;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Stop the QUIC stream server.
  Future<bool> stopQuicServer() async {
    try {
      final ok = p2PQuicServerStop();
      _quicPort = null;
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return false;
    }
  }

  /// Stop the LAN chunk server.
  Future<bool> stopLanServer() async {
    try {
      final ok = p2PLanServerStop();
      _lanPort = null;
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return false;
    }
  }

  /// Fetch one verified chunk range from a peer over a QUIC stream
  /// (blob-hash or chunk-hash mode; BLAKE3-verified on the Rust side).
  Future<Uint8List?> fetchQuicChunk({
    required String addr,
    required String hash,
    required BigInt offset,
    required BigInt length,
  }) async {
    try {
      final bytes = await p2PQuicFetchChunk(
        addr: addr,
        hash: hash,
        offset: offset,
        length: length,
      );
      clearLastError();
      notifyListeners();
      return bytes;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return null;
    }
  }

  /// Drain newly discovered LAN peers.
  Future<List<P2pPeerDto>> drainPeers() async {
    try {
      final found = p2PMdnsBrowseDrain();
      final capped = found.length > 50 ? found.sublist(0, 50) : found;
      if (!listEquals(_peers, capped)) {
        _peers
          ..clear()
          ..addAll(capped);
        _cachedPeers = List.unmodifiable(_peers);
        notifyListeners();
      }
      clearLastError();
      return capped;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return const [];
    }
  }

  /// UI-facing wrapper: sample battery/connectivity and push into the Rust
  /// seeding scheduler now (same path as the periodic poller).
  Future<void> refreshPowerFromOs() => _pollPower();

  /// Sample OS power/connectivity in Rust and push into the seeding
  /// scheduler. Errors are swallowed: power state simply stays at its last
  /// good value. Idle unchanged state backs off 30s → 60s → 120s; state
  /// change, active peers, or in-flight downloads reset to 30s.
  Future<void> _pollPower() async {
    try {
      final sample = await powerSampleOsState();
      if (_disposed) return;
      await updatePower(
        charging: sample.charging,
        batteryPercent: sample.batteryPercent,
        cellular: sample.cellular,
        lowPowerMode: sample.lowPowerMode,
      );
      if (_downloads.isNotEmpty || _peers.isNotEmpty) {
        _powerInterval = const Duration(seconds: 30);
      } else if (_lastPowerSample == sample) {
        _powerInterval = _powerInterval.inSeconds >= 60
            ? const Duration(seconds: 120)
            : const Duration(seconds: 60);
      } else {
        _powerInterval = const Duration(seconds: 30);
      }
      _lastPowerSample = sample;
      _startPowerTimer();
    } catch (_) {
      if (_disposed) return;
      final backoff = _powerInterval.inSeconds * 2;
      _powerInterval = Duration(seconds: backoff > 300 ? 300 : backoff);
      _startPowerTimer();
    }
  }

  /// Kick off a swarm download of a blob manifest from LAN peers.
  /// Returns the download id; poll with [swarmStatus].
  /// quicPorts is a parallel list of optional QUIC ports (null if not advertised).
  Future<String> swarmDownload({
    required String manifestJson,
    required List<String> peers,
    required List<int?> quicPorts,
    required String outPath,
    int maxParallel = 4,
  }) async {
    try {
      final quicPortsJson = jsonEncode(quicPorts);
      final id = p2PSwarmDownload(
        manifestJson: manifestJson,
        peersJson: jsonEncode(peers),
        quicPortsJson: quicPortsJson,
        outPath: outPath,
        maxParallel: BigInt.from(maxParallel),
      );
      if (_downloads.length >= 50) {
        _downloads.remove(_downloads.keys.first);
      }
      _downloads[id] = P2pSwarmStatusDto(
        state: 'running',
        verifiedChunks: BigInt.zero,
        bytesDownloaded: BigInt.zero,
        failures: BigInt.zero,
        failedHashes: const [],
      );
      _cachedDownloads = Map.unmodifiable(_downloads);
      clearLastError();
      notifyListeners();
      return id;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Poll a swarm download once; finished downloads report + drop from map.
  Future<P2pSwarmStatusDto?> swarmStatus(String id) async {
    try {
      final status = p2PSwarmPoll(id: id);
      final prev = _downloads[id];
      if (prev == null || prev != status) {
        if (status.state == 'done') {
          _downloads.remove(id);
        } else {
          _downloads[id] = status;
        }
        _cachedDownloads = Map.unmodifiable(_downloads);
        notifyListeners();
      }
      return status;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return null;
    }
  }

  /// Cancel a swarm download (detaches the worker thread).
  Future<void> swarmCancel(String id) async {
    try {
      p2PSwarmCancel(id: id);
      _downloads.remove(id);
      _cachedDownloads = Map.unmodifiable(_downloads);
      notifyListeners();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
    }
  }

  /// Push OS power/connectivity facts into the seeding scheduler.
  /// Returns the resulting mode snapshot (paused/throttled/full).
  Future<P2pPowerDto> updatePower({
    required bool charging,
    required int batteryPercent,
    required bool cellular,
    required bool lowPowerMode,
  }) async {
    try {
      final next = p2PPowerUpdate(
        charging: charging,
        batteryPercent: batteryPercent,
        cellular: cellular,
        lowPowerMode: lowPowerMode,
      );
      // Gate: telemetry pushes (battery drain events) arrive frequently
      // while the mode stays put; don't rebuild subscribers on no-change.
      if (_power == next) {
        clearLastError();
        return next;
      }
      _power = next;
      clearLastError();
      notifyListeners();
      return _power!;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Current seeding mode snapshot.
  Future<P2pPowerDto?> currentPower() async {
    try {
      final next = p2PPowerMode();
      if (_power == next) {
        clearLastError();
        return next;
      }
      _power = next;
      notifyListeners();
      return _power;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return null;
    }
  }

  /// Encode raw payload into rateless RaptorQ Fountain code packets.
  /// Wire into Phase F upload path for big blobs.
  Future<Map<String, dynamic>> encodeFountainPayload({
    required Uint8List data,
    required double redundancyRatio,
  }) async {
    try {
      final manifestJson = p2PEncodeFountainPayload(
        data: data,
        redundancyRatio: redundancyRatio,
      );
      final Map<String, dynamic> manifest =
          Map<String, dynamic>.from(jsonDecode(manifestJson) as Map);
      clearLastError();
      notifyListeners();
      return manifest;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return {'success': false, 'error_msg': e.toString()};
    }
  }

  /// Rebuild original payload from received Fountain code packets.
  Future<Uint8List?> decodeFountainPayload({
    required String manifestJson,
    required String packetsB64Json,
  }) async {
    try {
      final bytes = p2PDecodeFountainPayload(
        manifestJson: manifestJson,
        packetsB64Json: packetsB64Json,
      );
      clearLastError();
      notifyListeners();
      return bytes;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return null;
    }
  }

  /// Tear down advertiser, browser, LAN server, and in-flight downloads.
  Future<void> stopAll() async {
    _pollTimer?.cancel();
    _pollTimer = null;
    try {
      p2PStopAll();
    } catch (e, st) {
      setLastError(e, st);
    }
    _advertising = false;
    _browsing = false;
    _lanPort = null;
    _quicPort = null;
    _downloads.clear();
    _peers.clear();
    _cachedDownloads = const {};
    _cachedPeers = const [];
    notifyListeners();
  }

  /// Poll swarm downloads + drain peers every [interval]; call from a
  /// lifecycle-aware owner so the timer stops when the app backgrounds.
  void startPolling(Duration interval, {String? activeDownloadId}) {
    _pollingEnabled = true;
    _pollInterval = interval;
    _pollTimer?.cancel();
    _pollTimer = Timer.periodic(interval, (_) async {
      final active = activeDownloadId ??
          (_downloads.keys.isNotEmpty ? _downloads.keys.first : null);
      if (active != null) {
        await swarmStatus(active);
      }
      await drainPeers();
    });
  }

  /// Stops the swarm poll timer (e.g. when the owning screen disposes).
  void stopPolling() {
    _pollingEnabled = false;
    _pollInterval = null;
    _pollTimer?.cancel();
    _pollTimer = null;
  }

  void _pausePollTimer() {
    _pollTimer?.cancel();
    _pollTimer = null;
  }

  void _resumePollTimer() {
    final interval = _pollInterval;
    if (!_pollingEnabled || !_appActive || interval == null) return;
    startPolling(interval);
  }

  @override
  void dispose() {
    _disposed = true;
    _pollTimer?.cancel();
    _powerTimer?.cancel();
    _lifecycle?.dispose();
    super.dispose();
  }
}
