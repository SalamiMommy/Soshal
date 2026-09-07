// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import 'social_entry.dart';
import '../utils/service_guard.dart';

/// Vouch Service
/// Kind-31989 web-of-trust vouches, signed in-process and relay-published;
/// fetched vouches are signature-verified.
class VouchService extends ChangeNotifier with LastErrorMixin, ServiceGuard {
  List<VouchEntry> _vouches = [];

  List<VouchEntry> get vouches => _vouches;

  /// Publish a vouch for `targetPubkey`. Returns the event id.
  Future<String> publish(String targetPubkey, String content) => guard(() {
        return RustLib.instance.api.crateFfiVouchVouchPublish(
          targetPubkey: targetPubkey,
          content: content,
        );
      }, notifyOnSuccess: false);

  /// Fetch verified vouches addressed to `targetPubkey`.
  Future<List<VouchEntry>> fetch(String targetPubkey) => guard(() async {
        final json = await RustLib.instance.api.crateFfiVouchVouchFetch(
          targetPubkey: targetPubkey,
        );
        final decoded = jsonDecode(json);
        _vouches = (decoded as List<dynamic>)
            .map((e) => VouchEntry.fromJson(e as Map<String, dynamic>))
            .toList();
        return _vouches;
      });
}

/// A verified vouch entry for a target pubkey.
class VouchEntry extends SocialEntry {
  VouchEntry({
    required super.id,
    required super.pubkey,
    required super.content,
    required super.createdAt,
  });

  factory VouchEntry.fromJson(Map<String, dynamic> json) {
    final base = SocialEntry.fromJson(json);
    return VouchEntry(
      id: base.id,
      pubkey: base.pubkey,
      content: base.content,
      createdAt: base.createdAt,
    );
  }
}
