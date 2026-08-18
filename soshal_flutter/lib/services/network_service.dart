import '../utils/json_ext.dart';
// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';
import 'dart:io';
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/ffi/network.dart';
import 'package:soshal_flutter/ffi/p2p.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Network Service
/// Transport status: I2P + Freenet presence + HTTP/3 stack + transport mode.
class NetworkService extends ChangeNotifier with LastErrorMixin {
  static const String _modeKey = 'transport_mode';

  /// Relays used when none are configured for the active account yet.
  /// Initialized at app bootstrap so relay-gated features (chatrandom,
  /// musicloud, …) work before sign-in.
  static const List<String> defaultRelays = [
    'wss://relay.nostr.band',
    'wss://nos.lol',
  ];

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
        clearLastError();
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
      clearLastError();
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
      clearLastError();
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
      clearLastError();
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
      clearLastError();
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
      clearLastError();
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
      clearLastError();
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
      clearLastError();
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
      clearLastError();
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
      clearLastError();
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
      clearLastError();
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
      clearLastError();
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
      clearLastError();
      notifyListeners();
      return res;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return {'success': false, 'error_msg': e.toString()};
    }
  }

  /// Connects to a Freenet gateway (external-infra; honest label in UI).
  Future<bool> freenetConnect({
    required String url,
    required String authToken,
  }) async {
    try {
      final ok = await RustLib.instance.api.crateFfiNetworkFreenetConnect(
        url: url,
        authToken: authToken,
      );
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Fetches a Freenet contract state for `key`.
  Future<String> freenetGetContract({
    required String url,
    required String authToken,
    required String key,
    required bool subscribe,
  }) async {
    try {
      final json = await RustLib.instance.api.crateFfiNetworkFreenetGetContract(
        url: url,
        authToken: authToken,
        key: key,
        subscribe: subscribe,
      );
      clearLastError();
      notifyListeners();
      return json;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Puts a contract state (JSON) onto Freenet.
  Future<String> freenetPutContract({
    required String url,
    required String authToken,
    required String stateJson,
    required bool subscribe,
  }) async {
    try {
      final json = await RustLib.instance.api.crateFfiNetworkFreenetPutContract(
        url: url,
        authToken: authToken,
        stateJson: stateJson,
        subscribe: subscribe,
      );
      clearLastError();
      notifyListeners();
      return json;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Subscribes to a Freenet contract key.
  Future<bool> freenetSubscribe({
    required String url,
    required String authToken,
    required String key,
    String? summaryJson,
  }) async {
    try {
      final ok = await RustLib.instance.api.crateFfiNetworkFreenetSubscribe(
        url: url,
        authToken: authToken,
        key: key,
        summaryJson: summaryJson,
      );
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Raw SAM connect to a local i2pd daemon (sync FFI).
  bool i2pConnect({required String samHost, required int samPort}) {
    try {
      final ok = RustLib.instance.api
          .crateFfiNetworkI2PConnect(samHost: samHost, samPort: samPort);
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Creates an I2P SAM session; returns the session destination.
  String i2pCreateSession({
    required String samHost,
    required int samPort,
    required String sessionId,
    String? destination,
  }) {
    try {
      final dest = RustLib.instance.api.crateFfiNetworkI2PCreateSession(
        samHost: samHost,
        samPort: samPort,
        sessionId: sessionId,
        destination: destination,
      );
      clearLastError();
      notifyListeners();
      return dest;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Generates a fresh I2P destination via SAM.
  String i2pGenerateDestination({
    required String samHost,
    required int samPort,
  }) {
    try {
      final dest = RustLib.instance.api.crateFfiNetworkI2PGenerateDestination(
        samHost: samHost,
        samPort: samPort,
      );
      clearLastError();
      notifyListeners();
      return dest;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Opens a SAM tunnel to a remote I2P destination.
  bool i2pConnectToDestination({
    required String samHost,
    required int samPort,
    required String sessionId,
    required String destination,
  }) {
    try {
      final ok = RustLib.instance.api.crateFfiNetworkI2PConnectToDestination(
        samHost: samHost,
        samPort: samPort,
        sessionId: sessionId,
        destination: destination,
      );
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Subscribes the relay client to a raw NIP-01 filter; returns the
  /// subscription id.
  Future<String> relaySubscribe({required String filterJson}) async {
    try {
      final id = await RustLib.instance.api
          .crateFfiNetworkNetworkSubscribe(filterJson: filterJson);
      clearLastError();
      notifyListeners();
      return id;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Unsubscribes a raw relay subscription by id.
  Future<bool> relayUnsubscribe({required String subscriptionId}) async {
    try {
      final ok = await RustLib.instance.api
          .crateFfiNetworkNetworkUnsubscribe(subscriptionId: subscriptionId);
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Publishes a raw signed event JSON to the relay client.
  Future<int> publishEvent({required String eventJson}) async {
    try {
      final id = await RustLib.instance.api.crateFfiNetworkNetworkPublishEvent(
        eventJson: eventJson,
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

  /// Queries relays with a raw NIP-01 filter JSON; returns events JSON.
  Future<String> queryEvents({required String filterJson}) async {
    try {
      final json = await RustLib.instance.api
          .crateFfiNetworkNetworkQueryEvents(filterJson: filterJson);
      clearLastError();
      notifyListeners();
      return json;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Ingests a BLE beacon announcement for off-grid mesh sync.
  Future<String?> processBleBeacon({
    required String beacon,
    required String localRootHex,
    required String ownPubkey,
  }) async {
    try {
      final result =
          await RustLib.instance.api.crateFfiNetworkNetworkProcessBleBeacon(
        beacon: beacon,
        localRootHex: localRootHex,
        ownPubkey: ownPubkey,
      );
      clearLastError();
      notifyListeners();
      return result;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Verifies a ZK WoT proof commitment (experimental).
  Future<bool> verifyZkWotProof({
    required String proofJson,
    required String expectedWotRoot,
    required String blacklistedNullifiersJson,
  }) async {
    try {
      final ok =
          await RustLib.instance.api.crateFfiNetworkNetworkVerifyZkWotProof(
        proofJson: proofJson,
        expectedWotRoot: expectedWotRoot,
        blacklistedNullifiersJson: blacklistedNullifiersJson,
      );
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Notifies the Rust side that the network interface changed. Resolves the
  /// local IPv4 address and the running QUIC server port so the bridge
  /// receives a real `ip:port` SocketAddr; returns false without an error
  /// when either is unavailable.
  Future<bool> notifyInterfaceChange() async {
    final ip = await _localIpv4();
    if (ip == null) return false;
    final int port;
    try {
      port = p2PQuicServerPort();
    } catch (_) {
      return false;
    }
    try {
      final ok = RustLib.instance.api
          .crateFfiNetworkNetworkNotifyInterfaceChange(newIp: '$ip:$port');
      clearLastError();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return false;
    }
  }

  static Future<String?> _localIpv4() async {
    try {
      final interfaces = await NetworkInterface.list(
          type: InternetAddressType.IPv4, includeLoopback: false);
      for (final iface in interfaces) {
        for (final addr in iface.addresses) {
          if (addr.isLoopback) continue;
          return addr.address;
        }
      }
    } catch (_) {}
    return null;
  }

  /// Stops the Reticulum transport (sync FFI).
  bool reticulumStop() {
    try {
      final ok = RustLib.instance.api.crateFfiNetworkNetworkReticulumStop();
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Reticulum transport status JSON (sync FFI).
  String reticulumStatus() {
    try {
      final json = RustLib.instance.api.crateFfiNetworkNetworkReticulumStatus();
      clearLastError();
      notifyListeners();
      return json;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Sends a Reticulum announce for `pubkey` (sync FFI).
  bool reticulumAnnounce({required String pubkey}) {
    try {
      final ok = RustLib.instance.api
          .crateFfiNetworkNetworkReticulumAnnounce(pubkey: pubkey);
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Publishes the account relay list as a kind-10002 event (sync FFI).
  String publishRelayList({required List<String> relayUrls}) {
    try {
      final result = RustLib.instance.api
          .crateFfiIdentityIdentityPublishRelayList(relayUrls: relayUrls);
      clearLastError();
      return result;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Resolves protocol metadata (NIP-05 / NIP-19 style URI parts).
  Future<String> fetchProtocolMetadata({
    required String scheme,
    required String host,
    required String path,
  }) async {
    try {
      final json =
          await RustLib.instance.api.crateFfiProtocolHandlerProtocolGetMetadata(
        scheme: scheme,
        host: host,
        path: path,
      );
      clearLastError();
      return json;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
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
      url: json.strOf('url'),
      connected: json.boolOf('connected'),
      latencyMs: json.intOf('latency_ms'),
      lastEventAt: json.intOf('last_event_at'),
    );
  }
}
