// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

/// Category probabilities from the on-device AI moderation engine.
class AiCategoryScores {
  final double spam;
  final double csam;
  final double gore;
  final double bigotry;
  final double harassment;

  const AiCategoryScores({
    this.spam = 0.0,
    this.csam = 0.0,
    this.gore = 0.0,
    this.bigotry = 0.0,
    this.harassment = 0.0,
  });

  factory AiCategoryScores.fromJson(Map<String, dynamic> json) {
    return AiCategoryScores(
      spam: (json['spam'] as num?)?.toDouble() ?? 0.0,
      csam: (json['csam'] as num?)?.toDouble() ?? 0.0,
      gore: (json['gore'] as num?)?.toDouble() ?? 0.0,
      bigotry: (json['bigotry'] as num?)?.toDouble() ?? 0.0,
      harassment: (json['harassment'] as num?)?.toDouble() ?? 0.0,
    );
  }
}

/// Detailed result of evaluating text with the AI filter.
class AiModerationResult {
  final bool isFlagged;
  final String? primaryCategory;
  final double confidence;
  final AiCategoryScores scores;
  final List<String> detectedReasons;
  final double evasionScore;

  const AiModerationResult({
    required this.isFlagged,
    this.primaryCategory,
    this.confidence = 0.0,
    this.scores = const AiCategoryScores(),
    this.detectedReasons = const [],
    this.evasionScore = 0.0,
  });

  factory AiModerationResult.clean() =>
      const AiModerationResult(isFlagged: false);

  factory AiModerationResult.fromJson(Map<String, dynamic> json) {
    return AiModerationResult(
      isFlagged: json['is_flagged'] as bool? ?? false,
      primaryCategory: json['primary_category'] as String?,
      confidence: (json['confidence'] as num?)?.toDouble() ?? 0.0,
      scores: json['scores'] is Map<String, dynamic>
          ? AiCategoryScores.fromJson(json['scores'] as Map<String, dynamic>)
          : const AiCategoryScores(),
      detectedReasons: (json['detected_reasons'] as List<dynamic>?)
              ?.map((e) => e.toString())
              .toList() ??
          const [],
      evasionScore: (json['evasion_score'] as num?)?.toDouble() ?? 0.0,
    );
  }
}

/// Category scores output by the RoBERTa classifier head.
class RobertaCategoryScores {
  final double toxic;
  final double severeToxic;
  final double obscene;
  final double threat;
  final double insult;
  final double identityHate;
  final double spam;
  final double csam;
  final double gore;

  const RobertaCategoryScores({
    this.toxic = 0.0,
    this.severeToxic = 0.0,
    this.obscene = 0.0,
    this.threat = 0.0,
    this.insult = 0.0,
    this.identityHate = 0.0,
    this.spam = 0.0,
    this.csam = 0.0,
    this.gore = 0.0,
  });

  factory RobertaCategoryScores.fromJson(Map<String, dynamic> json) {
    return RobertaCategoryScores(
      toxic: (json['toxic'] as num?)?.toDouble() ?? 0.0,
      severeToxic: (json['severe_toxic'] as num?)?.toDouble() ?? 0.0,
      obscene: (json['obscene'] as num?)?.toDouble() ?? 0.0,
      threat: (json['threat'] as num?)?.toDouble() ?? 0.0,
      insult: (json['insult'] as num?)?.toDouble() ?? 0.0,
      identityHate: (json['identity_hate'] as num?)?.toDouble() ?? 0.0,
      spam: (json['spam'] as num?)?.toDouble() ?? 0.0,
      csam: (json['csam'] as num?)?.toDouble() ?? 0.0,
      gore: (json['gore'] as num?)?.toDouble() ?? 0.0,
    );
  }
}

/// Result of evaluating text through RoBERTa.
class RobertaResult {
  final bool isFlagged;
  final String? primaryCategory;
  final double confidence;
  final RobertaCategoryScores scores;
  final int tokenCount;
  final List<String> detectedSignals;

  const RobertaResult({
    required this.isFlagged,
    this.primaryCategory,
    this.confidence = 0.0,
    this.scores = const RobertaCategoryScores(),
    this.tokenCount = 0,
    this.detectedSignals = const [],
  });

  factory RobertaResult.fromJson(Map<String, dynamic> json) {
    return RobertaResult(
      isFlagged: json['is_flagged'] as bool? ?? false,
      primaryCategory: json['primary_category'] as String?,
      confidence: (json['confidence'] as num?)?.toDouble() ?? 0.0,
      scores: json['scores'] is Map<String, dynamic>
          ? RobertaCategoryScores.fromJson(
              json['scores'] as Map<String, dynamic>)
          : const RobertaCategoryScores(),
      tokenCount: (json['token_count'] as num?)?.toInt() ?? 0,
      detectedSignals: (json['detected_signals'] as List<dynamic>?)
              ?.map((e) => e.toString())
              .toList() ??
          const [],
    );
  }
}

