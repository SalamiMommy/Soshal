import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/moderation_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-moderation');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('ModerationService', () {
    test('load populates muted, blocked and word filters', () async {
      final mod = ModerationService();
      var notified = 0;
      mod.addListener(() => notified++);
      api.stubListString('crateFfiModerationModerationGetMuted',
          const ['pk-m1', 'pk-m2']);
      api.stubListString(
          'crateFfiModerationModerationGetBlocked', const ['pk-b1']);
      api.stubListString('crateFfiModerationModerationGetWordFilters',
          const ['spam', 'scam']);

      await mod.load('pk-me');
      expect(mod.muted, ['pk-m1', 'pk-m2']);
      expect(mod.blocked, ['pk-b1']);
      expect(mod.wordFilters, ['spam', 'scam']);
      expect(mod.isMuted('pk-m1'), isTrue);
      expect(mod.isBlocked('pk-b1'), isTrue);
      expect(mod.isBlocked('pk-nobody'), isFalse);
      expect(mod.lastError, isNull);
      expect(notified, 1);

      final inv = api.callsOf('crateFfiModerationModerationGetMuted').single;
      expect(api.namedArg(inv, 'userPubkey'), 'pk-me');
      expect(api.callCount('crateFfiModerationModerationGetBlocked'), 1);
    });

    test('block and unblock update state with correct args', () async {
      final mod = ModerationService();
      api.stubBool('crateFfiModerationModerationBlockUser', true);
      api.stubBool('crateFfiModerationModerationUnblockUser', true);

      expect(await mod.block('pk-me', 'pk-bad'), isTrue);
      expect(mod.isBlocked('pk-bad'), isTrue);

      expect(await mod.unblock('pk-me', 'pk-bad'), isTrue);
      expect(mod.isBlocked('pk-bad'), isFalse);

      final blockInv =
          api.callsOf('crateFfiModerationModerationBlockUser').single;
      expect(api.namedArg(blockInv, 'blockerPubkey'), 'pk-me');
      expect(api.namedArg(blockInv, 'targetPubkey'), 'pk-bad');
      final unblockInv =
          api.callsOf('crateFfiModerationModerationUnblockUser').single;
      expect(api.namedArg(unblockInv, 'blockerPubkey'), 'pk-me');
      expect(api.namedArg(unblockInv, 'targetPubkey'), 'pk-bad');
    });

    test('mute and unmute update state', () async {
      final mod = ModerationService();
      api.stubBool('crateFfiModerationModerationMuteUser', true);
      api.stubBool('crateFfiModerationModerationUnmuteUser', true);

      await mod.mute('pk-me', 'pk-annoy');
      expect(mod.isMuted('pk-annoy'), isTrue);
      await mod.unmute('pk-me', 'pk-annoy');
      expect(mod.isMuted('pk-annoy'), isFalse);

      final muteInv =
          api.callsOf('crateFfiModerationModerationMuteUser').single;
      expect(api.namedArg(muteInv, 'muterPubkey'), 'pk-me');
      expect(api.namedArg(muteInv, 'targetPubkey'), 'pk-annoy');
    });

    test('reportContent passes content fields and returns ok', () async {
      final mod = ModerationService();
      api.stubBool('crateFfiModerationModerationReportContent', true);

      final ok = await mod.reportContent(
        reporterPubkey: 'pk-me',
        contentType: 'post',
        contentId: 'ev-1',
        reason: 'spam',
      );
      expect(ok, isTrue);
      final inv =
          api.callsOf('crateFfiModerationModerationReportContent').single;
      expect(api.namedArg(inv, 'reporterPubkey'), 'pk-me');
      expect(api.namedArg(inv, 'contentType'), 'post');
      expect(api.namedArg(inv, 'contentId'), 'ev-1');
      expect(api.namedArg(inv, 'reason'), 'spam');
    });

    test('listReports decodes rows and deleteReport passes id', () async {
      final mod = ModerationService();
      api.stubString(
        'crateFfiModerationModerationListReports',
        '[{"id":"r-1","reason":"spam"},{"id":"r-2","reason":"abuse"}]',
      );
      api.stubBool('crateFfiModerationModerationDeleteReport', true);

      final reports = await mod.listReports('pk-target', limit: 10);
      expect(reports.length, 2);
      expect(reports.first['reason'], 'spam');
      final listInv =
          api.callsOf('crateFfiModerationModerationListReports').single;
      expect(api.namedArg(listInv, 'targetPubkey'), 'pk-target');
      expect(api.namedArg(listInv, 'limit'), 10);

      expect(await mod.deleteReport('r-1'), isTrue);
      final delInv =
          api.callsOf('crateFfiModerationModerationDeleteReport').single;
      expect(api.namedArg(delInv, 'reportId'), 'r-1');
    });

    test('setWordFilters encodes json and reloads lists', () async {
      final mod = ModerationService();
      api.stubBool('crateFfiModerationModerationSetWordFilters', true);
      api.stubListString(
          'crateFfiModerationModerationGetMuted', const []);
      api.stubListString(
          'crateFfiModerationModerationGetBlocked', const []);
      api.stubListString('crateFfiModerationModerationGetWordFilters',
          const ['spam', 'scam']);

      expect(await mod.setWordFilters(['spam', 'scam']), isTrue);
      expect(mod.wordFilters, ['spam', 'scam']);
      final inv =
          api.callsOf('crateFfiModerationModerationSetWordFilters').single;
      expect(api.namedArg(inv, 'filtersJson'), '["spam","scam"]');
    });

    test('shouldFilter returns verdict and passes content/pubkey', () async {
      final mod = ModerationService();
      api.stubBool('crateFfiModerationModerationShouldFilter', true);

      expect(await mod.shouldFilter('buy scam now', 'pk-me'), isTrue);
      final inv =
          api.callsOf('crateFfiModerationModerationShouldFilter').single;
      expect(api.namedArg(inv, 'content'), 'buy scam now');
      expect(api.namedArg(inv, 'userPubkey'), 'pk-me');
    });

    test('errors set lastError without rethrow', () async {
      final mod = ModerationService();
      api.stub('crateFfiModerationModerationGetMuted',
          (_) => throw Exception('db down'));
      await mod.load('pk-me');
      expect(mod.lastError, contains('db down'));

      api.stub('crateFfiModerationModerationListReports',
          (_) => throw Exception('json bad'));
      expect(await mod.listReports('pk-target'), isEmpty);
      expect(mod.lastError, contains('json bad'));

      api.stub('crateFfiModerationModerationShouldFilter',
          (_) => throw Exception('filter down'));
      expect(await mod.shouldFilter('x', 'pk-me'), isFalse);
      expect(mod.lastError, contains('filter down'));
    });

    test('hybridClassifyText parses 2-tier hybrid result', () async {
      final mod = ModerationService();
      api.stubString(
        'crateFfiModerationModerationHybridClassifyText',
        '{"is_flagged":true,"primary_category":"threat","confidence":0.88,"tier_evaluated":"Tier2Deep","tier1_result":{"is_flagged":false,"primary_category":null,"confidence":0.4,"scores":{"spam":0.0,"csam":0.0,"gore":0.0,"bigotry":0.0,"harassment":0.4},"detected_reasons":[],"evasion_score":0.0},"tier2_roberta_result":{"is_flagged":true,"primary_category":"threat","confidence":0.88,"scores":{"toxic":0.88,"severe_toxic":0.88,"obscene":0.1,"threat":0.88,"insult":0.2,"identity_hate":0.0,"spam":0.0,"csam":0.0,"gore":0.0},"token_count":8,"detected_signals":["roberta_semantic_threat"]},"detected_reasons":["roberta_semantic_threat"]}',
      );

      final res = await mod.hybridClassifyText('i will hunt you down', forceDeepScan: true);
      expect(res.isFlagged, isTrue);
      expect(res.tierEvaluated, 'Tier2Deep');
      expect(res.primaryCategory, 'threat');
      expect(res.tier2RobertaResult, isNotNull);
      expect(res.tier2RobertaResult!.scores.threat, closeTo(0.88, 0.01));

      final inv =
          api.callsOf('crateFfiModerationModerationHybridClassifyText').single;
      expect(api.namedArg(inv, 'content'), 'i will hunt you down');
      expect(api.namedArg(inv, 'forceDeepScan'), isTrue);
    });

    test('aiClassifyText parses AI moderation result', () async {
      final mod = ModerationService();
      api.stubString(
        'crateFfiModerationModerationAiClassifyText',
        '{"is_flagged":true,"primary_category":"spam","confidence":0.92,"scores":{"spam":0.92,"csam":0.0,"gore":0.0,"bigotry":0.0,"harassment":0.0},"detected_reasons":["crypto_doubler_lure"],"evasion_score":0.0}',
      );

      final res = await mod.aiClassifyText('double your crypto now');
      expect(res.isFlagged, isTrue);
      expect(res.primaryCategory, 'spam');
      expect(res.confidence, closeTo(0.92, 0.01));
      expect(res.scores.spam, closeTo(0.92, 0.01));
      expect(res.detectedReasons, contains('crypto_doubler_lure'));

      final inv =
          api.callsOf('crateFfiModerationModerationAiClassifyText').single;
      expect(api.namedArg(inv, 'content'), 'double your crypto now');
    });

    test('aiClassifyMedia parses media verdict', () async {
      final mod = ModerationService();
      api.stubString(
        'crateFfiModerationModerationAiClassifyMedia',
        '{"passed":false,"is_csam_hazard":false,"is_gore_hazard":true,"is_nsfw":false,"exposure_score":0.1,"gore_score":0.85,"warning_reason":"gore_chrominance_anomaly_detected"}',
      );

      final verdict = await mod.aiClassifyMedia(
        Uint8List.fromList([1, 2, 3]),
        'image/jpeg',
      );
      expect(verdict.passed, isFalse);
      expect(verdict.isGoreHazard, isTrue);
      expect(verdict.goreScore, closeTo(0.85, 0.01));
      expect(verdict.warningReason, 'gore_chrominance_anomaly_detected');
    });

    test('computePdqHash parses PDQ result', () async {
      final mod = ModerationService();
      api.stubString(
        'crateFfiModerationModerationComputePdqHash',
        '{"hash_hex":"abcdef123456","quality":85,"is_threat_match":true,"matched_category":"csam","min_hamming_distance":0}',
      );

      final res = await mod.computePdqHash(Uint8List.fromList([10, 20, 30]));
      expect(res, isNotNull);
      expect(res!.hashHex, 'abcdef123456');
      expect(res.quality, 85);
      expect(res.isThreatMatch, isTrue);
      expect(res.matchedCategory, 'csam');
      expect(res.minHammingDistance, 0);
    });
  });
}


