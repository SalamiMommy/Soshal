// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// Nostr Protocol Service
/// Manages relay pool lifecycle, subscriptions, event publishing, and relay persistence.
/// Wraps the 8 `nostr_*` FFI functions exposed by network-core.
class NostrService extends ChangeNotifier {
  bool _initialized = false;
  List<String> _relayUrls = [];
  String? _lastError;

  bool get initialized => _initialized;
  List<String> get relayUrls => _relayUrls;
  int get relayCount => _relayUrls.length;
  String? get lastError => _lastError;

  /// Initialize relay pool with default or cached relays.
  /// If no [urls] provided, uses cached relays or falls back to damus.io, nos.lol, nostr.wine.
  Future<void> initRelays({List<String> urls = const []}) async {
    try {
      await RustLib.instance.api
          .crateFfiNetworkNetworkInitRelays(relayUrls: urls);
      _lastError = null;
      _initialized = true;
      await refreshStatus();
      notifyListeners();
    } catch (e) {
      _lastError = e.toString();
      _initialized = false;
      notifyListeners();
      rethrow;
    }
  }

  /// Add a relay to the relay pool.
  Future<bool> addRelay(String url) async {
    try {
      final ok =
          await RustLib.instance.api.crateFfiNetworkNetworkAddRelay(url: url);
      _lastError = null;
      if (ok && !_relayUrls.contains(url)) {
        _relayUrls.add(url);
      }
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Remove a relay from the relay pool.
  Future<bool> removeRelay(String url) async {
    try {
      final ok = await RustLib.instance.api
          .crateFfiNetworkNetworkRemoveRelay(url: url);
      _lastError = null;
      if (ok) {
        _relayUrls.removeWhere((u) => u == url);
      }
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Subscribe to events matching a filter. Returns subscription ID.
  /// Filter JSON format: `{"kinds": [1], "authors": ["pubkey"], "limit": 100}`
  Future<String?> subscribe(String filterJson) async {
    try {
      final subId = await RustLib.instance.api
          .crateFfiNetworkNetworkSubscribe(filterJson: filterJson);
      _lastError = null;
      return subId;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Unsubscribe from a subscription.
  Future<bool> unsubscribe(String subscriptionId) async {
    try {
      final ok = await RustLib.instance.api
          .crateFfiNetworkNetworkUnsubscribe(subscriptionId: subscriptionId);
      _lastError = null;
      return ok;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Publish a pre-signed event to all relays.
  /// Event JSON must include: id, pubkey, created_at, kind, content, tags, sig.
  /// Returns number of relays that accepted the event.
  Future<int> publishEvent(String eventJson) async {
    try {
      final count = await RustLib.instance.api
          .crateFfiNetworkNetworkPublishEvent(eventJson: eventJson);
      _lastError = null;
      return count;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Query events matching a filter. Returns array of event JSON.
  /// Filter JSON format: `{"kinds": [1], "authors": ["pubkey"], "limit": 100}`
  Future<List<dynamic>?> queryEvents(String filterJson) async {
    try {
      final result = await RustLib.instance.api
          .crateFfiNetworkNetworkQueryEvents(filterJson: filterJson);
      _lastError = null;

      final decoded = jsonDecode(result) as List<dynamic>;
      return decoded;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Refresh relay connection status.
  Future<void> refreshStatus() async {
    try {
      final statusJson =
          await RustLib.instance.api.crateFfiNetworkNetworkGetRelayStatus();
      _lastError = null;

      final statuses = jsonDecode(statusJson) as List<dynamic>;
      final urls = <String>[];
      for (final status in statuses) {
        if (status is Map<String, dynamic>) {
          final url = status['url'] as String?;
          if (url != null) {
            urls.add(url);
          }
        }
      }
      _relayUrls = urls;
      notifyListeners();
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Get current relay connection status. Returns JSON with url + connected state per relay.
  Future<String?> getStatus() async {
    try {
      final statusJson =
          await RustLib.instance.api.crateFfiNetworkNetworkGetRelayStatus();
      _lastError = null;
      return statusJson;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }
}