/// Result of 2-Tier Hybrid Text Evaluation.
class HybridModerationResult {
  final bool isFlagged;
  final String? primaryCategory;
  final double confidence;
  final String tierEvaluated; // 'Tier1Fast' | 'Tier2Deep'
  final AiModerationResult tier1Result;
  final RobertaResult? tier2RobertaResult;
  final List<String> detectedReasons;

  const HybridModerationResult({
    required this.isFlagged,
    this.primaryCategory,
    this.confidence = 0.0,
    this.tierEvaluated = 'Tier1Fast',
    this.tier1Result = const AiModerationResult(isFlagged: false),
    this.tier2RobertaResult,
    this.detectedReasons = const [],
  });

  factory HybridModerationResult.clean() =>
      const HybridModerationResult(isFlagged: false);

  factory HybridModerationResult.fromJson(Map<String, dynamic> json) {
    return HybridModerationResult(
      isFlagged: json['is_flagged'] as bool? ?? false,
      primaryCategory: json['primary_category'] as String?,
      confidence: (json['confidence'] as num?)?.toDouble() ?? 0.0,
      tierEvaluated: json['tier_evaluated'] as String? ?? 'Tier1Fast',
      tier1Result: json['tier1_result'] is Map<String, dynamic>
          ? AiModerationResult.fromJson(
              json['tier1_result'] as Map<String, dynamic>)
          : const AiModerationResult(isFlagged: false),
      tier2RobertaResult: json['tier2_roberta_result'] is Map<String, dynamic>
          ? RobertaResult.fromJson(
              json['tier2_roberta_result'] as Map<String, dynamic>)
          : null,
      detectedReasons: (json['detected_reasons'] as List<dynamic>?)
              ?.map((e) => e.toString())
              .toList() ??
          const [],
    );
  }
}

/// Result of evaluating a media item with the AI perceptual analyzer.
class AiMediaVerdict {
  final bool passed;
  final bool isCsamHazard;
  final bool isGoreHazard;
  final bool isNsfw;
  final double exposureScore;
  final double goreScore;
  final String? warningReason;

  const AiMediaVerdict({
    required this.passed,
    this.isCsamHazard = false,
    this.isGoreHazard = false,
    this.isNsfw = false,
    this.exposureScore = 0.0,
    this.goreScore = 0.0,
    this.warningReason,
  });

  factory AiMediaVerdict.pass() => const AiMediaVerdict(passed: true);

  factory AiMediaVerdict.fromJson(Map<String, dynamic> json) {
    return AiMediaVerdict(
      passed: json['passed'] as bool? ?? true,
      isCsamHazard: json['is_csam_hazard'] as bool? ?? false,
      isGoreHazard: json['is_gore_hazard'] as bool? ?? false,
      isNsfw: json['is_nsfw'] as bool? ?? false,
      exposureScore: (json['exposure_score'] as num?)?.toDouble() ?? 0.0,
      goreScore: (json['gore_score'] as num?)?.toDouble() ?? 0.0,
      warningReason: json['warning_reason'] as String?,
    );
  }
}

/// Result of 256-bit Meta PDQ Perceptual Image Hashing.
class PdqHashResult {
  final String hashHex;
  final int quality;
  final bool isThreatMatch;
  final String? matchedCategory;
  final int? minHammingDistance;

  const PdqHashResult({
    required this.hashHex,
    this.quality = 0,
    this.isThreatMatch = false,
    this.matchedCategory,
    this.minHammingDistance,
  });

  factory PdqHashResult.fromJson(Map<String, dynamic> json) {
    return PdqHashResult(
      hashHex: json['hash_hex'] as String? ?? '',
      quality: (json['quality'] as num?)?.toInt() ?? 0,
      isThreatMatch: json['is_threat_match'] as bool? ?? false,
      matchedCategory: json['matched_category'] as String?,
      minHammingDistance: (json['min_hamming_distance'] as num?)?.toInt(),
    );
  }
}

/// Result of 2-Tier Hybrid Media Evaluation.
class HybridMediaResult {
  final bool passed;
  final AiMediaVerdict tier1Verdict;
  final PdqHashResult? tier2Pdq;
  final bool isCsamThreat;
  final bool isGoreThreat;
  final bool isNsfw;
  final String? warningReason;

  const HybridMediaResult({
    required this.passed,
    this.tier1Verdict = const AiMediaVerdict(passed: true),
    this.tier2Pdq,
    this.isCsamThreat = false,
    this.isGoreThreat = false,
    this.isNsfw = false,
    this.warningReason,
  });

  factory HybridMediaResult.pass() => const HybridMediaResult(passed: true);

