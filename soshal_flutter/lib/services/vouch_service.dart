// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Vouch Service
/// Kind-31989 web-of-trust vouches, signed in-process and relay-published;
/// fetched vouches are signature-verified.
class VouchService extends ChangeNotifier with LastErrorMixin {
  List<VouchEntry> _vouches = [];

  List<VouchEntry> get vouches => _vouches;

  /// Publish a vouch for `targetPubkey`. Returns the event id.
  Future<String> publish(String targetPubkey, String content) async {
    try {
      final id = await RustLib.instance.api.crateFfiVouchVouchPublish(
        targetPubkey: targetPubkey,
        content: content,
      );
      _lastError = null;
      return id;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Fetch verified vouches addressed to `targetPubkey`.
  Future<List<VouchEntry>> fetch(String targetPubkey) async {
    try {
      final json = await RustLib.instance.api.crateFfiVouchVouchFetch(
        targetPubkey: targetPubkey,
      );
      final decoded = jsonDecode(json);
      _vouches = (decoded as List<dynamic>)
          .map((e) => VouchEntry.fromJson(e as Map<String, dynamic>))
          .toList();
      _lastError = null;
      notifyListeners();
      return _vouches;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }
}

/// A verified vouch entry for a target pubkey.
class VouchEntry {
  final String id;
  final String pubkey;
  final String content;
  final int createdAt;

  VouchEntry({
    required this.id,
    required this.pubkey,
    required this.content,
    required this.createdAt,
  });

  factory VouchEntry.fromJson(Map<String, dynamic> json) {
    return VouchEntry(
      id: json['id'] as String? ?? '',
      pubkey: json['pubkey'] as String? ?? '',
      content: json['content'] as String? ?? '',
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
    );
  }
}
