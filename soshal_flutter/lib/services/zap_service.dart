import '../utils/json_ext.dart';
// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Zap Service
/// NIP-57 zaps via a NWC connection: connect, receipts and totals.
class ZapService extends ChangeNotifier with LastErrorMixin, DeferredNotify {
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
  Future<bool> connect(String nwcUri) async {
    try {
      final ok =
          await RustLib.instance.api.crateFfiZapZapConnectNwc(nwcUri: nwcUri);
      if (ok) {
        await refreshStatus();
      }
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Disconnect from NWC.
  Future<bool> disconnect() async {
    try {
      final ok = await RustLib.instance.api.crateFfiZapZapDisconnectNwc();
      if (ok) {
        _nwcStatus = 'disconnected';
        _nwcPubkey = null;
      }
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

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
  Future<int> fetchTotalMsat(String eventId) async {
    try {
      final msat = await RustLib.instance.api.crateFfiZapZapGetTotalMsat(
        eventId: eventId,
      );
      _totalMsat = msat.toInt();
      clearLastError();
      notifyDeferred();
      return _totalMsat;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

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
      {int limit = 20}) async {
    try {
      final json = await RustLib.instance.api.crateFfiZapZapFetchReceipts(
        eventId: eventId,
        limit: limit,
      );
      final decoded = jsonDecode(json);
      _receipts = (decoded as List<dynamic>)
          .map((e) => ZapReceipt.fromJson(e as Map<String, dynamic>))
          .toList();
      clearLastError();
      notifyDeferred();
      return _receipts;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Parse LNURL metadata for a zapper address.
  Future<String> parseLnurl(String lnurl) async {
    try {
      final json = await RustLib.instance.api
          .crateFfiZapZapParseLnurlMetadata(lnurl: lnurl);
      clearLastError();
      return json;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Fetch a BOLT-11 invoice for a zap via the connected NWC provider.
  /// Returns the serialized invoice JSON (`bolt11`, `amount_msat`, …).
  Future<String> fetchInvoice({
    required String lnurl,
    required int amountMsat,
    String comment = '',
    String nostrEvent = '',
  }) async {
    try {
      final json = await RustLib.instance.api.crateFfiZapZapFetchInvoice(
        lnurl: lnurl,
        amountMsat: BigInt.from(amountMsat),
        comment: comment,
        nostrEvent: nostrEvent,
      );
      clearLastError();
      return json;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Pay a BOLT-11 invoice via the connected NWC provider. Returns the
  /// serialized pay_invoice response (payment preimage).
  Future<String> sendPayment(String bolt11) async {
    try {
      final json =
          await RustLib.instance.api.crateFfiZapZapSendPayment(bolt11: bolt11);
      clearLastError();
      return json;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }
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
