import '../utils/json_ext.dart';
// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

/// Zap Service
/// NIP-57 zaps via a NWC connection: connect, receipts and totals.
class ZapService extends ChangeNotifier
    with LastErrorMixin, DeferredNotify, ServiceGuard {
  String? _nwcStatus;
  String? _nwcPubkey;
  int _totalMsat = 0;
  List<ZapReceipt> _receipts = [];

  /// Per-id in-flight map, so two callers asking for the same event's total
  /// issue one `zap_get_total_msat` rather than two.
  final Map<String, Future<int>> _totalInFlight = {};

  /// The [fetchTotals] batch currently in the air, if any. A single-id
  /// [fetchTotalMsat] for an id inside that batch joins it instead of issuing
  /// its own round-trip. The feed fires one of those per card while the page's
  /// batch is still in flight, and every one of them was previously thrown
  /// away the moment the batch landed.
  _TotalsBatch? _pendingBatch;

  String? get nwcStatus => _nwcStatus;
  String? get nwcPubkey => _nwcPubkey;
  int get totalMsat => _totalMsat;
  List<ZapReceipt> get receipts => _receipts;

  bool get isConnected =>
      (_nwcStatus ?? '').isNotEmpty && (_nwcStatus ?? '') != 'disconnected';

  /// Clear wallet connection + cached receipts on account switch so Account B
  /// never inherits Account A's NWC connection or payment capability.
  void resetForAccountSwitch() {
    disconnect();
    _totalMsat = 0;
    _receipts.clear();
    _totalInFlight.clear();
    _pendingBatch = null;
    clearLastError();
    notifyDeferred();
  }

  /// Connect to a Nostr Wallet Connect URI.
  Future<bool> connect(String nwcUri) => guard(() async {
        final ok =
            await RustLib.instance.api.crateFfiZapZapConnectNwc(nwcUri: nwcUri);
        if (ok) {
          await refreshStatus();
        }
        return ok;
      }, onNotify: notifyDeferred);

  /// Disconnect from NWC.
  Future<bool> disconnect() => guard(() async {
        final ok = await RustLib.instance.api.crateFfiZapZapDisconnectNwc();
        if (ok) {
          _nwcStatus = 'disconnected';
          _nwcPubkey = null;
        }
        return ok;
      }, onNotify: notifyDeferred);

  /// Refresh NWC status + pubkey.
  Future<void> refreshStatus() async {
    try {
      _nwcStatus = await RustLib.instance.api.crateFfiZapZapGetNwcStatus();
      try {
        _nwcPubkey = await RustLib.instance.api.crateFfiZapZapGetNwcPubkey();
      } catch (_) {
        _nwcPubkey = null;
      }
      clearLastError();
      notifyDeferred();
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
    }
  }

  /// Total msats zapped to an event.
  ///
  /// Coalesced two ways: onto an in-flight batch that already covers this id
  /// (see [_pendingBatch]), and onto an in-flight single-id call for the same
  /// id. Only a genuinely uncovered id pays its own FFI round-trip.
  Future<int> fetchTotalMsat(String eventId) {
    final batch = _pendingBatch;
    if (batch != null && batch.ids.contains(eventId)) {
      // The batch never rejects — [fetchTotals] absorbs its own errors and
      // resolves to an empty map — so this join cannot fail where the direct
      // call would have.
      return batch.result.then((totals) => _recordTotal(
            eventId,
            totals[eventId] ?? 0,
          ));
    }
    final inFlight = _totalInFlight[eventId];
    if (inFlight != null) return inFlight;
    final future = _fetchTotalMsatUncoalesced(eventId);
    _totalInFlight[eventId] = future;
    future.then((_) {
      _totalInFlight.remove(eventId);
    }, onError: (_) {
      // Drop the entry on failure too, or a rejected future is cached for the
      // rest of the session and every later caller replays the error.
      _totalInFlight.remove(eventId);
    });
    return future;
  }

  int _recordTotal(String eventId, int msat) {
    _totalMsat = msat;
    return _totalMsat;
  }

  Future<int> _fetchTotalMsatUncoalesced(String eventId) => guard(() async {
        final msat = await RustLib.instance.api.crateFfiZapZapGetTotalMsat(
          eventId: eventId,
        );
        return _recordTotal(eventId, msat.toInt());
      }, onNotify: notifyDeferred);

  /// Total msats zapped to many events, keyed by event id.
  Future<Map<String, int>> fetchTotals(List<String> eventIds) {
    final batch = _TotalsBatch(eventIds.toSet());
    _pendingBatch = batch;
    final future = _fetchTotals(eventIds);
    batch.result = future;
    future.then((_) {
      if (identical(_pendingBatch, batch)) _pendingBatch = null;
    }, onError: (_) {
      if (identical(_pendingBatch, batch)) _pendingBatch = null;
    });
    return future;
  }

  Future<Map<String, int>> _fetchTotals(List<String> eventIds) async {
    try {
      final json = RustLib.instance.api.crateFfiZapZapFetchTotals(
        eventIds: eventIds,
      );
      final decoded = jsonDecode(json) as Map<String, dynamic>;
      clearLastError();
      return decoded.map((k, v) => MapEntry(k, (v as num).toInt()));
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return {};
    }
  }

  /// Recent zap receipts for an event.
  Future<List<ZapReceipt>> fetchReceipts(String eventId, {int limit = 20}) =>
      guard(() async {
        final json = await RustLib.instance.api.crateFfiZapZapFetchReceipts(
          eventId: eventId,
          limit: limit,
        );
        final decoded = jsonDecode(json);
        final items = decoded as List<dynamic>;
        _receipts = List<ZapReceipt>.generate(
          items.length,
          (i) => ZapReceipt.fromJson(items[i] as Map<String, dynamic>),
          growable: true,
        );
        return _receipts;
      }, onNotify: notifyDeferred);

  /// Parse LNURL metadata for a zapper address.
  Future<String> parseLnurl(String lnurl) => guard(() {
        return RustLib.instance.api
            .crateFfiZapZapParseLnurlMetadata(lnurl: lnurl);
      }, notifyOnSuccess: false, onNotify: notifyDeferred);

  /// Fetch a BOLT-11 invoice for a zap via the connected NWC provider.
  /// Returns the serialized invoice JSON (`bolt11`, `amount_msat`, …).
  Future<String> fetchInvoice({
    required String lnurl,
    required int amountMsat,
    String comment = '',
    String nostrEvent = '',
  }) =>
      guard(() {
        return RustLib.instance.api.crateFfiZapZapFetchInvoice(
          lnurl: lnurl,
          amountMsat: BigInt.from(amountMsat),
          comment: comment,
          nostrEvent: nostrEvent,
        );
      }, notifyOnSuccess: false, onNotify: notifyDeferred);

  /// Pay a BOLT-11 invoice via the connected NWC provider. Returns the
  /// serialized pay_invoice response (payment preimage).
  Future<String> sendPayment(String bolt11) => guard(() {
        return RustLib.instance.api.crateFfiZapZapSendPayment(bolt11: bolt11);
      }, notifyOnSuccess: false, onNotify: notifyDeferred);
}

/// A zap receipt row.
/// A [ZapService.fetchTotals] batch that is currently in the air, so
/// single-id lookups for ids it covers can join it instead of duplicating the
/// query. [result] is assigned by [ZapService.fetchTotals] immediately after
/// construction, before the instance is reachable from [_pendingBatch].
class _TotalsBatch {
  _TotalsBatch(this.ids);

  /// Ids this batch covers. A single-id request for an id outside this set is
  /// not answered by the batch and falls through to its own FFI call.
  final Set<String> ids;

  late final Future<Map<String, int>> result;
}

class ZapReceipt {
  final String id;
  final String eventId;
  final String zapperPubkey;
  final int amountMsat;
  final int createdAt;

  ZapReceipt({
    required this.id,
    required this.eventId,
    required this.zapperPubkey,
    required this.amountMsat,
    required this.createdAt,
  });

  factory ZapReceipt.fromJson(Map<String, dynamic> json) {
    return ZapReceipt(
      id: json.strOf('id'),
      eventId: json.strOf('event_id'),
      zapperPubkey: json['zapper_pubkey'] ?? json['pubkey'] ?? '',
      amountMsat: json.intOf('amount_msat'),
      createdAt: json.intOf('created_at'),
    );
  }
}
