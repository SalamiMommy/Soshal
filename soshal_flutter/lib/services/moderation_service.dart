// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Moderation Service
/// Mute/block lists and word filters, backed by the Rust moderation core.
class ModerationService extends ChangeNotifier with LastErrorMixin {
  Set<String> _muted = {};
  Set<String> _blocked = {};
  List<String> _wordFilters = [];

  List<String> get muted => _muted.toList();
  List<String> get blocked => _blocked.toList();
  List<String> get wordFilters => _wordFilters;

  bool isBlocked(String pubkey) => _blocked.contains(pubkey);
  bool isMuted(String pubkey) => _muted.contains(pubkey);

  /// Load muted users, blocked users and word filters.
  Future<void> load(String pubkey) async {
    try {
      _muted = Set.from(
        RustLib.instance.api.crateFfiModerationModerationGetMuted(
          userPubkey: pubkey,
        ),
      );
      _blocked = Set.from(
        RustLib.instance.api.crateFfiModerationModerationGetBlocked(
          userPubkey: pubkey,
        ),
      );
      _wordFilters =
          RustLib.instance.api.crateFfiModerationModerationGetWordFilters();
      clearLastError();
      notifyListeners();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
    }
  }

  /// Block a user.
  Future<bool> block(String blockerPubkey, String targetPubkey) async {
    final ok = RustLib.instance.api.crateFfiModerationModerationBlockUser(
      blockerPubkey: blockerPubkey,
      targetPubkey: targetPubkey,
    );
    if (ok && !_blocked.contains(targetPubkey)) {
      _blocked.add(targetPubkey);
    }
    notifyListeners();
    return ok;
  }

  /// Unblock a user.
  Future<bool> unblock(String blockerPubkey, String targetPubkey) async {
    final ok = RustLib.instance.api.crateFfiModerationModerationUnblockUser(
      blockerPubkey: blockerPubkey,
      targetPubkey: targetPubkey,
    );
    if (ok) {
      _blocked.remove(targetPubkey);
    }
    notifyListeners();
    return ok;
  }

  /// File a report on content (recorded locally for later moderation sync).
  Future<bool> reportContent({
    required String reporterPubkey,
    required String contentType,
    required String contentId,
    required String reason,
  }) async {
    final ok = RustLib.instance.api.crateFfiModerationModerationReportContent(
      reporterPubkey: reporterPubkey,
      contentType: contentType,
      contentId: contentId,
      reason: reason,
    );
    notifyListeners();
    return ok;
  }

  /// List spam reports for a target pubkey (newest first).
  Future<List<dynamic>> listReports(String targetPubkey,
      {int limit = 10}) async {
    try {
      final json = RustLib.instance.api.crateFfiModerationModerationListReports(
        targetPubkey: targetPubkey,
        limit: limit,
      );
      return jsonDecode(json) as List<dynamic>;
    } catch (e, st) {
      setLastError(e, st);
      return [];
    }
  }

  /// Delete a spam report by id.
  Future<bool> deleteReport(String reportId) async {
    try {
      return RustLib.instance.api.crateFfiModerationModerationDeleteReport(
        reportId: reportId,
      );
    } catch (e, st) {
      setLastError(e, st);
      return false;
    }
  }

  /// Mute a user.
  Future<bool> mute(String muterPubkey, String targetPubkey) async {
    final ok = RustLib.instance.api.crateFfiModerationModerationMuteUser(
      muterPubkey: muterPubkey,
      targetPubkey: targetPubkey,
    );
    if (ok && !_muted.contains(targetPubkey)) {
      _muted.add(targetPubkey);
    }
    notifyListeners();
    return ok;
  }

  /// Unmute a user.
  Future<bool> unmute(String muterPubkey, String targetPubkey) async {
    final ok = RustLib.instance.api.crateFfiModerationModerationUnmuteUser(
      muterPubkey: muterPubkey,
      targetPubkey: targetPubkey,
    );
    if (ok) {
      _muted.remove(targetPubkey);
    }
    notifyListeners();
    return ok;
  }

  /// Replace the word-filter list.
  Future<bool> setWordFilters(List<String> filters) async {
    final ok = RustLib.instance.api.crateFfiModerationModerationSetWordFilters(
      filtersJson: jsonEncode(filters),
    );
    await load('');
    return ok;
  }

  /// Check whether a content string trips any filter.
  Future<bool> shouldFilter(String content, String pubkey) async {
    try {
      return RustLib.instance.api.crateFfiModerationModerationShouldFilter(
        content: content,
        userPubkey: pubkey,
      );
    } catch (e, st) {
      setLastError(e, st);
      return false;
    }
  }

  /// Whether `targetPubkey` is blocked or muted by `actorPubkey`.
  bool isRestricted(String actorPubkey, String targetPubkey) {
    try {
      return RustLib.instance.api.crateFfiModerationModerationIsRestricted(
        actorPubkey: actorPubkey,
        targetPubkey: targetPubkey,
      );
    } catch (e, st) {
      setLastError(e, st);
      return false;
    }
  }

  /// Create a FROST threshold jury case for community moderation. Returns
  /// the serialized `ModerationJuryCase` JSON to feed back into
  /// `submitJuryVote`.
  Future<String> createJuryCase({
    required String caseId,
    required String targetPubkey,
    required String reason,
    required int threshold,
    required int totalJurors,
    required String groupPubkey,
  }) async {
    try {
      final json =
          RustLib.instance.api.crateFfiModerationModerationCreateJuryCase(
        caseId: caseId,
        targetPubkey: targetPubkey,
        reason: reason,
        threshold: threshold,
        totalJurors: totalJurors,
        groupPubkey: groupPubkey,
      );
      clearLastError();
      return json;
    } catch (e, st) {
      setLastError(e, st);
      rethrow;
    }
  }

  /// Submit a juror's partial-signature vote to a moderation jury case.
  /// `caseJson` is the JSON returned by `createJuryCase`; `voteShareJson` a
  /// serialized `FrostSignatureShare`. Returns verdict status JSON.
  Future<String> submitJuryVote({
    required String caseJson,
    required String voteShareJson,
  }) async {
    try {
      final json =
          RustLib.instance.api.crateFfiModerationModerationSubmitJuryVote(
        caseJson: caseJson,
        voteShareJson: voteShareJson,
      );
      clearLastError();
      return json;
    } catch (e, st) {
      setLastError(e, st);
      rethrow;
    }
  }
}
