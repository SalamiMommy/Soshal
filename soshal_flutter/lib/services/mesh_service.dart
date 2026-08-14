// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// Mesh Service
/// Reticulum P2P mesh networking: transport, interfaces, addressing,
/// announce, and status. Wraps the 9 `reticulum_*` bridge fns; the pubkey
/// is resolved lazily per call from the active session account.
class MeshService extends ChangeNotifier {
  MeshService({String? Function()? pubkey}) : _pubkeyResolver = pubkey;

  /// Resolves the active account pubkey at call time (SessionService).
  final String? Function()? _pubkeyResolver;

  String? _destinationHash;
  bool _running = false;
  int _rxPackets = 0;
  int _txPackets = 0;
  int _activeRoutes = 0;
  String? _lastError;

  String? get destinationHash => _destinationHash;
  bool get running => _running;
  int get rxPackets => _rxPackets;
  int get txPackets => _txPackets;
  int get activeRoutes => _activeRoutes;
  String? get lastError => _lastError;

  /// Active account pubkey, or null when no account is selected.
  String? _pubkey() => _pubkeyResolver?.call();

  /// Active account pubkey, throwing when no account is selected.
  String _requirePubkey() {
    final pubkey = _pubkey();
    if (pubkey == null || pubkey.isEmpty) {
      throw Exception('No active account pubkey');
    }
    return pubkey;
  }

  /// Starts the Reticulum UDP transport on the given bind address.
  Future<void> startTransport({String bindAddr = '0.0.0.0:4242'}) async {
    try {
      final json = RustLib.instance.api.crateFfiNetworkReticulumStartTransport(
        pubkey: _requirePubkey(),
        bindAddr: bindAddr,
      );
      _lastError = null;
      _parseStatus(json);
      notifyListeners();
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Starts Reticulum AutoInterface for peer discovery.
  Future<void> startAutoInterface({
    required bool enabled,
    required int port,
    required int intervalMs,
  }) async {
    try {
      final json =
          RustLib.instance.api.crateFfiNetworkReticulumStartAutoInterface(
        pubkey: _requirePubkey(),
        enabled: enabled,
        port: port,
        intervalMs: BigInt.from(intervalMs),
      );
      _lastError = null;
      _parseStatus(json);
      notifyListeners();
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Starts the Reticulum TCP server interface.
  Future<void> startTcpServer({
    required int port,
    required int maxConnections,
  }) async {
    try {
      final json = RustLib.instance.api.crateFfiNetworkReticulumStartTcpServer(
        pubkey: _requirePubkey(),
        port: port,
        maxConnections: BigInt.from(maxConnections),
      );
      _lastError = null;
      _parseStatus(json);
      notifyListeners();
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Sends a Reticulum packet to the specified destination.
  Future<bool> sendPacket({
    required String destAddr,
    required String packetJson,
  }) async {
    try {
      final ok = RustLib.instance.api.crateFfiNetworkReticulumSendPacket(
        pubkey: _requirePubkey(),
        destAddr: destAddr,
        packetJson: packetJson,
      );
      _lastError = null;
      return ok;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Creates a Reticulum link request to a remote destination (hex).
  Future<String?> requestLink(String destHex) async {
    try {
      final json = RustLib.instance.api.crateFfiNetworkReticulumRequestLink(
        pubkey: _requirePubkey(),
        destHex: destHex,
      );
      _lastError = null;
      return json;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Creates a Reticulum address from the active account public key.
  Future<String?> addressFromPubkey() async {
    try {
      final json =
          RustLib.instance.api.crateFfiNetworkReticulumAddressFromPubkey(
        pubkey: _requirePubkey(),
      );
      _lastError = null;
      return json;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Creates a Reticulum address from an app name and aspect.
  Future<String?> addressFromAspect(String appName, String aspect) async {
    try {
      final json =
          RustLib.instance.api.crateFfiNetworkReticulumAddressFromAspect(
        appName: appName,
        aspect: aspect,
      );
      _lastError = null;
      return json;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Broadcasts an ANNOUNCE packet for identity discovery.
  Future<void> announce([String? aspect]) async {
    try {
      final json = RustLib.instance.api.crateFfiNetworkReticulumCreateAnnounce(
        pubkey: _requirePubkey(),
        aspect: aspect,
      );
      _lastError = null;
      _parseStatus(json);
      notifyListeners();
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Refreshes node status from the bridge.
  Future<void> refreshStatus() async {
    try {
      final json = RustLib.instance.api.crateFfiNetworkReticulumGetStatus(
        pubkey: _requirePubkey(),
      );
      _lastError = null;
      _parseStatus(json);
      notifyListeners();
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Shared parser for the status JSON returned by the `reticulum_*`
  /// wrappers: `{running, destination_hash, active_routes, rx_packets,
  /// tx_packets, interfaces}`. Only overwrites fields present in the
  /// payload; parse failures land in [_lastError].
  void _parseStatus(String json) {
    try {
      final map = jsonDecode(json) as Map<String, dynamic>;
      if (map.containsKey('running')) {
        _running = map['running'] as bool? ?? false;
      }
      if (map.containsKey('destination_hash')) {
        _destinationHash = map['destination_hash'] as String?;
      }
      if (map.containsKey('active_routes')) {
        _activeRoutes = (map['active_routes'] as num?)?.toInt() ?? 0;
      }
      if (map.containsKey('rx_packets')) {
        _rxPackets = (map['rx_packets'] as num?)?.toInt() ?? 0;
      }
      if (map.containsKey('tx_packets')) {
        _txPackets = (map['tx_packets'] as num?)?.toInt() ?? 0;
      }
    } catch (e) {
      _lastError = e.toString();
    }
  }
}
