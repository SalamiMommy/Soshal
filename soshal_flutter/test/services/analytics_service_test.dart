// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/analytics_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-analytics');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('AnalyticsService', () {
    test('computeStats calls backend and returns result', () async {
      const statsResult = '{"posts": 100, "engagement": 42}';
      api.stubString('crateFfiAnalyticsAnalyticsComputeStats', statsResult);

      final analytics = AnalyticsService();
      var notified = 0;
      analytics.addListener(() => notified++);

      final result = await analytics.computeStats();

      expect(result, statsResult);
      expect(notified, 1);
      expect(analytics.lastError, isNull);
    });

    test('computeStats clears previous error', () async {
      const statsResult = '{}';
      api.stubString('crateFfiAnalyticsAnalyticsComputeStats', statsResult);

      final analytics = AnalyticsService();
      analytics.setLastError(Exception('old error'), StackTrace.current);

      await analytics.computeStats();

      expect(analytics.lastError, isNull);
    });

    test('computeStats sets error on exception', () async {
      final analytics = AnalyticsService();
      api.stub('crateFfiAnalyticsAnalyticsComputeStats', (_) {
        throw Exception('Stats computation failed');
      });

      expect(
        () => analytics.computeStats(),
        throwsException,
      );
      expect(analytics.lastError, isNotNull);
      expect(analytics.lastError!.contains('Stats computation'), true);
    });

    test('generateEmbedding sends text and returns embedding', () async {
      const embedding = '[0.1, 0.2, 0.3, 0.4, 0.5]';
      api.stubString('crateFfiAnalyticsAnalyticsSlmGenerateEmbedding', embedding);

      final analytics = AnalyticsService();
      var notified = 0;
      analytics.addListener(() => notified++);

      const text = 'hello world this is interesting';
      final result = await analytics.generateEmbedding(text);

      expect(result, embedding);
      expect(notified, 1);

      final inv = api.callsOf('crateFfiAnalyticsAnalyticsSlmGenerateEmbedding').single;
      expect(api.namedArg(inv, 'text'), text);
    });

    test('generateEmbedding clears previous error', () async {
      const embedding = '[]';
      api.stubString('crateFfiAnalyticsAnalyticsSlmGenerateEmbedding', embedding);

      final analytics = AnalyticsService();
      analytics.setLastError(Exception('old'), StackTrace.current);

      await analytics.generateEmbedding('text');

      expect(analytics.lastError, isNull);
    });

    test('generateEmbedding sets error on exception', () async {
      final analytics = AnalyticsService();
      api.stub('crateFfiAnalyticsAnalyticsSlmGenerateEmbedding', (_) {
        throw Exception('Embedding generation failed');
      });

      expect(
        () => analytics.generateEmbedding('text'),
        throwsException,
      );
      expect(analytics.lastError, isNotNull);
    });

    test('classifyPost sends text and returns classification', () async {
      const classification = '{"sentiment": "positive", "spam": false}';
      api.stubString('crateFfiAnalyticsAnalyticsSlmClassifyPost', classification);

      final analytics = AnalyticsService();
      var notified = 0;
      analytics.addListener(() => notified++);

      const text = 'Great post! Check out my site: http://example.com';
      final result = await analytics.classifyPost(text);

      expect(result, classification);
      expect(notified, 1);

      final inv = api.callsOf('crateFfiAnalyticsAnalyticsSlmClassifyPost').single;
      expect(api.namedArg(inv, 'text'), text);
    });

    test('classifyPost clears previous error', () async {
      const classification = '{}';
      api.stubString('crateFfiAnalyticsAnalyticsSlmClassifyPost', classification);

      final analytics = AnalyticsService();
      analytics.setLastError(Exception('old'), StackTrace.current);

      await analytics.classifyPost('text');

      expect(analytics.lastError, isNull);
    });

    test('classifyPost sets error on exception', () async {
      final analytics = AnalyticsService();
      api.stub('crateFfiAnalyticsAnalyticsSlmClassifyPost', (_) {
        throw Exception('Classification failed');
      });

      expect(
        () => analytics.classifyPost('text'),
        throwsException,
      );
      expect(analytics.lastError, isNotNull);
    });
  });
}
