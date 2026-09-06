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

  String? get nwcStatus => _nwcStatus;
  String? get nwcPubkey => _nwcPubkey;
  int get totalMsat => _totalMsat;
  List<ZapReceipt> get receipts => _receipts;

  bool get isConnected =>
      (_nwcStatus ?? '').isNotEmpty && (_nwcStatus ?? '') != 'disconnected';

  /// Connect to a Nostr Wallet Connect URI.
  Future<bool> connect(String nwcUri) =>
      guard(() async {
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
  Future<int> fetchTotalMsat(String eventId) => guard(() async {
        final msat = await RustLib.instance.api.crateFfiZapZapGetTotalMsat(
          eventId: eventId,
        );
        _totalMsat = msat.toInt();
        return _totalMsat;
      }, onNotify: notifyDeferred);

  /// Total msats zapped to many events, keyed by event id.
  Future<Map<String, int>> fetchTotals(List<String> eventIds) async {
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
  Future<List<ZapReceipt>> fetchReceipts(String eventId,
        {int limit = 20}) =>
    guard(() async {
      final json = await RustLib.instance.api.crateFfiZapZapFetchReceipts(
        eventId: eventId,
        limit: limit,
      );
      final decoded = jsonDecode(json);
      _receipts = (decoded as List<dynamic>)
          .map((e) => ZapReceipt.fromJson(e as Map<String, dynamic>))
          .toList();
      return _receipts;
    }, onNotify: notifyDeferred);

  /// Parse LNURL metadata for a zapper address.
  Future<String> parseLnurl(String lnurl) =>
      guard(() {
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
  Future<String> sendPayment(String bolt11) =>
      guard(() {
        return RustLib.instance.api.crateFfiZapZapSendPayment(bolt11: bolt11);
      }, notifyOnSuccess: false, onNotify: notifyDeferred);
}

/// A zap receipt row.
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
