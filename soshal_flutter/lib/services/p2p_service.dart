// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:convert';

import 'package:battery_plus/battery_plus.dart';
import 'package:connectivity_plus/connectivity_plus.dart';
import 'package:flutter/foundation.dart';
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
        p2PQuicServerPort,
        p2PQuicServerStart,
        p2PQuicServerStop,
        p2PSwarmCancel,
        p2PSwarmDownload,
        p2PSwarmPoll,
        p2PStopAll;

import 'error_log.dart';

/// P2P Service
///
/// Local-first peer stack: mDNS LAN discovery (advertise + browse), the
/// HMAC-authenticated LAN chunk server, parallel swarm downloads into
/// mmap'd sparse files, and the thermal/battery-aware seeding scheduler.
/// All key material stays in the Rust signer; Dart only sees pubkeys and
/// OS power/connectivity facts.
class P2pService extends ChangeNotifier with LastErrorMixin {
  final List<P2pPeerDto> _peers = [];
  final Map<String, P2pSwarmStatusDto> _downloads = {};
  int? _lanPort;
  int? _quicPort;
  P2pPowerDto? _power;
  bool _advertising = false;
  bool _browsing = false;
  Timer? _pollTimer;

  List<P2pPeerDto> get peers => List.unmodifiable(_peers);
  Map<String, P2pSwarmStatusDto> get downloads => Map.unmodifiable(_downloads);
  int? get lanPort => _lanPort;
  int? get quicPort => _quicPort;
  P2pPowerDto? get power => _power;
  bool get advertising => _advertising;
  bool get browsing => _browsing;

  /// Battery/connectivity plugin handles (lazy, so construction never fails).
  final _battery = Battery();
  final _connectivity = Connectivity();
  Timer? _powerTimer;
  StreamSubscription<List<ConnectivityResult>>? _connSub;

  P2pService() {
    _powerTimer =
        Timer.periodic(const Duration(seconds: 30), (_) => _pollPower());
    _connSub = _connectivity.onConnectivityChanged.listen(
      (_) => _pollPower(),
      onError: (Object _) {},
    );
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

  /// Port of the running QUIC stream server, read live from Rust.
  Future<int?> quicServerPort() async {
    try {
      _quicPort = p2PQuicServerPort();
      notifyListeners();
      return _quicPort;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return null;
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

  /// Port of the running LAN chunk server, read live from Rust.
  Future<int?> lanServerPort() async {
    try {
      _lanPort = p2PLanServerPort();
      notifyListeners();
      return _lanPort;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return null;
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
      final bytes = p2PQuicFetchChunk(
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
      _peers
        ..clear()
        ..addAll(capped);
      clearLastError();
      notifyListeners();
      return found;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return const [];
    }
  }

  /// UI-facing wrapper: sample battery/connectivity and push into the Rust
  /// seeding scheduler now (same path as the periodic poller).
  Future<void> refreshPowerFromOs() => _pollPower();

  /// Sample battery + connectivity and push into the Rust seeding scheduler.
  /// Errors are swallowed: power state simply stays at its last good value.
  Future<void> _pollPower() async {
    try {
      final batteryLevel = await _battery.batteryLevel;
      final batteryState = await _battery.batteryState;
      final isSaving = await _battery.isInBatterySaveMode;
      final connections = await _connectivity.checkConnectivity();
      final cellular = connections.any(
        (c) => c == ConnectivityResult.mobile,
      );
      await updatePower(
        charging: batteryState == BatteryState.charging ||
            batteryState == BatteryState.full,
        batteryPercent: batteryLevel < 0 ? 100 : batteryLevel,
        cellular: cellular,
        lowPowerMode: isSaving,
      );
    } catch (_) {
      // Plugins can throw on emulators / desktop; keep last known state.
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
      _downloads[id] = status;
      if (status.state == 'done') {
        _downloads.remove(id);
      }
      notifyListeners();
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
      _power = p2PPowerUpdate(
        charging: charging,
        batteryPercent: batteryPercent,
        cellular: cellular,
        lowPowerMode: lowPowerMode,
      );
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
      _power = p2PPowerMode();
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
    notifyListeners();
  }

  /// Poll swarm downloads + drain peers every [interval]; call from a
  /// lifecycle-aware owner so the timer stops when the app backgrounds.
  void startPolling(Duration interval, {String? activeDownloadId}) {
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

  @override
  void dispose() {
    _pollTimer?.cancel();
    _powerTimer?.cancel();
    _connSub?.cancel();
    super.dispose();
  }
}