  factory HybridMediaResult.fromJson(Map<String, dynamic> json) {
    return HybridMediaResult(
      passed: json['passed'] as bool? ?? true,
      tier1Verdict: json['tier1_verdict'] is Map<String, dynamic>
          ? AiMediaVerdict.fromJson(
              json['tier1_verdict'] as Map<String, dynamic>)
          : const AiMediaVerdict(passed: true),
      tier2Pdq: json['tier2_pdq'] is Map<String, dynamic>
          ? PdqHashResult.fromJson(json['tier2_pdq'] as Map<String, dynamic>)
          : null,
      isCsamThreat: json['is_csam_threat'] as bool? ?? false,
      isGoreThreat: json['is_gore_threat'] as bool? ?? false,
      isNsfw: json['is_nsfw'] as bool? ?? false,
      warningReason: json['warning_reason'] as String?,
    );
  }
}

/// Moderation Service
/// Mute/block lists, word filters, and on-device AI moderation engine.
class ModerationService extends ChangeNotifier
    with LastErrorMixin, ServiceGuard {
  Set<String> _muted = {};
  Set<String> _blocked = {};
  List<String> _wordFilters = [];

  List<String> get muted => _muted.toList();
  List<String> get blocked => _blocked.toList();
  List<String> get wordFilters => _wordFilters;

  /// Clear all account-scoped state on account switch so Account B never
  /// sees Account A's muted/blocked lists or word filters.
  void resetForAccountSwitch() {
    _muted = {};
    _blocked = {};
    _wordFilters = [];
    clearLastError();
    notifyListeners();
  }

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
    try {
      final ok = RustLib.instance.api.crateFfiModerationModerationReportContent(
        reporterPubkey: reporterPubkey,
        contentType: contentType,
        contentId: contentId,
        reason: reason,
      );
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      return false;
    }
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
    _wordFilters =
        RustLib.instance.api.crateFfiModerationModerationGetWordFilters();
    clearLastError();
    notifyListeners();
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
  /// the serialized `ModerationJuryCase` JSON (jury voting unavailable).
  Future<String> createJuryCase({
    required String caseId,
    required String targetPubkey,
    required String reason,
    required int threshold,
    required int totalJurors,
    required String groupPubkey,
  }) =>
      guard(() {
        return RustLib.instance.api.crateFfiModerationModerationCreateJuryCase(
          caseId: caseId,
          targetPubkey: targetPubkey,
          reason: reason,
          threshold: threshold,
          totalJurors: totalJurors,
          groupPubkey: groupPubkey,
        );
      }, notifyOnSuccess: false, notifyOnError: false);

  /// 2-Tier Hybrid text evaluation (Tier 1 N-Gram -> Tier 2 heuristic embeddings; real ML model on roadmap).
  Future<HybridModerationResult> hybridClassifyText(String content,
          {bool forceDeepScan = false}) =>
      guard(() {
        final json =
            RustLib.instance.api.crateFfiModerationModerationHybridClassifyText(
          content: content,
          forceDeepScan: forceDeepScan,
        );
        final map = jsonDecode(json) as Map<String, dynamic>;
        return HybridModerationResult.fromJson(map);
      }, notifyOnSuccess: false, notifyOnError: false);

  /// Classify text using the lightweight AI moderation engine (Spam, CSAM, Gore, Bigotry, Harassment).
  Future<AiModerationResult> aiClassifyText(String content) => guard(() {
        final json =
            RustLib.instance.api.crateFfiModerationModerationAiClassifyText(
          content: content,
        );
        final map = jsonDecode(json) as Map<String, dynamic>;
        return AiModerationResult.fromJson(map);
      }, notifyOnSuccess: false, notifyOnError: false);

  /// Classify raw media bytes with the AI media perceptual and chrominance analyzer.
  Future<AiMediaVerdict> aiClassifyMedia(
          Uint8List imageBytes, String mimeType) =>
      guard(() {
        final json =
            RustLib.instance.api.crateFfiModerationModerationAiClassifyMedia(
          imageBytes: imageBytes,
          mimeType: mimeType,
        );
        final map = jsonDecode(json) as Map<String, dynamic>;
        return AiMediaVerdict.fromJson(map);
      }, notifyOnSuccess: false, notifyOnError: false);

  /// Compute 256-bit Meta PDQ perceptual image hash and evaluate against threat blocklist.
  Future<PdqHashResult?> computePdqHash(Uint8List imageBytes) async {
    try {
      final json =
          RustLib.instance.api.crateFfiModerationModerationComputePdqHash(
        imageBytes: imageBytes,
      );
      final map = jsonDecode(json) as Map<String, dynamic>;
      clearLastError();
      return PdqHashResult.fromJson(map);
    } catch (e, st) {
      setLastError(e, st);
      return null;
    }
  }
}
