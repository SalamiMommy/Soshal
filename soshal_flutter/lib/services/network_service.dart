// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/ffi/network.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Network Service
/// Transport status: I2P + Freenet presence + HTTP/3 stack + transport mode.
class NetworkService extends ChangeNotifier with LastErrorMixin {
  static const String _modeKey = 'transport_mode';

  bool? _i2p;
  bool? _freenet;
  List<RelayInfo> _relays = [];
  TransportMode _transportMode = TransportMode.clearnet;

  bool? get i2p => _i2p;
  bool? get freenet => _freenet;
  List<RelayInfo> get relays => _relays;
  TransportMode get transportMode => _transportMode;
  bool get i2pForced => _transportMode == TransportMode.i2p;

  /// Loads the persisted transport mode, then syncs the Rust-side static.
  Future<void> loadTransportMode() async {
    try {
      final saved = RustLib.instance.api.crateFfiDbDbGetSetting(key: _modeKey);
      final mode = TransportMode.parse(saved?.toString() ?? '');
      if (mode != null) {
        await setTransportMode(mode);
      } else {
        final current =
            RustLib.instance.api.crateFfiNetworkNetworkGetTransportMode();
        _transportMode = TransportMode.parse(current) ?? TransportMode.clearnet;
        notifyListeners();
      }
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
    }
  }

  /// Persists + applies the transport mode. Re-init relays afterwards for
  /// the SOCKS proxy to take effect.
  Future<bool> setTransportMode(TransportMode mode) async {
    try {
      final ok = RustLib.instance.api
          .crateFfiNetworkNetworkSetTransportMode(mode: mode.name);
      if (ok) {
        _transportMode = mode;
        RustLib.instance.api
            .crateFfiDbDbSetSetting(key: _modeKey, value: mode.name);
        _lastError = null;
        notifyListeners();
      }
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Starts the persistent i2p SAM session; returns the destination address.
  Future<String> startI2pSession({String? destination}) async {
    try {
      final dest = RustLib.instance.api
          .crateFfiNetworkI2PStartSession(destination: destination);
      _lastError = null;
      notifyListeners();
      return dest;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Stops the persistent i2p SAM session.
  Future<bool> stopI2pSession() async {
    try {
      final ok = RustLib.instance.api.crateFfiNetworkI2PStopSession();
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// `{running, destination}` for the i2p SAM session.
  Future<Map<String, dynamic>> i2pSessionStatus() async {
    try {
      final json = RustLib.instance.api.crateFfiNetworkI2PSessionStatus();
      final map = jsonDecode(json) as Map<String, dynamic>;
      _lastError = null;
      return map;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Reconnects the relay client through the currently active transport.
  Future<void> reinitRelays() async {
    if (_relays.isEmpty) return;
    await initRelays(_relays.map((r) => r.url).toList());
  }

  /// Refresh transport status from the bridge.
  Future<void> refresh() async {
    try {
      _i2p = await RustLib.instance.api.crateFfiNetworkNetworkI2PStatus();
      _freenet =
          await RustLib.instance.api.crateFfiNetworkNetworkFreenetStatus();
      _lastError = null;
      notifyListeners();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
    }
  }

  /// Perform an HTTP request over Rust HTTP/3 & QUIC network stack.
  Future<HttpResponseDto> fetchHttp3(
    String url, {
    String method = 'GET',
    Map<String, String>? headers,
    Uint8List? body,
  }) async {
    try {
      final headersJson = jsonEncode(headers ?? {});
      final resp = await RustLib.instance.api.crateFfiNetworkNetworkFetchHttp3(
        url: url,
        method: method,
        headersJson: headersJson,
        body: body,
      );
      _lastError = null;
      return resp;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Fetch per-relay connection status from the bridge client.
  Future<List<RelayInfo>> fetchRelayStatus() async {
    try {
      final json =
          await RustLib.instance.api.crateFfiNetworkNetworkGetRelayStatus();
      final list = jsonDecode(json) as List<dynamic>;
      final relays = list
          .map((e) => RelayInfo.fromJson(e as Map<String, dynamic>))
          .toList();
      _relays = relays;
      _lastError = null;
      notifyListeners();
      return relays;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Connect a new relay to the bridge relay client.
  Future<bool> addRelay(String url) async {
    try {
      final ok =
          await RustLib.instance.api.crateFfiNetworkNetworkAddRelay(url: url);
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Disconnect and forget a relay.
  Future<bool> removeRelay(String url) async {
    try {
      final ok = await RustLib.instance.api
          .crateFfiNetworkNetworkRemoveRelay(url: url);
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// (Re)initialize the relay client with a fresh URL list.
  Future<String> initRelays(List<String> relayUrls) async {
    try {
      final result = await RustLib.instance.api
          .crateFfiNetworkNetworkInitRelays(relayUrls: relayUrls);
      _lastError = null;
      notifyListeners();
      return result;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Multi-bearer off-grid mesh status (BLE, Wi-Fi Direct, LAN) as JSON map.
  Future<Map<String, dynamic>> fetchMultiBearerStatus(String ownPubkey) async {
    try {
      final json = await RustLib.instance.api
          .crateFfiNetworkNetworkGetMultiBearerStatus(ownPubkey: ownPubkey);
      final map = jsonDecode(json) as Map<String, dynamic>;
      _lastError = null;
      return map;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Kernel / hardware crypto / storage engine diagnostics as JSON string
  /// (sync FFI; awaiting is harmless but this returns immediately).
  String fetchSysDiagnostics() {
    try {
      final json =
          RustLib.instance.api.crateFfiNetworkNetworkGetSysDiagnostics();
      _lastError = null;
      return json;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Reconciles Prolly Tree root hashes with a remote peer.
  /// Call this after LAN peer connect in p2p_service.
  Future<Map<String, dynamic>> reconcileProllyTree({
    required Map<String, String> localKv,
    required String remoteRootHash,
  }) async {
    try {
      final localKvJson = jsonEncode(localKv);
      final resJson =
          await RustLib.instance.api.crateFfiNetworkNetworkReconcileProllyTree(
        localKvJson: localKvJson,
        remoteRootHash: remoteRootHash,
      );
      final Map<String, dynamic> res =
          Map<String, dynamic>.from(jsonDecode(resJson) as Map);
      _lastError = null;
      notifyListeners();
      return res;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return {'success': false, 'error_msg': e.toString()};
    }
  }
}

/// Transport mode for outgoing traffic (mirrors network-core transport.rs).
enum TransportMode {
  clearnet,
  auto,
  i2p;

  static TransportMode? parse(String name) {
    for (final mode in TransportMode.values) {
      if (mode.name == name) return mode;
    }
    return null;
  }
}

/// Relay connection status as served by the bridge network module.
class RelayInfo {
  final String url;
  final bool connected;
  final int latencyMs;
  final int lastEventAt;

  RelayInfo({
    required this.url,
    required this.connected,
    required this.latencyMs,
    required this.lastEventAt,
  });

  factory RelayInfo.fromJson(Map<String, dynamic> json) {
    return RelayInfo(
      url: json['url'] as String? ?? '',
      connected: json['connected'] as bool? ?? false,
      latencyMs: (json['latency_ms'] as num?)?.toInt() ?? 0,
      lastEventAt: (json['last_event_at'] as num?)?.toInt() ?? 0,
    );
  }
}
