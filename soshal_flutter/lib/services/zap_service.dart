// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Zap Service
/// NIP-57 zaps via a NWC connection: connect, receipts and totals.
class ZapService extends ChangeNotifier with LastErrorMixin {
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
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
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
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
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
      _lastError = null;
      notifyListeners();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
    }
  }

  /// Total msats zapped to an event.
  Future<int> fetchTotalMsat(String eventId) async {
    try {
      final msat = await RustLib.instance.api.crateFfiZapZapGetTotalMsat(
        eventId: eventId,
      );
      _totalMsat = msat.toInt();
      _lastError = null;
      notifyListeners();
      return _totalMsat;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
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
      _lastError = null;
      notifyListeners();
      return _receipts;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Parse LNURL metadata for a zapper address.
  Future<String> parseLnurl(String lnurl) async {
    try {
      final json = await RustLib.instance.api
          .crateFfiZapZapParseLnurlMetadata(lnurl: lnurl);
      _lastError = null;
      return json;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
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
      id: json['id'] as String? ?? '',
      eventId: json['event_id'] as String? ?? '',
      zapperPubkey: json['zapper_pubkey'] ?? json['pubkey'] ?? '',
      amountMsat: (json['amount_msat'] as num?)?.toInt() ?? 0,
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
    );
  }
}
